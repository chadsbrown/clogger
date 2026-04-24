use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use logger_core::{ContestHistoryLookup, DupeChecker, MultChecker, QsoDraft};
use qsolog::{
    core::store::QsoStore,
    op::StoredOp,
    persist::{OpSink, sqlite::SqliteOpSink},
    qso::{ExchangeBlob, QsoDraft as StoreDraft, QsoFlags, QsoRecord},
    types::{Band, Mode, QsoId},
};
use tokio::sync::{mpsc, watch};
use tracing::info;

use crate::contest_history::ContestHistoryIndex;
use crate::scoring::{ContestScorer, ScoreBreakdown, ScoreSummary};

/// A point-in-time view of the log, published by `LogAdapter` on every
/// mutation. Consumers (scoreboard uploader, RTC adapter) subscribe via
/// `watch::Receiver<Arc<LogSnapshot>>` and react without needing
/// synchronous access to the adapter itself. Cloning is cheap: the
/// ordered records are shared via `Arc<Vec<QsoRecord>>`; `ScoreBreakdown`
/// is wrapped in `Arc` for the same reason.
#[derive(Debug, Clone)]
pub struct LogSnapshot {
    /// All QSOs in insertion order, including voided records (their
    /// `flags.is_void` bit is set). Consumers filter as needed.
    /// Backed by `Arc<Vec<_>>` (not `Arc<[_]>`) so the `LogAdapter`
    /// can extend the cache in place via `Arc::make_mut` on the hot
    /// insert path.
    pub records: Arc<Vec<QsoRecord>>,
    /// Per-(band, mode) score breakdown at the moment of this snapshot.
    pub score_breakdown: Arc<ScoreBreakdown>,
    /// Monotonic counter bumped on every mutation. Useful for "did
    /// anything change since last tick?" checks without comparing
    /// records.
    pub score_epoch: u64,
}

/// Persistence strategy for a `LogAdapter`.
///
/// - `None`: in-memory only. Used by `logger-cli` golden tests and any
///   headless run without `db_path`.
/// - `Sync`: direct synchronous SQLite writes via an owned `SqliteOpSink`.
///   Used by `log_adapter`'s own unit tests (which need to read the DB
///   back within the same process) and by any caller that can tolerate
///   the write latency inline.
/// - `Async`: writes are sent to a dedicated `persist_task` over an
///   unbounded mpsc channel. The event loop never awaits on disk I/O.
///   Used by the TUI in production via `bootstrap`.
enum PersistBackend {
    None,
    Sync(SqliteOpSink),
    Async(mpsc::UnboundedSender<Vec<StoredOp>>),
}

pub struct LogAdapter {
    store: QsoStore,
    persist: PersistBackend,
    scorer: Box<dyn ContestScorer>,
    /// The contest this adapter is logging for. All inserted QSOs are
    /// tagged with this id in the qsolog store. A log session always
    /// represents one contest, so this is set at construction time.
    contest_instance_id: u64,
    /// Monotonic counter that increments whenever the score state could
    /// have changed — insert, undo, redo, or an initial rebuild from
    /// disk. Consumers that build expensive score snapshots
    /// (e.g. the scoreboard uploader) compare the current epoch against
    /// the last one they built from and skip work when nothing moved.
    score_epoch: u64,
    /// Per-contest autopopulate index. Tracks the most-recently-logged
    /// received exchange per callsign so the reducer can seed form
    /// fields when the operator re-works a known station. Updated
    /// alongside the scorer on insert/undo/redo.
    contest_history: ContestHistoryIndex,
    /// Optional snapshot publisher. When set, every mutation
    /// (insert/undo/redo) pushes a fresh `LogSnapshot` so that
    /// bootstrap-hosted adapters (scoreboard, RTC) can react without
    /// being driven by the UI event loop. `None` for CLI golden tests
    /// and any caller that doesn't want the allocation overhead.
    change_tx: Option<watch::Sender<Arc<LogSnapshot>>>,
    /// Cached materialization of `store`'s ordered records. Read-only
    /// consumers — GUI panes each frame, snapshot publication, scorer
    /// rebuild — borrow a slice or clone the Arc instead of re-walking
    /// the store and cloning every record.
    ///
    /// Maintained incrementally on the hot `insert` path: `Arc::make_mut`
    /// appends in place when the Arc isn't shared with a subscriber, or
    /// clones the Vec once when it is — same worst-case cost as the
    /// old full rebuild, but O(1) amortized when nobody is holding a
    /// published snapshot. `undo`/`redo` still go through
    /// `refresh_records_cache` because they can reorder arbitrary ids.
    records_cache: Arc<Vec<QsoRecord>>,
}

