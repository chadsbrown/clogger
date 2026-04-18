//! Fetches N0NBH solar/propagation data from hamqsl.com and pushes
//! periodic snapshots to the TUI via `AppEvent::CondXUpdate`.
//!
//! Opt-in: `[condx] enabled = true` in config.toml. When disabled, no
//! HTTP request is ever issued. Failures (network, parse) are logged
//! and retried on the next interval — the operator sees a stale
//! timestamp rather than a crash.

use std::time::Duration;

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, warn};

const HAMQSL_URL: &str = "https://www.hamqsl.com/solarxml.php";
const DEFAULT_REFRESH_SECS: u64 = 3600; // 60 min; hamqsl updates ~hourly

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CondXConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Override the default 60-minute poll cadence. Minimum 60s to avoid
    /// hammering hamqsl.com — values below are clamped up.
    pub refresh_interval_secs: Option<u64>,
}

/// Parsed snapshot of the N0NBH feed. Fields use the original XML names
/// lowercased; string fields hold the raw feed value (e.g. `"UNSETTLD"`,
/// `"B3.2"`, `"S2-S3"`) for display without reinterpretation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CondXSnapshot {
    pub updated: String,
    pub sfi: Option<u32>,
    pub sunspots: Option<u32>,
    pub a_index: Option<u32>,
    pub k_index: Option<u32>,
    pub xray: String,
    pub geomag_field: String,
    pub signal_noise: String,
    pub conditions: Vec<BandCondition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandCondition {
    /// e.g. `"80m-40m"`
    pub band: String,
    /// `"day"` or `"night"`
    pub time: String,
    /// `"Poor"`, `"Fair"`, `"Good"`, or whatever the feed reports.
    pub rating: String,
}

/// Spawn the CondX polling task. Returns `None` when the feature is
/// disabled (so the event loop can skip the `select!` arm entirely).
pub fn spawn_condx_adapter(config: CondXConfig) -> Option<mpsc::Receiver<CondXSnapshot>> {
    if !config.enabled {
        return None;
    }
    let (tx, rx) = mpsc::channel::<CondXSnapshot>(4);
    let interval_secs = config
        .refresh_interval_secs
        .unwrap_or(DEFAULT_REFRESH_SECS)
        .max(60);

    tokio::spawn(async move {
        // Construct the client once; reqwest reuses the underlying
        // connection pool across poll cycles.
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!("condx: failed to build HTTP client, disabling: {e}");
                return;
            }
        };

        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
        // The first tick fires immediately so the panel doesn't show
        // "Loading…" for an hour after startup.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            match fetch_and_parse(&client).await {
                Ok(snapshot) => {
                    debug!("condx: fetched snapshot, sfi={:?}", snapshot.sfi);
                    let _ = tx.send(snapshot).await;
                }
                Err(e) => {
                    warn!("condx: fetch failed, will retry in {interval_secs}s: {e:#}");
                }
            }
        }
    });
    Some(rx)
}

async fn fetch_and_parse(client: &reqwest::Client) -> Result<CondXSnapshot> {
    let body = client
        .get(HAMQSL_URL)
        .send()
        .await
        .with_context(|| format!("GET {HAMQSL_URL}"))?
        .error_for_status()?
        .text()
        .await
        .context("reading hamqsl response body")?;
    parse_solar_xml(&body)
}

/// Parse the hamqsl `<solar><solardata>...</solardata></solar>` feed.
/// Unknown elements are skipped — keeps us forward-compatible if
/// hamqsl adds new fields. Malformed XML bubbles an error out.
fn parse_solar_xml(xml: &str) -> Result<CondXSnapshot> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut snapshot = CondXSnapshot::default();
    let mut buf = Vec::new();
    // Track the currently-open element + any attributes we care about.
    // The hamqsl schema is flat within <solardata> except for the
    // <calculatedconditions><band .../> list.
    let mut current_tag: Option<String> = None;
    let mut band_name: Option<String> = None;
    let mut band_time: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = std::str::from_utf8(e.name().as_ref())?.to_string();
                if name == "band" {
                    // Capture attributes before descending into text.
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref())?;
                        let val = attr.unescape_value()?.into_owned();
                        match key {
                            "name" => band_name = Some(val),
                            "time" => band_time = Some(val),
                            _ => {}
                        }
                    }
                }
                current_tag = Some(name);
            }
            Ok(Event::End(e)) => {
                let raw = e.name();
                let name = std::str::from_utf8(raw.as_ref())?;
                if name == "band" {
                    band_name = None;
                    band_time = None;
                }
                current_tag = None;
            }
            Ok(Event::Text(t)) => {
                let Some(tag) = &current_tag else { continue };
                let text = t.unescape()?.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                apply_field(&mut snapshot, tag, &text, band_name.as_deref(), band_time.as_deref());
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => anyhow::bail!("XML parse at pos {}: {e}", reader.buffer_position()),
        }
        buf.clear();
    }

    Ok(snapshot)
}

