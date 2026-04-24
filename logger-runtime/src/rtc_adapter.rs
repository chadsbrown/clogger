//! Real Time Contest (RTC) uploader adapter.
//!
//! Subscribes to `LogSnapshot` updates and posts `<dynamicresults>` +
//! per-QSO containers to the HamScore RTC endpoint on a mandatory
//! 2-minute cadence (fixed by the spec; not configurable). Handles
//! CFM tracking across restarts via a JSON sidecar.
//!
//! Spec: `~/Downloads/RTC_Specification_2.4_XML.pdf` (rev 2.4-xml, Jan 2025).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use qsolog::qso::QsoRecord;
use qsolog::types::QsoId;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::RtcSpawnConfig;
use crate::log_adapter::LogSnapshot;
use crate::rtc_xml::{
    ContactData, PendingContact, RtcQth, qso_to_contact_data, serialize_dynamicresults_body,
    serialize_envelope,
};
use crate::scoreboard_adapter::ScoreboardSnapshot;

/// Spec-mandated interval. "The client must send a posting every 2
/// minutes, even if there are no changes in the contest log. This
/// interval must not be configurable."
const RTC_POST_INTERVAL: Duration = Duration::from_secs(120);

/// Status reported by the adapter back to UIs.
///
/// `Idle`        - adapter spawned, no tick has fired yet.
/// `Ok`          - last post succeeded with CFM (or OK if no changes).
/// `Failing`     - last post failed (network error, non-success HTTP, or
///                server returned `{"Status":"Error",...}`).
/// `Unsupported` - server returned "Contest not supported"; adapter
///                keeps retrying at its normal cadence in case the server
///                is reconfigured mid-contest.
/// `NoChanges`   - last tick had nothing to post beyond `<dynamicresults>`
///                and the server responded OK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcStatus {
    Idle,
    Ok,
    Failing,
    Unsupported,
    NoChanges,
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the RTC adapter. Returns a status receiver; the task runs
/// until every receiver is dropped.
pub fn spawn_rtc_adapter(
    cfg: RtcSpawnConfig,
    log_rx: watch::Receiver<Arc<LogSnapshot>>,
) -> watch::Receiver<RtcStatus> {
    let (status_tx, status_rx) = watch::channel(RtcStatus::Idle);
    tokio::spawn(adapter_task(cfg, log_rx, status_tx));
    status_rx
}

// ---------------------------------------------------------------------------
// Sidecar state
// ---------------------------------------------------------------------------

/// Per-log CFM tracking state, persisted to a JSON sidecar alongside
/// the qsolog SQLite DB. Loaded at adapter start; rewritten
/// (atomically, tmp + rename) after every successful CFM.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SidecarState {
    #[serde(default = "default_version")]
    version: u32,
    contest_instance_id: u64,
    /// QSO IDs (qsolog's `QsoId`, u64) the server has CFMed.
    #[serde(default)]
    sent_ids: HashSet<QsoId>,
    /// For each CFMed QSO, the void flag at the moment of CFM.
    /// When the live record's void flag differs, emit a
    /// `<contactreplace>` with `<x-qso>` to sync.
    #[serde(default)]
    sent_void_state: HashMap<QsoId, bool>,
}

fn default_version() -> u32 {
    1
}

impl SidecarState {
    fn empty(contest_instance_id: u64) -> Self {
        Self {
            version: 1,
            contest_instance_id,
            sent_ids: HashSet::new(),
            sent_void_state: HashMap::new(),
        }
    }

    fn load_or_empty(path: &PathBuf, contest_instance_id: u64) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<SidecarState>(&bytes) {
                Ok(s) if s.contest_instance_id == contest_instance_id => s,
                Ok(_) => {
                    info!(
                        "rtc sidecar at {} is from a different contest; starting fresh",
                        path.display()
                    );
                    Self::empty(contest_instance_id)
                }
                Err(e) => {
                    warn!(
                        "rtc sidecar at {} is corrupt ({e}); starting fresh",
                        path.display()
                    );
                    Self::empty(contest_instance_id)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Self::empty(contest_instance_id)
            }
            Err(e) => {
                warn!("rtc sidecar read at {} failed: {e}", path.display());
                Self::empty(contest_instance_id)
            }
        }
    }

    fn write_atomic(&self, path: &PathBuf) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("rtc-state.json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }
}

