//! Real Time Contest (RTC) uploader adapter.
//!
//! Subscribes to `LogSnapshot` updates and posts `<dynamicresults>` +
//! per-QSO containers to the HamScore RTC endpoint on a mandatory
//! 2-minute cadence (fixed by the spec; not configurable). Handles
//! CFM tracking across restarts via a JSON sidecar.
//!
//! Spec: `~/Downloads/RTC_Specification_2.4_XML.pdf` (rev 2.4-xml, Jan 2025).

use std::sync::Arc;

use tokio::sync::watch;

use crate::config::RtcSpawnConfig;
use crate::log_adapter::LogSnapshot;

/// Status reported by the adapter back to UIs.
///
/// `Idle`   - adapter spawned, no tick has fired yet.
/// `Ok`     - last post succeeded with CFM (or OK if no changes).
/// `Failing` - last post failed (network error, non-success HTTP, or
///            server returned `{"Status":"Error",...}`).
/// `Unsupported` - server returned "Contest not supported"; adapter
///            keeps retrying at its normal cadence in case the server
///            is reconfigured mid-contest.
/// `NoChanges` - last tick had nothing to post beyond `<dynamicresults>`
///            and the server CFMed with OK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcStatus {
    Idle,
    Ok,
    Failing,
    Unsupported,
    NoChanges,
}

/// Spawn the RTC adapter. Returns a status receiver; the task runs
/// until every receiver is dropped.
///
/// Body is a stub — Phase 3c fills in the task loop (delta tracking,
/// XML serialization, POST + response parsing, sidecar persistence).
pub fn spawn_rtc_adapter(
    _cfg: RtcSpawnConfig,
    _log_rx: watch::Receiver<Arc<LogSnapshot>>,
) -> watch::Receiver<RtcStatus> {
    let (_status_tx, status_rx) = watch::channel(RtcStatus::Idle);
    // TODO(phase-3c): spawn adapter_task(cfg, log_rx, status_tx)
    status_rx
}