impl LogAdapter {
    pub fn new(scorer: Box<dyn ContestScorer>, contest_instance_id: u64) -> Self {
        Self {
            store: QsoStore::new(),
            persist: PersistBackend::None,
            scorer,
            contest_instance_id,
            score_epoch: 0,
            contest_history: ContestHistoryIndex::new(),
            change_tx: None,
            records_cache: Arc::new(Vec::<QsoRecord>::new()),
        }
    }

    /// Materialize `store`'s ordered records into `records_cache`.
    /// Call after any mutation that could change the ordered id list
    /// or individual record contents; callers then read via
    /// `records()` / `records_arc()` without re-walking the store.
    fn refresh_records_cache(&mut self) {
        let records: Vec<QsoRecord> = self
            .store
            .ordered_ids()
            .iter()
            .filter_map(|id| self.store.get_cloned(*id))
            .collect();
        self.records_cache = Arc::new(records);
    }

    /// All QSOs in insertion order, including voided records. Cheap
    /// — returns a borrow of the cached slice, no allocation.
    pub fn records(&self) -> &[QsoRecord] {
        &self.records_cache
    }

    /// Cheap `Arc` clone of the cached records (8-byte pointer bump).
    /// Used by `publish_snapshot` and by bootstrap for the initial
    /// `LogSnapshot` seed, so neither path pays for a full Vec copy.
    pub fn records_arc(&self) -> Arc<Vec<QsoRecord>> {
        Arc::clone(&self.records_cache)
    }

    /// Attach a `watch` publisher. After this call every mutation
    /// (insert/undo/redo) also sends a fresh `LogSnapshot` to
    /// subscribers. An initial snapshot is sent immediately so
    /// consumers starting after a DB load see current state. Returns
    /// `self` so bootstrap can chain: `adapter = adapter.with_change_publisher(tx);`.
    pub fn with_change_publisher(mut self, tx: watch::Sender<Arc<LogSnapshot>>) -> Self {
        self.change_tx = Some(tx);
        self.publish_snapshot();
        self
    }

    /// Build and push a snapshot to the watch sender, if configured.
    /// Called after each mutation. Cheap when no sender is attached.
    /// Cheap even with one: the records slice is shared via Arc, not
    /// copied.
    fn publish_snapshot(&self) {
        let Some(tx) = &self.change_tx else { return };
        let snapshot = Arc::new(LogSnapshot {
            records: Arc::clone(&self.records_cache),
            score_breakdown: Arc::new(self.scorer.score_breakdown()),
            score_epoch: self.score_epoch,
        });
        // Watch senders fail to send only when every receiver has been
        // dropped — e.g. the scoreboard adapter shut down. Not an error;
        // just means nobody's listening.
        let _ = tx.send(snapshot);
    }