// ---------------------------------------------------------------------------
// Task body
// ---------------------------------------------------------------------------

async fn adapter_task(
    cfg: RtcSpawnConfig,
    log_rx: watch::Receiver<Arc<LogSnapshot>>,
    status_tx: watch::Sender<RtcStatus>,
) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("rtc: failed to build reqwest client: {e}; adapter exiting");
            let _ = status_tx.send(RtcStatus::Failing);
            return;
        }
    };

    let qth = RtcQth::from_config(&cfg.http);
    let mut sidecar = SidecarState::load_or_empty(&cfg.sidecar_path, cfg.contest_instance_id);

    let mut interval = tokio::time::interval(RTC_POST_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        interval.tick().await;

        // Snapshot current log + the identity we'll post under.
        let snapshot = log_rx.borrow().clone();
        let dyn_body = {
            // Reuse scoreboard's score breakdown shape. For contests
            // where a category isn't configured the adapter wouldn't
            // have been spawned, so these identity fields are always
            // meaningful.
            let score_snap = ScoreboardSnapshot {
                cabrillo_id: cfg.contest_rtc_id.clone(),
                call: cfg.my_call.clone(),
                ops: cfg.my_call.clone(),
                category: cfg.http_category_placeholder(),
                breakdown: (*snapshot.score_breakdown).clone(),
            };
            serialize_dynamicresults_body(&score_snap, Some(&qth))
        };

        // Compute pending deltas
        let (pending, newly_sent_ids, new_void_state) =
            compute_pending(&sidecar, &snapshot.records, cfg.contest_instance_id);

        let xml = serialize_envelope(&dyn_body, &pending);

        match post_and_interpret(&client, &cfg, &xml).await {
            PostOutcome::Ok => {
                let _ = status_tx.send(RtcStatus::NoChanges);
            }
            PostOutcome::Cfm => {
                for id in newly_sent_ids {
                    sidecar.sent_ids.insert(id);
                }
                sidecar.sent_void_state = new_void_state;
                if let Err(e) = sidecar.write_atomic(&cfg.sidecar_path) {
                    warn!(
                        "rtc: sidecar write at {} failed: {e}",
                        cfg.sidecar_path.display()
                    );
                }
                let _ = status_tx.send(RtcStatus::Ok);
            }
            PostOutcome::ResyncLog => {
                // Server asked for a full resync. Clear state; next
                // tick will produce deletelog + re-upload every QSO.
                info!("rtc: server requested ResyncLog; clearing sidecar");
                sidecar = SidecarState::empty(cfg.contest_instance_id);
                // Persist the wipe so a restart before the next tick
                // doesn't accidentally treat everything as already sent.
                let _ = sidecar.write_atomic(&cfg.sidecar_path);
                let _ = status_tx.send(RtcStatus::Ok);
            }
            PostOutcome::Unsupported => {
                let _ = status_tx.send(RtcStatus::Unsupported);
            }
            PostOutcome::Failing => {
                let _ = status_tx.send(RtcStatus::Failing);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Delta computation
// ---------------------------------------------------------------------------

/// Walk the live record set and compare against the CFMed state.
/// Returns:
///  - the list of `PendingContact` containers to include in the next
///    post (empty when nothing has changed);
///  - the set of QSO IDs that will be newly CFMed on a successful
///    response (so we can update the sidecar atomically);
///  - the void-state map that will replace `sidecar.sent_void_state`
///    on CFM (flat copy of every record's current void flag, keyed
///    by id — simpler than tracking diffs).
fn compute_pending(
    sidecar: &SidecarState,
    records: &[Arc<QsoRecord>],
    contest_instance_id: u64,
) -> (Vec<PendingContact>, HashSet<QsoId>, HashMap<QsoId, bool>) {
    let mut pending = Vec::new();
    let mut newly_sent = HashSet::new();
    let mut new_void_state = HashMap::new();

    for rec in records {
        new_void_state.insert(rec.id, rec.flags.is_void);

        if !sidecar.sent_ids.contains(&rec.id) {
            // New QSO → contactinfo. Include x-qso only if already
            // voided (unusual but possible if the operator undid
            // within one 2-minute window).
            let Some(data) = qso_to_contact_data(
                rec,
                contest_instance_id,
                None, // cty lookup not yet wired; TODO: pass CtyDb in
                rec.flags.is_void,
            ) else {
                continue;
            };
            pending.push(PendingContact::Info(with_optional_x_qso(
                data,
                rec.flags.is_void,
                false,
            )));
            newly_sent.insert(rec.id);
            continue;
        }

        // Already CFMed. If the void flag has toggled, emit a
        // contactreplace with the new x-qso value.
        let prior_void = sidecar.sent_void_state.get(&rec.id).copied().unwrap_or(false);
        if prior_void != rec.flags.is_void {
            let Some(data) = qso_to_contact_data(
                rec,
                contest_instance_id,
                None,
                true, // include x-qso explicitly on replaces
            ) else {
                continue;
            };
            pending.push(PendingContact::Replace(data));
        }
    }

    (pending, newly_sent, new_void_state)
}

/// Override the x-qso flag on fresh contactinfo: omit (None) for
/// non-voided fresh logs; emit `<x-qso>1` for QSOs that were voided
/// before their first upload (rare but possible within a 2-minute
/// window). Replaces always carry x-qso, which is handled at the
/// call site.
fn with_optional_x_qso(mut data: ContactData, is_void: bool, force: bool) -> ContactData {
    data.x_qso = if force || is_void { Some(is_void) } else { None };
    data
}

// ---------------------------------------------------------------------------
// HTTP + response parsing
// ---------------------------------------------------------------------------

enum PostOutcome {
    /// 200 OK with `{"Status":"OK"}` — the post contained no QSO
    /// changes; nothing to CFM.
    Ok,
    /// 200 OK with `{"Status":"CFM"}` — every contained container is
    /// accepted. Also used for CFM-with-warning (the server
    /// acknowledges the upload but flags an exchange error).
    Cfm,
    /// 200 OK with `{"Status":"ResyncLog"}` — server wants the client
    /// to wipe its local view and re-upload the full log.
    ResyncLog,
    /// 200 OK with `{"Status":"Error","Description":"Contest not
    /// supported"}` — treat distinctly so the UI shows a specific
    /// "RTC:N/A" status.
    Unsupported,
    /// Any other condition: non-200, transport error, JSON parse
    /// failure, or `{"Status":"Error",...}` with some other
    /// description. The adapter retains the current sidecar state,
    /// so the same deltas retry next tick.
    Failing,
}

async fn post_and_interpret(
    client: &reqwest::Client,
    cfg: &RtcSpawnConfig,
    xml: &str,
) -> PostOutcome {
    let res = client
        .post(&cfg.http.url)
        .basic_auth(&cfg.my_call, Some(&cfg.http.password))
        .header("Content-Type", "application/xml")
        .header("User-Agent", &cfg.user_agent)
        .body(xml.to_string())
        .send()
        .await;

    let resp = match res {
        Ok(r) => r,
        Err(e) => {
            warn!("rtc POST to {} failed: {e}", cfg.http.url);
            return PostOutcome::Failing;
        }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        warn!(
            "rtc POST to {} HTTP {status}; body={body}",
            cfg.http.url
        );
        return PostOutcome::Failing;
    }

    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            warn!("rtc response JSON parse failed ({e}); body={body}");
            return PostOutcome::Failing;
        }
    };

    let status_str = parsed.get("Status").and_then(|v| v.as_str()).unwrap_or("");
    let description = parsed
        .get("Description")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match status_str {
        "OK" => PostOutcome::Ok,
        "CFM" => {
            if !description.is_empty() {
                warn!("rtc CFM with warning: {description}");
            }
            PostOutcome::Cfm
        }
        "ResyncLog" => PostOutcome::ResyncLog,
        "Error" => {
            if description.eq_ignore_ascii_case("Contest not supported") {
                warn!("rtc server rejected contest as unsupported");
                PostOutcome::Unsupported
            } else {
                warn!("rtc server error: {description}");
                PostOutcome::Failing
            }
        }
        other => {
            warn!("rtc unknown response Status='{other}'; body={body}");
            PostOutcome::Failing
        }
    }
}

// ---------------------------------------------------------------------------
// Placeholder: score category for RTC dynamicresults
// ---------------------------------------------------------------------------
//
// The RTC `<dynamicresults>` block requires the same `<class .../>`
// element the scoreboard path emits, which means we need a
// `CategoryConfig`. The user-supplied `[rtc]` section doesn't carry
// class info — it's shared with the scoreboard path, which does. The
// UI wiring ensures RTC is only enabled when a category is available
// (see `compose_rtc` in each UI), so at spawn time we have one.
//
// For now we stash the category on `RtcSpawnConfig` alongside the
// other identity — see the `http_category_placeholder` method on
// `RtcSpawnConfig` below.

impl RtcSpawnConfig {
    fn http_category_placeholder(&self) -> crate::config::CategoryConfig {
        self.category.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use qsolog::qso::{ExchangeBlob, QsoFlags, QsoRecord};
    use qsolog::types::{Band, Mode};

    fn record(id: u64, is_void: bool) -> Arc<QsoRecord> {
        let pairs: Vec<(String, String)> =
            vec![("name".to_string(), "ALEX".to_string()),
                 ("xchg".to_string(), "NJ".to_string()),
                 ("sent_name".to_string(), "VIC".to_string()),
                 ("sent_xchg".to_string(), "2212".to_string())];
        Arc::new(QsoRecord {
            id,
            contest_instance_id: 1,
            callsign_raw: format!("K{id}BB"),
            callsign_norm: format!("K{id}BB"),
            band: Band::B20m,
            mode: Mode::CW,
            freq_hz: 14_254_930,
            ts_ms: 1_718_028_709_000,
            radio_id: 0,
            operator_id: 0,
            exchange: ExchangeBlob {
                bytes: serde_json::to_vec(&pairs).unwrap(),
            },
            flags: QsoFlags { is_void, dupe_override: false },
        })
    }

    #[test]
    fn empty_sidecar_produces_contactinfo_for_every_record() {
        let sidecar = SidecarState::empty(1);
        let recs = vec![record(1, false), record(2, false), record(3, false)];
        let (pending, newly_sent, void_state) = compute_pending(&sidecar, &recs, 1);
        assert_eq!(pending.len(), 3);
        assert!(pending.iter().all(|p| matches!(p, PendingContact::Info(_))));
        assert_eq!(newly_sent.len(), 3);
        assert_eq!(void_state.len(), 3);
        assert_eq!(void_state.values().filter(|v| **v).count(), 0);
    }

    #[test]
    fn already_cfmed_record_produces_nothing_when_unchanged() {
        let mut sidecar = SidecarState::empty(1);
        sidecar.sent_ids.insert(1);
        sidecar.sent_void_state.insert(1, false);
        let recs = vec![record(1, false)];
        let (pending, newly_sent, _) = compute_pending(&sidecar, &recs, 1);
        assert!(pending.is_empty());
        assert!(newly_sent.is_empty());
    }

    #[test]
    fn void_toggle_on_cfmed_record_produces_contactreplace() {
        let mut sidecar = SidecarState::empty(1);
        sidecar.sent_ids.insert(1);
        sidecar.sent_void_state.insert(1, false);
        let recs = vec![record(1, true)]; // now voided
        let (pending, newly_sent, void_state) = compute_pending(&sidecar, &recs, 1);
        assert_eq!(pending.len(), 1);
        match &pending[0] {
            PendingContact::Replace(data) => {
                assert_eq!(data.x_qso, Some(true), "void → x-qso=1");
            }
            _ => panic!("expected Replace, got something else"),
        }
        // Newly-CFMed set stays empty — this QSO was already sent.
        assert!(newly_sent.is_empty());
        assert_eq!(void_state.get(&1), Some(&true));
    }

    #[test]
    fn unvoid_of_cfmed_record_produces_contactreplace_with_x_qso_zero() {
        let mut sidecar = SidecarState::empty(1);
        sidecar.sent_ids.insert(1);
        sidecar.sent_void_state.insert(1, true);
        let recs = vec![record(1, false)]; // unvoided (redo)
        let (pending, _, _) = compute_pending(&sidecar, &recs, 1);
        assert_eq!(pending.len(), 1);
        match &pending[0] {
            PendingContact::Replace(data) => {
                assert_eq!(data.x_qso, Some(false));
            }
            _ => panic!("expected Replace"),
        }
    }

    #[test]
    fn mix_of_new_and_cfmed_only_emits_new() {
        let mut sidecar = SidecarState::empty(1);
        sidecar.sent_ids.insert(1);
        sidecar.sent_void_state.insert(1, false);
        let recs = vec![record(1, false), record(2, false)];
        let (pending, newly_sent, _) = compute_pending(&sidecar, &recs, 1);
        assert_eq!(pending.len(), 1);
        assert!(matches!(&pending[0], PendingContact::Info(d) if d.call == "K2BB"));
        assert_eq!(newly_sent, HashSet::from([2]));
    }

    #[test]
    fn sidecar_roundtrips_through_json() {
        let mut s = SidecarState::empty(42);
        s.sent_ids.insert(1);
        s.sent_ids.insert(2);
        s.sent_void_state.insert(1, false);
        s.sent_void_state.insert(2, true);
        let bytes = serde_json::to_vec(&s).unwrap();
        let back: SidecarState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.contest_instance_id, 42);
        assert_eq!(back.sent_ids, s.sent_ids);
        assert_eq!(back.sent_void_state, s.sent_void_state);
    }

    #[test]
    fn sidecar_load_rejects_wrong_contest() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("test.db.rtc-state.json");
        let mut s = SidecarState::empty(1);
        s.sent_ids.insert(7);
        std::fs::write(&path, serde_json::to_vec(&s).unwrap()).unwrap();
        // Load with a different contest_instance_id → should start fresh.
        let loaded = SidecarState::load_or_empty(&path, 999);
        assert_eq!(loaded.contest_instance_id, 999);
        assert!(loaded.sent_ids.is_empty());
    }

    #[test]
    fn sidecar_load_missing_file_returns_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("nope.json");
        let loaded = SidecarState::load_or_empty(&path, 1);
        assert_eq!(loaded.contest_instance_id, 1);
        assert!(loaded.sent_ids.is_empty());
    }

    #[test]
    fn sidecar_load_corrupt_json_returns_empty() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("corrupt.json");
        std::fs::write(&path, b"not valid json at all").unwrap();
        let loaded = SidecarState::load_or_empty(&path, 1);
        assert!(loaded.sent_ids.is_empty());
    }

    #[test]
    fn sidecar_write_atomic_produces_readable_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("out.json");
        let mut s = SidecarState::empty(5);
        s.sent_ids.insert(1);
        s.write_atomic(&path).expect("atomic write");
        let reloaded = SidecarState::load_or_empty(&path, 5);
        assert_eq!(reloaded.sent_ids, s.sent_ids);
    }
}
