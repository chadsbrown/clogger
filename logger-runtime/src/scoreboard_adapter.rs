use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use tokio::sync::watch;
use tracing::warn;

use crate::config::{CategoryConfig, ScoreboardEndpoint};
use crate::log_adapter::LogSnapshot;
use crate::scoring::ScoreBreakdown;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Bundled scoreboard configuration used at adapter spawn time.
/// Carries both the user-configured endpoints/interval and the
/// contest/station identity needed to build the posted XML. The
/// identity fields don't come from the scoreboard TOML section — they're
/// composed by the UI (or bootstrap) from the contest, my_call, and
/// category before spawning.
#[derive(Clone)]
pub struct ScoreboardConfig {
    pub endpoints: Vec<ScoreboardEndpoint>,
    pub interval_secs: u64,
    /// Cabrillo contest identifier (e.g. "CQ-WW-CW"). Picked by the
    /// UI from `ContestEntry::cabrillo_id(mode)` at startup and
    /// cloned per tick into the `ScoreboardSnapshot`.
    pub cabrillo_id: String,
    /// Station callsign. Used as both `<call>` and `<ops>` in the
    /// scoreboard XML.
    pub call: String,
    /// Operator list (space-separated). Usually identical to `call`
    /// for single-op; UI-configurable for multi-op categories.
    pub ops: String,
    /// Cabrillo class metadata (power / assisted / mode / etc.). Not
    /// derived — required when scoreboard posting is enabled.
    pub category: CategoryConfig,
}

/// Snapshot that the adapter *internally* projects from the shared
/// `LogSnapshot` at post time. Kept public because the XML
/// serialization helper takes one, and exposing it keeps the helper
/// testable without a LogSnapshot fixture.
pub struct ScoreboardSnapshot {
    pub cabrillo_id: String,
    pub call: String,
    pub ops: String,
    pub category: CategoryConfig,
    pub breakdown: ScoreBreakdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreboardStatus {
    Idle,
    Ok,
    Failing,
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the scoreboard adapter. The adapter subscribes to
/// `log_rx` (published by `LogAdapter` on every mutation), reads the
/// latest `LogSnapshot` on each posting tick, projects it into the
/// scoreboard XML shape, and POSTs to every configured endpoint.
/// Returns a `watch::Receiver<ScoreboardStatus>` so UIs can render a
/// status indicator if they want.
pub fn spawn_scoreboard_adapter(
    cfg: ScoreboardConfig,
    log_rx: watch::Receiver<Arc<LogSnapshot>>,
) -> watch::Receiver<ScoreboardStatus> {
    let (status_tx, status_rx) = watch::channel(ScoreboardStatus::Idle);
    tokio::spawn(adapter_task(cfg, log_rx, status_tx));
    status_rx
}

// ---------------------------------------------------------------------------
// Internal task
// ---------------------------------------------------------------------------

async fn adapter_task(
    cfg: ScoreboardConfig,
    log_rx: watch::Receiver<Arc<LogSnapshot>>,
    status_tx: watch::Sender<ScoreboardStatus>,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client");

    let mut interval = tokio::time::interval(Duration::from_secs(cfg.interval_secs));

    loop {
        // Wait for the next posting tick. We don't post on snapshot change —
        // only on the interval. The watch channel always has the latest value.
        interval.tick().await;

        // Project the latest LogSnapshot into a ScoreboardSnapshot.
        // Cloning the breakdown is the one unavoidable cost per tick;
        // everything else (cabrillo_id/call/ops/category) is identity
        // material shared across ticks.
        let snap = {
            let log = log_rx.borrow();
            ScoreboardSnapshot {
                cabrillo_id: cfg.cabrillo_id.clone(),
                call: cfg.call.clone(),
                ops: cfg.ops.clone(),
                category: cfg.category.clone(),
                breakdown: (*log.score_breakdown).clone(),
            }
        };
        let xml = serialize_xml(&snap);

        // TEMP-DEBUG (remove-me): body dump to scoreboard.log; see
        // logger-runtime/src/temp_debug_log.rs for the full removal
        // recipe. Replaces the old `tracing::info!("scoreboard XML:
        // {xml}")` that used to pollute the regular log.
        crate::temp_debug_log::append("scoreboard.log", &xml);

        // POST to all endpoints in parallel
        let results = join_all(cfg.endpoints.iter().map(|ep| {
            post_to_endpoint(&client, ep, &xml)
        }))
        .await;

        let all_ok = results.iter().all(|r| *r);
        let new_status = if all_ok {
            ScoreboardStatus::Ok
        } else {
            ScoreboardStatus::Failing
        };
        let _ = status_tx.send(new_status);
    }
}

async fn post_to_endpoint(
    client: &reqwest::Client,
    endpoint: &ScoreboardEndpoint,
    xml: &str,
) -> bool {
    match client
        .post(&endpoint.url)
        .basic_auth("", Some(&endpoint.password))
        .header("Content-Type", "application/xml")
        .body(xml.to_string())
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.is_success() {
                tracing::info!("scoreboard POST to {} ok: {body}", endpoint.url);
                true
            } else {
                warn!(
                    "scoreboard POST to {} failed: HTTP {status} body: {body}",
                    endpoint.url,
                );
                false
            }
        }
        Err(e) => {
            warn!("scoreboard POST to {} failed: {e}", endpoint.url);
            false
        }
    }
}

// ---------------------------------------------------------------------------
// XML serialization
// ---------------------------------------------------------------------------

/// Build the full scoreboard XML post. Wraps the shared
/// dynamicresults body in the outer `<dynamicresults>` element with
/// the XML declaration the scoreboard path has always used. RTC
/// reuses the same body under a different outer envelope — see
/// `rtc_xml::serialize_envelope`.
fn serialize_xml(snap: &ScoreboardSnapshot) -> String {
    let body = crate::rtc_xml::serialize_dynamicresults_body(snap, None);
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<dynamicresults>\n{body}</dynamicresults>\n")
}