    /// Open a SQLite log and load its state, using **synchronous** writes.
    /// Preserved for tests and for callers that don't run a tokio runtime.
    /// The TUI uses `open_db_async` instead, which delegates writes to a
    /// `persist_task`.
    pub fn open_db(
        scorer: Box<dyn ContestScorer>,
        contest_instance_id: u64,
        path: &Path,
    ) -> Result<Self> {
        let sink = SqliteOpSink::open(path).map_err(|e| anyhow::anyhow!("open db: {e:?}"))?;
        let store = sink
            .load_store()
            .map_err(|e| anyhow::anyhow!("load store: {e:?}"))?;
        let count = store.ordered_ids().len();
        info!("loaded {count} QSOs from {}", path.display());
        let mut adapter = Self {
            store,
            persist: PersistBackend::Sync(sink),
            scorer,
            contest_instance_id,
            score_epoch: 0,
            contest_history: ContestHistoryIndex::new(),
            change_tx: None,
            records_cache: Arc::new(Vec::<QsoRecord>::new()),
        };
        // Populate scorer state from the loaded log.
        adapter.refresh_records_cache();
        let records = Arc::clone(&adapter.records_cache);
        adapter.scorer.rebuild(&records);
        adapter.contest_history.rebuild(&records);
        adapter.score_epoch = adapter.score_epoch.wrapping_add(1);
        Ok(adapter)
    }

    /// Open a SQLite log and load its state, sending subsequent writes to a
    /// dedicated `persist_task`. The sink is moved into the task; after this
    /// returns, the only way to write is through the channel (which the
    /// adapter owns the sender for). The `LogAdapter::insert` path stays
    /// synchronous-looking to the caller but does a non-blocking
    /// channel send rather than a blocking disk write.
    pub fn open_db_async(
        scorer: Box<dyn ContestScorer>,
        contest_instance_id: u64,
        path: &Path,
        err_tx: mpsc::Sender<logger_core::AppEvent>,
    ) -> Result<Self> {
        let sink = SqliteOpSink::open(path).map_err(|e| anyhow::anyhow!("open db: {e:?}"))?;
        let store = sink
            .load_store()
            .map_err(|e| anyhow::anyhow!("load store: {e:?}"))?;
        let count = store.ordered_ids().len();
        info!("loaded {count} QSOs from {}", path.display());
        // Transfer ownership of the sink to the persist task.
        let persist_tx = crate::persist_task::spawn_persist_task(sink, err_tx);
        let mut adapter = Self {
            store,
            persist: PersistBackend::Async(persist_tx),
            scorer,
            contest_instance_id,
            score_epoch: 0,
            contest_history: ContestHistoryIndex::new(),
            change_tx: None,
            records_cache: Arc::new(Vec::<QsoRecord>::new()),
        };
        adapter.refresh_records_cache();
        let records = Arc::clone(&adapter.records_cache);
        adapter.scorer.rebuild(&records);
        adapter.contest_history.rebuild(&records);
        adapter.score_epoch = adapter.score_epoch.wrapping_add(1);
        Ok(adapter)
    }

    pub fn insert(
        &mut self,
        draft: QsoDraft,
        ts_ms: u64,
        radio_id: u32,
        operator_id: u32,
    ) -> Result<QsoId> {
        let exchange = ExchangeBlob {
            bytes: encode_exchange_pairs(&draft.exchange_pairs)?,
        };

        let store_draft = StoreDraft {
            contest_instance_id: self.contest_instance_id,
            callsign_raw: draft.callsign.clone(),
            callsign_norm: draft.callsign,
            band: to_band(&draft.band),
            mode: to_mode(&draft.mode),
            freq_hz: draft.freq_hz,
            ts_ms,
            radio_id,
            operator_id,
            exchange,
            flags: QsoFlags::default(),
        };

        let (id, _) = self
            .store
            .insert(store_draft)
            .map_err(|e| anyhow::anyhow!("insert failed: {e:?}"))?;

        self.flush_pending_ops()?;

        if let Some(rec) = self.store.get_cloned(id) {
            self.scorer.on_inserted(&rec);
            if let Ok(pairs) = decode_exchange_pairs(&rec.exchange) {
                self.contest_history
                    .on_inserted(&rec.callsign_norm, &pairs);
            }
            Arc::make_mut(&mut self.records_cache).push(rec);
        }
        self.score_epoch = self.score_epoch.wrapping_add(1);
        self.publish_snapshot();

        Ok(id)
    }

