use anyhow::Context;
use dxfeed::{
    domain::DxMode,
    feed::DxFeedBuilder,
    filter::config::FilterConfigSerde,
    model::{DxEvent, SourceId, SpotEventKind},
    skimmer::config::SkimmerQualityConfig,
    source::{cluster::ClusterSourceConfig, supervisor::SourceConfig},
};
use logger_core::{AppEvent, Spot};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::DxFeedConfig;

pub async fn spawn_dxfeed_adapter(
    config: &DxFeedConfig,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<()> {
    let mut builder = DxFeedBuilder::new();

    for (i, src) in config.sources.iter().enumerate() {
        info!(
            "adding dxfeed source: {}:{} as {}",
            src.host, src.port, src.callsign
        );
        let cluster = ClusterSourceConfig::new(
            &src.host,
            src.port,
            &src.callsign,
            SourceId(format!("cluster-{i}")),
        );
        builder = builder.add_source(SourceConfig::Cluster(cluster));
    }

    // Optional filter pipeline (band/callsign/spotter rules) loaded from JSON.
    if let Some(path) = &config.filter_file {
        let filter = load_filter_file(path)?;
        info!("dxfeed: loaded filter from {}", path.display());
        builder = builder.set_filter(filter);
    }

    // Skimmer quality engine: user overrides (validated) or dxfeed defaults.
    let skimmer = match &config.skimmer_quality {
        Some(s) => {
            s.validate()?;
            s.to_dxfeed()
        }
        None => SkimmerQualityConfig::default(),
    };

    let mut feed = builder
        .set_skimmer_quality(skimmer)
        .build()
        .map_err(|e| anyhow::anyhow!("dxfeed build: {e:?}"))?;

    tokio::spawn(async move {
        while let Some(event) = feed.next_event().await {
            match event {
                DxEvent::Spot(spot_event) => match spot_event.kind {
                    SpotEventKind::New | SpotEventKind::Update => {
                        let mode = dxmode_to_str(spot_event.spot.mode);
                        let _ = tx
                            .send(AppEvent::SpotReceived {
                                spot: Spot {
                                    call: spot_event.spot.dx_call,
                                    freq_hz: spot_event.spot.freq_hz,
                                    mode,
                                },
                            })
                            .await;
                    }
                    SpotEventKind::Withdraw => {
                        let _ = tx
                            .send(AppEvent::SpotWithdrawn {
                                call: spot_event.spot.dx_call,
                            })
                            .await;
                    }
                },
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

pub(crate) fn load_filter_file(path: &std::path::Path) -> anyhow::Result<FilterConfigSerde> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading dxfeed filter_file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing dxfeed filter_file {}", path.display()))
}

fn dxmode_to_str(mode: DxMode) -> String {
    match mode {
        DxMode::CW => "CW",
        DxMode::SSB => "SSB",
        DxMode::DIG => "DIGITAL",
        DxMode::AM => "AM",
        DxMode::FM => "FM",
        DxMode::Unknown => "CW",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::load_filter_file;

    #[test]
    fn filter_file_missing_returns_error() {
        let err = load_filter_file(std::path::Path::new("/nonexistent/dxfeed-filter.json"))
            .unwrap_err();
        assert!(err.to_string().contains("reading dxfeed filter_file"));
    }

    #[test]
    fn filter_file_invalid_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let err = load_filter_file(&path).unwrap_err();
        assert!(err.to_string().contains("parsing dxfeed filter_file"));
    }

    #[test]
    fn filter_file_minimal_valid_json_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filter.json");
        // Smallest valid FilterConfigSerde — just the required defaults.
        let cfg = dxfeed::filter::config::FilterConfigSerde::default();
        std::fs::write(&path, serde_json::to_string(&cfg).unwrap()).unwrap();
        let loaded = load_filter_file(&path).expect("should load");
        assert_eq!(loaded.max_age_secs, cfg.max_age_secs);
    }
}
