use std::path::Path;

use anyhow::{Context, Result};
use logger_core::{DupeChecker, MultChecker, QsoDraft};
use qsolog::{
    core::store::QsoStore,
    persist::{OpSink, sqlite::SqliteOpSink},
    qso::{ExchangeBlob, QsoDraft as StoreDraft, QsoFlags, QsoRecord},
    types::{Band, Mode, QsoId},
};
use tracing::info;

use crate::scoring::{ContestScorer, ScoreBreakdown, ScoreSummary};

pub struct LogAdapter {
    store: QsoStore,
    sink: Option<SqliteOpSink>,
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
}

impl LogAdapter {
    pub fn new(scorer: Box<dyn ContestScorer>, contest_instance_id: u64) -> Self {
        Self {
            store: QsoStore::new(),
            sink: None,
            scorer,
            contest_instance_id,
            score_epoch: 0,
        }
    }

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
            sink: Some(sink),
            scorer,
            contest_instance_id,
            score_epoch: 0,
        };
        // Populate scorer state from the loaded log
        let records = adapter.ordered_records();
        adapter.scorer.rebuild(&records);
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

        if let Some(rec) = self.store.get(id) {
            self.scorer.on_inserted(rec);
        }
        self.score_epoch = self.score_epoch.wrapping_add(1);

        Ok(id)
    }

    pub fn ordered_records(&self) -> Vec<QsoRecord> {
        self.store
            .ordered_ids()
            .iter()
            .filter_map(|id| self.store.get_cloned(*id))
            .collect()
    }

    pub fn undo(&mut self) -> Result<()> {
        self.store
            .undo()
            .map_err(|e| anyhow::anyhow!("undo failed: {e:?}"))?;
        self.flush_pending_ops()?;
        let records = self.ordered_records();
        self.scorer.rebuild(&records);
        self.score_epoch = self.score_epoch.wrapping_add(1);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<()> {
        self.store
            .redo()
            .map_err(|e| anyhow::anyhow!("redo failed: {e:?}"))?;
        self.flush_pending_ops()?;
        let records = self.ordered_records();
        self.scorer.rebuild(&records);
        self.score_epoch = self.score_epoch.wrapping_add(1);
        Ok(())
    }

    /// Drain any pending ops from the store and append them to the SQLite sink
    /// if one is attached. Called after operations that mutate the store.
    fn flush_pending_ops(&mut self) -> Result<()> {
        if let Some(sink) = &mut self.sink {
            let ops = self.store.drain_pending_ops();
            if !ops.is_empty() {
                sink.append_ops(&ops)
                    .map_err(|e| anyhow::anyhow!("persist failed: {e:?}"))?;
            }
        }
        Ok(())
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
        self.ordered_records()
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
    use logger_core::contest_from_id;

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

    #[test]
    fn undo_redo_placeholder_roundtrip() {
        let contest = contest_from_id("cqww").expect("cqww contest");
        let scorer = scorer_for_contest(contest.as_ref(), 4, &std::collections::HashMap::new());
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
                scorer_for_contest(contest.as_ref(), 4, &std::collections::HashMap::new());
            let mut adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("open_db");
            adapter.insert(sample_draft(), 1, 1, 1).expect("insert");
            adapter.undo().expect("undo");
            assert!(adapter.ordered_records()[0].flags.is_void);
        }

        {
            let scorer =
                scorer_for_contest(contest.as_ref(), 4, &std::collections::HashMap::new());
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
                scorer_for_contest(contest.as_ref(), 4, &std::collections::HashMap::new());
            let mut adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("open_db");
            adapter.insert(sample_draft(), 1, 1, 1).expect("insert");
            adapter.undo().expect("undo");
            adapter.redo().expect("redo");
            assert!(!adapter.ordered_records()[0].flags.is_void);
        }

        {
            let scorer =
                scorer_for_contest(contest.as_ref(), 4, &std::collections::HashMap::new());
            let adapter = LogAdapter::open_db(scorer, 1, &db_path).expect("reopen_db");
            let records = adapter.ordered_records();
            assert_eq!(records.len(), 1);
            assert!(
                !records[0].flags.is_void,
                "redo should have persisted across reload"
            );
        }
    }
}