    /// All QSOs in insertion order, owned. Prefer `records()` (slice
    /// borrow) internally; this exists for external callers that need
    /// an owned `Vec` (e.g. tests that compare lengths after mutation).
    pub fn ordered_records(&self) -> Vec<QsoRecord> {
        self.records_cache.to_vec()
    }

    pub fn undo(&mut self) -> Result<()> {
        self.store
            .undo()
            .map_err(|e| anyhow::anyhow!("undo failed: {e:?}"))?;
        self.flush_pending_ops()?;
        self.refresh_records_cache();
        let records = Arc::clone(&self.records_cache);
        self.scorer.rebuild(&records);
        self.contest_history.rebuild(&records);
        self.score_epoch = self.score_epoch.wrapping_add(1);
        self.publish_snapshot();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<()> {
        self.store
            .redo()
            .map_err(|e| anyhow::anyhow!("redo failed: {e:?}"))?;
        self.flush_pending_ops()?;
        self.refresh_records_cache();
        let records = Arc::clone(&self.records_cache);
        self.scorer.rebuild(&records);
        self.contest_history.rebuild(&records);
        self.score_epoch = self.score_epoch.wrapping_add(1);
        self.publish_snapshot();
        Ok(())
    }

    /// Drain any pending ops from the store and flush them via the
    /// configured persistence backend. Called after operations that mutate
    /// the store. `None` is a no-op; `Sync` writes synchronously and can
    /// return an error; `Async` sends to the persist task and never blocks
    /// — failures there surface asynchronously via `PersistError`
    /// `AppEvent`s on the main event channel.
    fn flush_pending_ops(&mut self) -> Result<()> {
        match &mut self.persist {
            PersistBackend::None => Ok(()),
            PersistBackend::Sync(sink) => {
                let ops = self.store.drain_pending_ops();
                if !ops.is_empty() {
                    sink.append_ops(&ops)
                        .map_err(|e| anyhow::anyhow!("persist failed: {e:?}"))?;
                }
                Ok(())
            }
            PersistBackend::Async(tx) => {
                let ops = self.store.drain_pending_ops();
                if !ops.is_empty() {
                    // Unbounded send — only fails if the receiver dropped,
                    // which only happens at shutdown. Ignore errors: the
                    // in-memory store update already succeeded and will
                    // stay consistent for the lifetime of the process.
                    let _ = tx.send(ops);
                }
                Ok(())
            }
        }
    }

    pub fn score_summary(&self) -> ScoreSummary {
        self.scorer.score_summary()
    }

    pub fn score_breakdown(&self) -> ScoreBreakdown {
        self.scorer.score_breakdown()
    }

    /// Monotonic counter that increments whenever the score state could
    /// have changed. Consumers building expensive snapshots (e.g. the
    /// scoreboard uploader) can compare against a previously-seen value
    /// to avoid rebuilding when nothing moved.
    pub fn score_epoch(&self) -> u64 {
        self.score_epoch
    }

    /// Returns the highest serial number found in logged exchange_pairs, or 0 if none.
    pub fn max_sent_serial(&self) -> u32 {
        self.records()
            .iter()
            .filter(|r| !r.flags.is_void)
            .filter_map(|r| {
                decode_exchange_pairs(&r.exchange)
                    .ok()?
                    .into_iter()
                    .find(|(k, _)| k == "serial")
                    .and_then(|(_, v)| v.parse::<u32>().ok())
            })
            .max()
            .unwrap_or(0)
    }
}

impl DupeChecker for LogAdapter {
    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        self.scorer.is_dupe(call_norm, band, mode)
    }
}

impl MultChecker for LogAdapter {
    fn is_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        self.scorer.would_be_new_mult(call_norm, band, mode)
    }
}

impl ContestHistoryLookup for LogAdapter {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
        self.contest_history.lookup(call_norm)
    }
}

