use anyhow::Context;
use dxfeed::{
    domain::DxMode,
    feed::DxFeedBuilder,
    filter::config::FilterConfigSerde,
    model::{DxEvent, DxSpotEvent, SourceId, SpotEventKind, SpotKey},
    skimmer::config::SkimmerQualityConfig,
    source::{cluster::ClusterSourceConfig, supervisor::SourceConfig},
};
use logger_core::{AppEvent, Spot};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::config::DxFeedConfig;
use crate::cty::CtyDb;
use crate::dxfeed_enrichment::ScpEnrichment;
use crate::dxfeed_entity::CtyEntityResolver;
use logger_core::ScpLookup;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn spawn_dxfeed_adapter(
    config: &DxFeedConfig,
    tx: mpsc::Sender<AppEvent>,
    scp: Option<Arc<dyn ScpLookup>>,
    cty: Option<Arc<CtyDb>>,
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
        // Guard against the silent-drop trap: if the filter uses any
        // `geo.*` rule but no cty.dat is loaded, every spot's continent
        // will be None, and an allowlist + Neutral unknown_policy will
        // drop every spot. Refuse to start rather than run the contest
        // with an invisible filter failure.
        if filter_uses_geo(&filter) && cty.is_none() {
            anyhow::bail!(
                "dxfeed filter at {} uses `geo.*` rules but no cty_file is \
                 configured — geo filters require a cty.dat. Set `cty_file` \
                 in config.toml (or pass --cty) to a country-files.com cty.dat.",
                path.display()
            );
        }
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
    builder = builder.set_skimmer_quality(skimmer);

    // Enrichment: when an SCP database is available, wire it through so
    // filter.json's `enrichment.in_master_db` rule has real data to match
    // against. Calls not in SCP are almost always busted RBN copies.
    if let Some(scp) = scp {
        info!("dxfeed: installing SCP-backed enrichment resolver");
        builder = builder.enrichment_resolver(Box::new(ScpEnrichment::new(scp)));
    }

    // Entity resolution: when a cty.dat is loaded, wire the resolver so
    // `geo.*` filter rules (continent_allow, cq_zone_allow, etc.) see
    // real data. Without this, geo fields resolve to None and any
    // allowlist silently drops every spot (see filter_uses_geo guard above).
    if let Some(cty) = cty {
        info!("dxfeed: installing cty.dat-backed entity resolver");
        builder = builder.entity_resolver(Box::new(CtyEntityResolver::new(cty)));
    }

    let mut feed = builder
        .build()
        .map_err(|e| anyhow::anyhow!("dxfeed build: {e:?}"))?;

    tokio::spawn(async move {
        let mut tracker = SpotTracker::default();
        while let Some(event) = feed.next_event().await {
            match event {
                DxEvent::Spot(spot_event) => {
                    for app_event in tracker.translate(spot_event) {
                        let _ = tx.send(app_event).await;
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

/// Translates dxfeed spot events into bandmap events.
///
/// dxfeed identifies a spot by call, band, mode and frequency, and
/// guarantees that every `New` is followed by exactly one `Withdraw` for
/// the same key. The bandmap stores only call, frequency and mode, and the
/// frequency on a `Withdraw` is the latest one reported, which can differ
/// from the one sent with the `New`. So this remembers the bandmap spot
/// sent for each key and withdraws exactly that spot: a call spotted on
/// two bands keeps its other spot when one of them goes away.
#[derive(Default)]
struct SpotTracker {
    sent: HashMap<SpotKey, Spot>,
}

impl SpotTracker {
    fn translate(&mut self, event: DxSpotEvent) -> Vec<AppEvent> {
        let spot = Spot {
            call: event.spot.dx_call,
            freq_hz: event.spot.freq_hz,
            mode: dxmode_to_str(event.spot.mode),
        };
        let key = event.spot.spot_key;
        match event.kind {
            SpotEventKind::New | SpotEventKind::Update => {
                let previous = self.sent.insert(key, spot.clone());
                if previous.as_ref() == Some(&spot) {
                    // An update that changes nothing the bandmap shows.
                    return Vec::new();
                }
                let mut out = Vec::new();
                if let Some(previous) = previous {
                    out.push(withdrawn(previous));
                }
                out.push(AppEvent::SpotReceived { spot });
                out
            }
            SpotEventKind::Withdraw => {
                debug!(
                    call = %spot.call,
                    freq_hz = spot.freq_hz,
                    reason = ?event.withdraw_reason,
                    "dxfeed withdraw"
                );
                match self.sent.remove(&key) {
                    Some(previous) => vec![withdrawn(previous)],
                    // Never sent to the bandmap, so there is nothing to remove.
                    None => Vec::new(),
                }
            }
        }
    }
}

fn withdrawn(spot: Spot) -> AppEvent {
    AppEvent::SpotWithdrawn {
        call: spot.call,
        freq_hz: Some(spot.freq_hz),
    }
}

pub(crate) fn load_filter_file(path: &std::path::Path) -> anyhow::Result<FilterConfigSerde> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading dxfeed filter_file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parsing dxfeed filter_file {}", path.display()))
}

/// Does the filter configure any rule that depends on entity resolution?
/// If so, a cty.dat must be loaded or every spot's geo fields will be
/// None — and any allowlist will drop every spot under the default
/// `unknown_policy: Neutral`.
fn filter_uses_geo(filter: &FilterConfigSerde) -> bool {
    use dxfeed::filter::config::EntityFiltersSerde;
    fn entity_has_rules(e: &EntityFiltersSerde) -> bool {
        !e.continent_allow.is_empty()
            || !e.continent_deny.is_empty()
            || !e.cq_zone_allow.is_empty()
            || !e.cq_zone_deny.is_empty()
            || !e.itu_zone_allow.is_empty()
            || !e.itu_zone_deny.is_empty()
            || !e.entity_allow.is_empty()
            || !e.entity_deny.is_empty()
            || !e.country_allow.is_empty()
            || !e.country_deny.is_empty()
            || !e.state_allow.is_empty()
            || !e.state_deny.is_empty()
    }
    filter.geo.require_resolvers
        || entity_has_rules(&filter.geo.dx)
        || entity_has_rules(&filter.geo.spotter)
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
    use super::{filter_uses_geo, load_filter_file, SpotTracker};
    use chrono::Utc;
    use dxfeed::domain::{Band, Continent, DxMode};
    use dxfeed::filter::config::FilterConfigSerde;
    use dxfeed::model::{
        DxSpot, DxSpotEvent, SpotConfidence, SpotEventKind, SpotKey, WithdrawReason,
    };
    use logger_core::{AppEvent, Spot};

    /// A dxfeed event for `call` whose key is anchored at `anchor_hz` and
    /// whose latest reported frequency is `freq_hz`.
    fn spot_event(
        call: &str,
        band: Band,
        anchor_hz: u64,
        freq_hz: u64,
        kind: SpotEventKind,
    ) -> DxSpotEvent {
        let now = Utc::now();
        DxSpotEvent {
            spot: DxSpot {
                spot_key: SpotKey {
                    dx_call_norm: call.to_string(),
                    band,
                    mode: DxMode::CW,
                    freq_bucket_hz: anchor_hz,
                },
                dx_call: call.to_string(),
                freq_hz,
                band,
                mode: DxMode::CW,
                first_seen: now,
                last_seen: now,
                spot_time: None,
                comment: None,
                observations: Vec::new(),
                unique_originators: 1,
                unique_sources: 1,
                skim_quality: None,
                quality_detail: None,
                confidence: SpotConfidence::default(),
            },
            kind,
            revision: 1,
            withdraw_reason: (kind == SpotEventKind::Withdraw).then_some(WithdrawReason::Expired),
        }
    }

    fn received(call: &str, freq_hz: u64) -> AppEvent {
        AppEvent::SpotReceived {
            spot: Spot {
                call: call.to_string(),
                freq_hz,
                mode: "CW".to_string(),
            },
        }
    }

    fn withdrawn(call: &str, freq_hz: u64) -> AppEvent {
        AppEvent::SpotWithdrawn {
            call: call.to_string(),
            freq_hz: Some(freq_hz),
        }
    }

    #[test]
    fn withdraw_names_the_frequency_sent_with_new() {
        let mut tracker = SpotTracker::default();
        let new = spot_event("W1AW", Band::B20, 14_025_000, 14_025_000, SpotEventKind::New);
        assert_eq!(tracker.translate(new), vec![received("W1AW", 14_025_000)]);

        // The spot drifted 120 Hz inside dxfeed before it expired. The
        // bandmap still holds the frequency it was first given.
        let gone = spot_event("W1AW", Band::B20, 14_025_000, 14_025_120, SpotEventKind::Withdraw);
        assert_eq!(tracker.translate(gone), vec![withdrawn("W1AW", 14_025_000)]);
    }

    #[test]
    fn withdraw_on_one_band_leaves_the_other_band_alone() {
        let mut tracker = SpotTracker::default();
        tracker.translate(spot_event("W1AW", Band::B40, 7_025_000, 7_025_000, SpotEventKind::New));
        tracker.translate(spot_event("W1AW", Band::B20, 14_025_000, 14_025_000, SpotEventKind::New));

        let gone = spot_event("W1AW", Band::B40, 7_025_000, 7_025_000, SpotEventKind::Withdraw);
        assert_eq!(tracker.translate(gone), vec![withdrawn("W1AW", 7_025_000)]);

        // The 20m spot is still tracked and withdraws on its own later.
        let gone = spot_event("W1AW", Band::B20, 14_025_000, 14_025_000, SpotEventKind::Withdraw);
        assert_eq!(tracker.translate(gone), vec![withdrawn("W1AW", 14_025_000)]);
    }

    #[test]
    fn update_replaces_the_spot_only_when_it_changed() {
        let mut tracker = SpotTracker::default();
        tracker.translate(spot_event("W1AW", Band::B20, 14_025_000, 14_025_000, SpotEventKind::New));

        let same = spot_event("W1AW", Band::B20, 14_025_000, 14_025_000, SpotEventKind::Update);
        assert_eq!(tracker.translate(same), Vec::new());

        let moved = spot_event("W1AW", Band::B20, 14_025_000, 14_025_100, SpotEventKind::Update);
        assert_eq!(
            tracker.translate(moved),
            vec![withdrawn("W1AW", 14_025_000), received("W1AW", 14_025_100)]
        );
    }

    #[test]
    fn withdraw_for_a_spot_never_sent_is_ignored() {
        let mut tracker = SpotTracker::default();
        let gone = spot_event("W1AW", Band::B20, 14_025_000, 14_025_000, SpotEventKind::Withdraw);
        assert_eq!(tracker.translate(gone), Vec::new());
    }

    #[test]
    fn filter_without_geo_does_not_require_cty() {
        let cfg = FilterConfigSerde::default();
        assert!(!filter_uses_geo(&cfg));
    }

    #[test]
    fn filter_with_continent_allow_requires_cty() {
        let mut cfg = FilterConfigSerde::default();
        cfg.geo.dx.continent_allow.insert(Continent::NA);
        assert!(filter_uses_geo(&cfg));
    }

    #[test]
    fn filter_with_require_resolvers_requires_cty() {
        let mut cfg = FilterConfigSerde::default();
        cfg.geo.require_resolvers = true;
        assert!(filter_uses_geo(&cfg));
    }

    #[test]
    fn filter_with_spotter_geo_requires_cty() {
        let mut cfg = FilterConfigSerde::default();
        cfg.geo.spotter.cq_zone_allow.insert(5);
        assert!(filter_uses_geo(&cfg));
    }

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
