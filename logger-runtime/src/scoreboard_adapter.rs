use std::fmt::Write as FmtWrite;
use std::time::Duration;

use futures::future::join_all;
use tokio::sync::watch;
use tracing::warn;

use crate::config::{CategoryConfig, ScoreboardEndpoint};
use crate::scoring::ScoreBreakdown;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

pub struct ScoreboardConfig {
    pub endpoints: Vec<ScoreboardEndpoint>,
    pub interval_secs: u64,
}

/// Snapshot of current score state, sent from the event loop to the adapter
/// via a watch channel. The adapter serializes this to XML on each post tick.
pub struct ScoreboardSnapshot {
    pub cabrillo_id: &'static str,
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

pub struct ScoreboardHandle {
    pub snapshot_tx: watch::Sender<Option<ScoreboardSnapshot>>,
    pub status_rx: watch::Receiver<ScoreboardStatus>,
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

pub fn spawn_scoreboard_adapter(cfg: ScoreboardConfig) -> ScoreboardHandle {
    let (snapshot_tx, snapshot_rx) = watch::channel::<Option<ScoreboardSnapshot>>(None);
    let (status_tx, status_rx) = watch::channel(ScoreboardStatus::Idle);

    tokio::spawn(adapter_task(cfg, snapshot_rx, status_tx));

    ScoreboardHandle {
        snapshot_tx,
        status_rx,
    }
}

// ---------------------------------------------------------------------------
// Internal task
// ---------------------------------------------------------------------------

async fn adapter_task(
    cfg: ScoreboardConfig,
    snapshot_rx: watch::Receiver<Option<ScoreboardSnapshot>>,
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

        // Borrow the current snapshot
        let xml = {
            let snap_ref = snapshot_rx.borrow();
            match snap_ref.as_ref() {
                Some(snap) => serialize_xml(snap),
                None => continue,
            }
        };

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
            if resp.status().is_success() {
                true
            } else {
                warn!(
                    "scoreboard POST to {} failed: HTTP {}",
                    endpoint.url,
                    resp.status()
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

fn serialize_xml(snap: &ScoreboardSnapshot) -> String {
    let mut xml = String::with_capacity(2048);

    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");

    let _ = write!(xml, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = write!(xml, "<dynamicresults>\n");
    let _ = write!(xml, "  <contest>{}</contest>\n", xml_escape(snap.cabrillo_id));
    let _ = write!(xml, "  <call>{}</call>\n", xml_escape(&snap.call));
    let _ = write!(xml, "  <ops>{}</ops>\n", xml_escape(&snap.ops));

    let cat = &snap.category;
    let _ = write!(xml, "  <class power=\"{}\"", cat.power.as_str());
    let _ = write!(xml, " assisted=\"{}\"", cat.assisted.as_str());
    let _ = write!(xml, " transmitter=\"{}\"", cat.transmitter.as_str());
    let _ = write!(xml, " ops=\"{}\"", cat.operator.as_str());
    let _ = write!(xml, " bands=\"{}\"", cat.bands.as_str());
    let _ = write!(xml, " mode=\"{}\"", cat.mode.as_str());
    let _ = write!(xml, " overlay=\"{}\"", cat.overlay.as_str());
    let _ = write!(xml, " />\n");

    // Breakdown
    let _ = write!(xml, "  <breakdown>\n");
    for row in &snap.breakdown.rows {
        let _ = write!(
            xml,
            "    <qso band=\"{}\" mode=\"{}\">{}</qso>\n",
            row.band, row.mode, row.qsos
        );
        for (mult_type, count) in &row.mults {
            let _ = write!(
                xml,
                "    <mult type=\"{}\" band=\"{}\" mode=\"{}\">{}</mult>\n",
                mult_type, row.band, row.mode, count
            );
        }
        let _ = write!(
            xml,
            "    <point band=\"{}\" mode=\"{}\">{}</point>\n",
            row.band, row.mode, row.points
        );
    }
    let _ = write!(xml, "  </breakdown>\n");

    let _ = write!(xml, "  <score>{}</score>\n", snap.breakdown.claimed_score);
    let _ = write!(xml, "  <timestamp>{}</timestamp>\n", timestamp);
    let _ = write!(xml, "</dynamicresults>\n");

    xml
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