pub fn decode_exchange_pairs(blob: &ExchangeBlob) -> Result<Vec<(String, String)>> {
    serde_json::from_slice(&blob.bytes).context("decode exchange bytes")
}

fn encode_exchange_pairs(pairs: &[(String, String)]) -> Result<Vec<u8>> {
    serde_json::to_vec(pairs).context("encode exchange bytes")
}

fn to_band(s: &str) -> Band {
    match s.to_ascii_lowercase().as_str() {
        "160m" => Band::B160m,
        "80m" => Band::B80m,
        "40m" => Band::B40m,
        "20m" => Band::B20m,
        "15m" => Band::B15m,
        "10m" => Band::B10m,
        _ => Band::Other,
    }
}

fn to_mode(s: &str) -> Mode {
    match s.to_ascii_uppercase().as_str() {
        "CW" => Mode::CW,
        "SSB" => Mode::SSB,
        "DIGITAL" => Mode::Digital,
        _ => Mode::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::LogAdapter;
    use crate::scoring::scorer_for_contest;
    use contest_engine::spec::Value;
    use logger_core::{NoCallHistory, contest_from_id};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Build a cqww config that satisfies the spec's required config_fields.
    /// cqww requires `my_cq_zone`; without it, `scorer_for_contest` now errors
    /// at construction instead of silently starting with a dead session.
    fn cqww_test_config() -> HashMap<String, Value> {
        let mut cfg = HashMap::new();
        cfg.insert("my_cq_zone".to_string(), Value::Int(4));
        cfg
    }

    fn sample_draft() -> logger_core::QsoDraft {
        logger_core::QsoDraft {
            contest_id: "cqww".to_string(),
            callsign: "K1ABC".to_string(),
            band: "20m".to_string(),
            mode: "CW".to_string(),
            freq_hz: 14_025_000,
            exchange_schema_id: 1,
            exchange_pairs: vec![
                ("rst".to_string(), "599".to_string()),
                ("zone".to_string(), "5".to_string()),
            ],
        }
    }

    /// Regression guard: `is_dupe` must reflect the operator's actual
    /// work even when contest-engine rejects the stored exchange on
    /// replay (e.g. empty pairs, or a spec-validation failure). Before
    /// this was fixed, restarting mid-contest painted some already-
    /// worked stations as un-worked on the bandmap because `loose_dupes`
    /// was only populated inside the `Ok` arm of `apply_qso_with_mode`.
    #[test]
    fn is_dupe_flags_call_even_when_exchange_is_empty() {
        use logger_core::DupeChecker;

        let contest = contest_from_id("cqww").expect("cqww contest");
        let scorer = scorer_for_contest(
            contest.as_ref(),
            cqww_test_config(),
            Arc::new(NoCallHistory),
            None,
        )
        .expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, contest.contest_instance_id());

        // Empty exchange_pairs → `raw_exchange_for_record` returns None
        // → contest-engine's apply_qso_with_mode never runs. The dupe
        // indicator must still fire on the (call, band, mode) tuple.
        let empty_draft = logger_core::QsoDraft {
            contest_id: "cqww".to_string(),
            callsign: "K1ABC".to_string(),
            band: "20m".to_string(),
            mode: "CW".to_string(),
            freq_hz: 14_025_000,
            exchange_schema_id: 1,
            exchange_pairs: Vec::new(),
        };
        adapter.insert(empty_draft, 1_000, 1, 1).expect("insert");

        assert!(
            adapter.is_dupe("K1ABC", "20m", "CW"),
            "is_dupe must fire on logged call/band/mode regardless of \
             whether contest-engine could replay the exchange"
        );
    }

    /// Regression guard: `scorer_for_contest` must return an Err (not a
    /// silent fallback) when a state QP is given an empty config that
    /// doesn't satisfy its required `my_is_<state>` field. Prior behavior
    /// was to log a warn and return a dead scorer, which silently dropped
    /// every QSO for the whole contest.
    #[test]
    fn state_qp_missing_required_config_errors_at_init() {
        let contest = contest_from_id("moqp").expect("moqp contest");
        let result = scorer_for_contest(contest.as_ref(), HashMap::new(), Arc::new(NoCallHistory), None);
        let err = match result {
            Ok(_) => panic!("empty moqp config must error, not succeed"),
            Err(e) => e,
        };
        let msg = format!("{err:#}");
        assert!(
            msg.contains("moqp"),
            "error message should name the spec: {msg}"
        );
        assert!(
            msg.contains("my_is") || msg.contains("[station]"),
            "error message should hint at the missing required field: {msg}"
        );
    }

    #[test]
    fn undo_redo_placeholder_roundtrip() {
        let contest = contest_from_id("cqww").expect("cqww contest");
        let scorer = scorer_for_contest(contest.as_ref(), cqww_test_config(), Arc::new(NoCallHistory), None).expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, 1);

        adapter.insert(sample_draft(), 1, 1, 1).expect("insert");
        assert_eq!(adapter.ordered_records().len(), 1);
        adapter.undo().expect("undo");
        assert!(adapter.ordered_records()[0].flags.is_void);
        adapter.redo().expect("redo");
        assert!(!adapter.ordered_records()[0].flags.is_void);
        assert_eq!(adapter.ordered_records().len(), 1);
    }

    #[test]
    fn undo_persists_to_sqlite_across_reload() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let db_path = tempdir.path().join("undo.db");

        let contest = contest_from_id("cqww").expect("cqww contest");

        {
            let scorer =
                scorer_for_contest(contest.as_ref(), cqww_test_config(), Arc::new(NoCallHistory), None).expect("scorer init");
            let mut adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("open_db");
            adapter.insert(sample_draft(), 1, 1, 1).expect("insert");
            adapter.undo().expect("undo");
            assert!(adapter.ordered_records()[0].flags.is_void);
        }

        {
            let scorer =
                scorer_for_contest(contest.as_ref(), cqww_test_config(), Arc::new(NoCallHistory), None).expect("scorer init");
            let adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("reopen_db");
            let records = adapter.ordered_records();
            assert_eq!(records.len(), 1, "record should still exist after reload");
            assert!(
                records[0].flags.is_void,
                "undo should have persisted across reload"
            );
        }
    }

    #[test]
    fn redo_persists_to_sqlite_across_reload() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let db_path = tempdir.path().join("redo.db");

        let contest = contest_from_id("cqww").expect("cqww contest");

        {
            let scorer =
                scorer_for_contest(contest.as_ref(), cqww_test_config(), Arc::new(NoCallHistory), None).expect("scorer init");
            let mut adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("open_db");
            adapter.insert(sample_draft(), 1, 1, 1).expect("insert");
            adapter.undo().expect("undo");
            adapter.redo().expect("redo");
            assert!(!adapter.ordered_records()[0].flags.is_void);
        }

        {
            let scorer =
                scorer_for_contest(contest.as_ref(), cqww_test_config(), Arc::new(NoCallHistory), None).expect("scorer init");
            let adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("reopen_db");
            let records = adapter.ordered_records();
            assert_eq!(records.len(), 1);
            assert!(
                !records[0].flags.is_void,
                "redo should have persisted across reload"
            );
        }
    }

    /// Minimal in-test call-history stub. Lookup returns pre-wired rows.
    struct TestCallHistory {
        rows: HashMap<String, Vec<(String, String)>>,
    }

    impl TestCallHistory {
        fn new(rows: &[(&str, &[(&str, &str)])]) -> Self {
            let rows = rows
                .iter()
                .map(|(call, pairs)| {
                    (
                        call.to_ascii_uppercase(),
                        pairs
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                    )
                })
                .collect();
            Self { rows }
        }
    }

    impl logger_core::CallHistoryLookup for TestCallHistory {
        fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
            self.rows.get(call_norm).cloned()
        }
    }

    fn ns_sprint_draft(call: &str, nr: &str, name: &str, loc: &str) -> logger_core::QsoDraft {
        logger_core::QsoDraft {
            contest_id: "ns_sprint".to_string(),
            callsign: call.to_string(),
            band: "20m".to_string(),
            mode: "CW".to_string(),
            freq_hz: 14_025_000,
            exchange_schema_id: 7,
            exchange_pairs: vec![
                ("nr".to_string(), nr.to_string()),
                ("name".to_string(), name.to_string()),
                ("loc".to_string(), loc.to_string()),
            ],
        }
    }

    fn ns_sprint_test_config() -> HashMap<String, Value> {
        let mut cfg = HashMap::new();
        cfg.insert("my_name".to_string(), Value::Text("CHRIS".to_string()));
        cfg.insert("my_loc".to_string(), Value::Text("MA".to_string()));
        cfg
    }

    /// The core regression: after one NH QSO is logged on 20m, an unknown
    /// call whose `.ch` row declares State=NH should NOT be flagged as a
    /// new mult on 20m (already worked). An unknown call with State=ON
    /// still is. An unknown call with no `.ch` row keeps the optimistic
    /// default. Exercises ns_sprint because its history_mapping maps
    /// State→loc (the mult-driving field).
    #[test]
    fn bandmap_mult_downgrade_uses_call_history() {
        use logger_core::MultChecker;

        let contest = contest_from_id("ns_sprint").expect("ns_sprint contest");
        let history = Arc::new(TestCallHistory::new(&[
            ("N2BB", &[("Name", "BOB"), ("State", "NH")]),
            ("N3CC", &[("Name", "SAM"), ("State", "ON")]),
        ]));
        let scorer = scorer_for_contest(
            contest.as_ref(),
            ns_sprint_test_config(),
            history,
            None,
        )
        .expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, contest.contest_instance_id());

        // Baseline: no QSOs logged — every unknown call (with or without
        // .ch hit) reads as a potential new mult.
        assert!(adapter.is_new_mult("N2BB", "20m", "CW"));
        assert!(adapter.is_new_mult("N3CC", "20m", "CW"));
        assert!(adapter.is_new_mult("N4DD", "20m", "CW"));

        // Log one NH station. NH is now in the worked-mult set for 20m.
        adapter
            .insert(ns_sprint_draft("N1AA", "1", "JOE", "NH"), 1_000, 1, 1)
            .expect("insert");

        // N2BB's .ch row says NH — downgraded to not-a-new-mult.
        assert!(
            !adapter.is_new_mult("N2BB", "20m", "CW"),
            "NH already worked on 20m; N2BB's .ch=NH should downgrade"
        );
        // Other band: NH isn't logged on 40m — still a mult.
        assert!(
            adapter.is_new_mult("N2BB", "40m", "CW"),
            "NH worked only on 20m; 40m lookup should stay optimistic"
        );
        // N3CC's .ch row says ON — still a new mult.
        assert!(
            adapter.is_new_mult("N3CC", "20m", "CW"),
            "ON not worked on 20m; N3CC should stay as a new-mult candidate"
        );
        // Unknown call with no .ch hit — falls back to optimistic default.
        assert!(
            adapter.is_new_mult("N4DD", "20m", "CW"),
            "N4DD has no .ch row; falls back to optimistic default"
        );
    }

    /// The classify cache must invalidate on log mutation: a verdict
    /// cached before the new QSO is inserted cannot outlive the QSO that
    /// changed the mult set.
    #[test]
    fn classify_cache_invalidates_on_insert() {
        use logger_core::MultChecker;

        let contest = contest_from_id("ns_sprint").expect("ns_sprint contest");
        let history = Arc::new(TestCallHistory::new(&[(
            "N2BB",
            &[("Name", "BOB"), ("State", "NH")],
        )]));
        let scorer = scorer_for_contest(
            contest.as_ref(),
            ns_sprint_test_config(),
            history,
            None,
        )
        .expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, contest.contest_instance_id());

        // Pre-insert: N2BB.ch=NH reads as a mult (nothing worked yet).
        // This populates the cache with is_new_mult=true.
        assert!(adapter.is_new_mult("N2BB", "20m", "CW"));

        // Logging an NH QSO must invalidate the cached "true" above.
        adapter
            .insert(ns_sprint_draft("N1AA", "1", "JOE", "NH"), 1_000, 1, 1)
            .expect("insert");

        assert!(
            !adapter.is_new_mult("N2BB", "20m", "CW"),
            "cache should be invalidated on insert; N2BB must now read as non-mult"
        );
    }

    /// Two QSOs for the same call with different received exchanges:
    /// the second one wins on lookup (last-write-wins, per the plan).
    #[test]
    fn contest_history_lookup_returns_latest_exchange() {
        use logger_core::ContestHistoryLookup;

        let contest = contest_from_id("ns_sprint").expect("ns_sprint");
        let scorer = scorer_for_contest(
            contest.as_ref(),
            ns_sprint_test_config(),
            Arc::new(NoCallHistory),
            None,
        )
        .expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, contest.contest_instance_id());

        adapter
            .insert(ns_sprint_draft("N3QE", "100", "TINA", "NH"), 1_000, 1, 1)
            .expect("first insert");
        adapter
            .insert(ns_sprint_draft("N3QE", "101", "TIM", "NH"), 2_000, 1, 1)
            .expect("second insert");

        let pairs = ContestHistoryLookup::lookup(&adapter, "N3QE").expect("hit");
        let map: HashMap<&str, &str> =
            pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("name"), Some(&"TIM"));
        assert_eq!(map.get("loc"), Some(&"NH"));
    }

    /// Stored exchange fields `nr` and `rst` (and the operator's sent
    /// `serial`) must not appear in the lookup output — replaying them
    /// would auto-populate stale counter values.
    #[test]
    fn contest_history_filters_blocklisted_fields() {
        use logger_core::ContestHistoryLookup;

        let contest = contest_from_id("ns_sprint").expect("ns_sprint");
        let scorer = scorer_for_contest(
            contest.as_ref(),
            ns_sprint_test_config(),
            Arc::new(NoCallHistory),
            None,
        )
        .expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, contest.contest_instance_id());

        adapter
            .insert(ns_sprint_draft("K1ABC", "42", "CHAD", "MA"), 1_000, 1, 1)
            .expect("insert");

        let pairs = ContestHistoryLookup::lookup(&adapter, "K1ABC").expect("hit");
        let keys: std::collections::HashSet<&str> =
            pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains("name"));
        assert!(keys.contains("loc"));
        assert!(!keys.contains("nr"), "nr must be blocklisted");
        assert!(!keys.contains("rst"), "rst must be blocklisted");
        assert!(!keys.contains("serial"));
    }

    /// Undo must remove the call from the lookup index; redo restores it.
    #[test]
    fn contest_history_respects_undo_redo() {
        use logger_core::ContestHistoryLookup;

        let contest = contest_from_id("ns_sprint").expect("ns_sprint");
        let scorer = scorer_for_contest(
            contest.as_ref(),
            ns_sprint_test_config(),
            Arc::new(NoCallHistory),
            None,
        )
        .expect("scorer init");
        let mut adapter = LogAdapter::new(scorer, contest.contest_instance_id());

        adapter
            .insert(ns_sprint_draft("W1AW", "1", "HIRAM", "CT"), 1_000, 1, 1)
            .expect("insert");
        assert!(ContestHistoryLookup::lookup(&adapter, "W1AW").is_some());

        adapter.undo().expect("undo");
        assert!(
            ContestHistoryLookup::lookup(&adapter, "W1AW").is_none(),
            "undo must drop the call from the contest-history index"
        );

        adapter.redo().expect("redo");
        assert!(
            ContestHistoryLookup::lookup(&adapter, "W1AW").is_some(),
            "redo must restore the index entry"
        );
    }
}