fn apply_field(
    s: &mut CondXSnapshot,
    tag: &str,
    text: &str,
    band_name: Option<&str>,
    band_time: Option<&str>,
) {
    match tag {
        "updated" => s.updated = text.to_string(),
        "solarflux" => s.sfi = text.parse().ok(),
        "sunspots" => s.sunspots = text.parse().ok(),
        "aindex" => s.a_index = text.parse().ok(),
        "kindex" => s.k_index = text.parse().ok(),
        "xray" => s.xray = text.to_string(),
        "geomagfield" => s.geomag_field = text.to_string(),
        "signalnoise" => s.signal_noise = text.to_string(),
        "band" => {
            if let (Some(name), Some(time)) = (band_name, band_time) {
                s.conditions.push(BandCondition {
                    band: name.to_string(),
                    time: time.to_string(),
                    rating: text.to_string(),
                });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<solar>
  <solardata>
    <source url="http://www.hamqsl.com/solar.html">N0NBH</source>
    <updated>18 Apr 2026 2057 GMT</updated>
    <solarflux>107</solarflux>
    <aindex>4</aindex>
    <kindex>3</kindex>
    <xray>B3.2</xray>
    <sunspots>44</sunspots>
    <signalnoise>S2-S3</signalnoise>
    <geomagfield>UNSETTLD</geomagfield>
    <calculatedconditions>
      <band name="80m-40m" time="day">Poor</band>
      <band name="30m-20m" time="day">Good</band>
      <band name="17m-15m" time="day">Fair</band>
      <band name="12m-10m" time="day">Poor</band>
      <band name="80m-40m" time="night">Fair</band>
      <band name="30m-20m" time="night">Good</band>
      <band name="17m-15m" time="night">Fair</band>
      <band name="12m-10m" time="night">Poor</band>
    </calculatedconditions>
  </solardata>
</solar>
"#;

    #[test]
    fn parses_scalar_fields() {
        let s = parse_solar_xml(SAMPLE).unwrap();
        assert_eq!(s.updated, "18 Apr 2026 2057 GMT");
        assert_eq!(s.sfi, Some(107));
        assert_eq!(s.sunspots, Some(44));
        assert_eq!(s.a_index, Some(4));
        assert_eq!(s.k_index, Some(3));
        assert_eq!(s.xray, "B3.2");
        assert_eq!(s.geomag_field, "UNSETTLD");
        assert_eq!(s.signal_noise, "S2-S3");
    }

    #[test]
    fn parses_all_band_conditions() {
        let s = parse_solar_xml(SAMPLE).unwrap();
        assert_eq!(s.conditions.len(), 8);
        let get = |band: &str, time: &str| -> String {
            s.conditions
                .iter()
                .find(|c| c.band == band && c.time == time)
                .map(|c| c.rating.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("80m-40m", "day"), "Poor");
        assert_eq!(get("80m-40m", "night"), "Fair");
        assert_eq!(get("30m-20m", "day"), "Good");
        assert_eq!(get("12m-10m", "night"), "Poor");
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let xml = r#"<solar><solardata>
            <solarflux>99</solarflux>
            <totallynewfield>xyz</totallynewfield>
            <kindex>5</kindex>
        </solardata></solar>"#;
        let s = parse_solar_xml(xml).unwrap();
        assert_eq!(s.sfi, Some(99));
        assert_eq!(s.k_index, Some(5));
    }

    #[test]
    fn missing_numeric_field_leaves_none() {
        let xml = r#"<solar><solardata>
            <solarflux>nope</solarflux>
        </solardata></solar>"#;
        let s = parse_solar_xml(xml).unwrap();
        assert_eq!(s.sfi, None);
    }
}
