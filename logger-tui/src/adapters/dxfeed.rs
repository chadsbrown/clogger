use dxfeed::{
    feed::DxFeedBuilder,
    model::{DxEvent, SourceId, SpotEventKind},
    source::{cluster::ClusterSourceConfig, supervisor::SourceConfig},
};
use logger_core::{AppEvent, Spot};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::adapters::terminal::TerminalEvent;
use crate::config::DxFeedConfig;

pub async fn spawn_dxfeed_adapter(
    config: &DxFeedConfig,
    tx: mpsc::Sender<TerminalEvent>,
) -> anyhow::Result<()> {
    let mut builder = DxFeedBuilder::new();

    for (i, src) in config.sources.iter().enumerate() {
        info!("adding dxfeed source: {}:{} as {}", src.host, src.port, src.callsign);
        let cluster = ClusterSourceConfig::new(
            &src.host,
            src.port,
            &src.callsign,
            SourceId(format!("cluster-{i}")),
        );
        builder = builder.add_source(SourceConfig::Cluster(cluster));
    }

    let mut feed = builder.build().map_err(|e| anyhow::anyhow!("dxfeed build: {e:?}"))?;

    tokio::spawn(async move {
        while let Some(event) = feed.next_event().await {
            match event {
                DxEvent::Spot(spot_event) => {
                    if matches!(spot_event.kind, SpotEventKind::New | SpotEventKind::Update) {
                        let _ = tx
                            .send(TerminalEvent::App(AppEvent::SpotReceived {
                                spot: Spot {
                                    call: spot_event.spot.dx_call,
                                    freq_hz: spot_event.spot.freq_hz,
                                },
                            }))
                            .await;
                    }
                }
                DxEvent::SourceStatus(status) => {
                    info!("dxfeed source {}: {:?}", status.source_id.0, status.state);
                }
                DxEvent::Error(err) => {
                    warn!("dxfeed error: {}", err.message);
                }
                _ => {}
            }
        }
    });

    Ok(())
}
