use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Category (Cabrillo class metadata)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CategoryConfig {
    pub power: CategoryPower,
    pub assisted: CategoryAssisted,
    pub transmitter: CategoryTransmitter,
    pub operator: CategoryOperator,
    pub bands: CategoryBands,
    pub mode: CategoryConfigMode,
    #[serde(default)]
    pub overlay: CategoryOverlay,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryPower {
    Low,
    High,
    Qrp,
}

impl CategoryPower {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::High => "HIGH",
            Self::Qrp => "QRP",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryAssisted {
    NonAssisted,
    Assisted,
}

impl CategoryAssisted {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NonAssisted => "NON-ASSISTED",
            Self::Assisted => "ASSISTED",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryTransmitter {
    Unlimited,
    Two,
    One,
}

impl CategoryTransmitter {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unlimited => "UNLIMITED",
            Self::Two => "TWO",
            Self::One => "ONE",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryOperator {
    MultiOp,
    SingleOp,
    MultiOne,
    MultiTwo,
    MultiMulti,
}

impl CategoryOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MultiOp => "MULTI-OP",
            Self::SingleOp => "SINGLE-OP",
            Self::MultiOne => "MULTI-ONE",
            Self::MultiTwo => "MULTI-TWO",
            Self::MultiMulti => "MULTI-MULTI",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryBands {
    All,
    #[serde(rename = "160M")]
    B160m,
    #[serde(rename = "80M")]
    B80m,
    #[serde(rename = "40M")]
    B40m,
    #[serde(rename = "20M")]
    B20m,
    #[serde(rename = "15M")]
    B15m,
    #[serde(rename = "10M")]
    B10m,
    #[serde(rename = "6M")]
    B6m,
    #[serde(rename = "2M")]
    B2m,
}

impl CategoryBands {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::B160m => "160M",
            Self::B80m => "80M",
            Self::B40m => "40M",
            Self::B20m => "20M",
            Self::B15m => "15M",
            Self::B10m => "10M",
            Self::B6m => "6M",
            Self::B2m => "2M",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryConfigMode {
    Mixed,
    Cw,
    #[serde(alias = "PH")]
    Ssb,
    Rtty,
    #[serde(alias = "RY")]
    Digi,
}

impl CategoryConfigMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mixed => "MIXED",
            Self::Cw => "CW",
            Self::Ssb => "SSB",
            Self::Rtty => "RTTY",
            Self::Digi => "DIGI",
        }
    }

    pub fn to_category_mode(&self) -> logger_core::CategoryMode {
        match self {
            Self::Cw => logger_core::CategoryMode::CW,
            Self::Ssb => logger_core::CategoryMode::SSB,
            _ => logger_core::CategoryMode::Mixed,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum CategoryOverlay {
    #[default]
    #[serde(rename = "N/A")]
    Na,
    TbWires,
    Rookie,
    Classic,
    WireOnly,
}

impl CategoryOverlay {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Na => "N/A",
            Self::TbWires => "TB-WIRES",
            Self::Rookie => "ROOKIE",
            Self::Classic => "CLASSIC",
            Self::WireOnly => "WIRE-ONLY",
        }
    }
}

// ---------------------------------------------------------------------------
// Cabrillo header metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CabrilloConfig {
    pub club: Option<String>,
    #[serde(default)]
    pub operators: Vec<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub address: Vec<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub soapbox: Vec<String>,
}

// ---------------------------------------------------------------------------
// Scoreboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ScoreboardEndpoint {
    pub url: String,
    pub password: String,
}

// ---------------------------------------------------------------------------
// Real Time Contest (RTC)
// ---------------------------------------------------------------------------

/// Configuration for the Real Time Contest (RTC) uploader. When present
/// and `enabled = true`, clogger posts every 2 minutes to the HamScore
/// RTC endpoint with `<dynamicresults>` + any new/changed QSOs.
///
/// All QTH fields are explicit — the operator may be portable or DXing
/// from outside their home zone, so we never infer location from the
/// callsign. When `enabled = true` the loader rejects empty values for
/// the required fields below.
#[derive(Debug, Clone, Deserialize)]
pub struct RtcConfig {
    /// Master switch. When false, the adapter is not spawned.
    pub enabled: bool,
    /// RTC server endpoint. Default: the HamScore direct-upload URL per
    /// the RTC 2.4-xml spec. The spec also lists a "recommended"
    /// scoredistributor.net URL, but that's HTTP and the spec elsewhere
    /// requires HTTPS, so we default to the direct HamScore URL.
    #[serde(default = "default_rtc_url")]
    pub url: String,
    /// Password assigned by the RTC service (HTTP Basic auth;
    /// username = station callsign).
    pub password: String,
    /// User-Agent header value. Defaults to `clogger/<cargo-pkg-version>`.
    #[serde(default)]
    pub user_agent: Option<String>,

    /// DXCC country prefix (e.g. "K", "VE", "EA6").
    pub dxcc_country: String,
    /// CQ zone number (1-40).
    pub cq_zone: u8,
    /// IARU zone number.
    pub iaru_zone: u8,
    /// ARRL section (e.g. "IL", "QC"). May be empty for DX ops.
    #[serde(default)]
    pub arrl_section: String,
    /// USA state or Canadian province abbreviation. May be empty for
    /// DX ops that have neither.
    #[serde(default)]
    pub state_or_province: String,
    /// Six-character Maidenhead grid locator (e.g. "EN50VE"). Required.
    pub grid6: String,
}

fn default_rtc_url() -> String {
    "https://hamscore.com/postxml/".to_string()
}

/// Bundled RTC spawn configuration used at bootstrap time. Composes the
/// user-supplied `RtcConfig` with contest-dependent metadata (the
/// per-contest RTC identifier, the contest instance id for QSO ID
/// hashing) and the log's sidecar path. UIs resolve these from the
/// selected contest before handing the bundle to `bootstrap()`.
#[derive(Clone)]
pub struct RtcSpawnConfig {
    pub http: RtcConfig,
    /// The RTC-server's contest identifier for the current contest and
    /// mode (e.g. "CW-OPS"). Resolved from
    /// `ContestEntry::cabrillo_id(mode)` by the UI — RTC and Cabrillo
    /// share the canonical name; the adapter is only spawned when this
    /// is `Some` on the UI side.
    pub contest_rtc_id: String,
    /// The station callsign, used as HTTP Basic auth username and as
    /// one input to the QSO ID hash.
    pub my_call: String,
    /// The contest_instance_id used in the qsolog store. Second input
    /// to the QSO ID hash; disambiguates IDs across contests reusing
    /// the same qso_id space.
    pub contest_instance_id: u64,
    /// Filesystem path for the RTC CFM-state sidecar. Typically
    /// `<db_path>.rtc-state.json`. UIs synthesize from `db_path`.
    pub sidecar_path: std::path::PathBuf,
    /// User-Agent header. Caller should default to
    /// `format!("clogger/{}", env!("CARGO_PKG_VERSION"))` when the
    /// TOML field is unset.
    pub user_agent: String,
    /// Cabrillo class metadata — needed for the `<class ... />`
    /// element inside `<dynamicresults>`. Shared with the scoreboard
    /// path (same element format). UIs require `[category]` to be
    /// configured whenever RTC is enabled.
    pub category: CategoryConfig,
}

impl RtcConfig {
    /// Validate that all required fields are non-empty when the adapter
    /// is enabled. Called by the UI config loaders; returns a
    /// user-readable error message listing every missing field.
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut missing = Vec::new();
        if self.url.trim().is_empty() {
            missing.push("url");
        }
        if self.password.trim().is_empty() {
            missing.push("password");
        }
        if self.dxcc_country.trim().is_empty() {
            missing.push("dxcc_country");
        }
        if self.grid6.trim().is_empty() {
            missing.push("grid6");
        }
        if self.cq_zone == 0 || self.cq_zone > 40 {
            missing.push("cq_zone (must be 1..=40)");
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "[rtc] missing or invalid required fields: {}",
                missing.join(", ")
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Hardware
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct RigConfig {
    /// Which radio this rig represents (1 or 2). Defaults to 1.
    #[serde(default = "default_radio_id")]
    pub radio_id: u8,
    pub model: String,
    pub port: String,
    pub baud_rate: Option<u32>,
    /// CW sending speed for this radio (used for < and > speed markers in macros)
    pub cw_speed: Option<u8>,
    /// Declares whether the operator has CI-V Transceive (Auto-Information)
    /// mode enabled on the radio. Must match the hardware menu setting.
    /// When `true`, clogger disables its own frequency/mode/passband polling
    /// and relies entirely on the rig's broadcast events (avoiding half-
    /// duplex bus collisions that can make front-panel buttons feel laggy).
    /// When `false`, clogger polls at 4 Hz as before. Default: `false`.
    #[serde(default)]
    pub transceive: bool,
}

fn default_radio_id() -> u8 {
    1
}

#[derive(Debug, Deserialize)]
pub struct KeyerConfig {
    pub port: String,
    #[serde(default = "default_speed")]
    pub speed_wpm: u8,
    #[serde(default)]
    pub contest_spacing: bool,
    /// Show CW characters in real-time as the WinKeyer echoes them back.
    #[serde(default)]
    pub cw_echo: bool,
    /// Enable the WinKeyer's audio sidetone output. When false, no audio
    /// is produced while sending CW (useful when the rig provides its own
    /// sidetone or for silent operation). Default: true.
    #[serde(default = "default_sidetone")]
    pub sidetone: bool,
}

fn default_speed() -> u8 {
    28
}

fn default_sidetone() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct So2rConfig {
    pub port: String,
    /// Default RX audio mode: "mono" (default), "stereo", or "reverse_stereo"
    #[serde(default = "default_rx_mode")]
    pub default_rx_mode: String,
}

fn default_rx_mode() -> String {
    "mono".to_string()
}

#[derive(Debug, Deserialize)]
pub struct DxFeedConfig {
    pub sources: Vec<DxFeedSourceConfig>,
    /// Optional path to a JSON file containing a dxfeed `FilterConfigSerde`.
    /// When set, the file is loaded at startup; missing/invalid → fail to start.
    pub filter_file: Option<PathBuf>,
    /// Optional skimmer quality engine overrides. Any field omitted from the
    /// `[dxfeed.skimmer_quality]` TOML section uses dxfeed's default.
    pub skimmer_quality: Option<SkimmerQualityConfigSerde>,
}

#[derive(Debug, Deserialize)]
pub struct DxFeedSourceConfig {
    pub host: String,
    pub port: u16,
    pub callsign: String,
}

/// TOML-friendly mirror of `dxfeed::skimmer::config::SkimmerQualityConfig`
/// with all fields optional. Decoupling lets us name fields for TOML
/// (`lookback_window_secs` reads better than a `Duration`) and accept
/// partial overrides without making the user copy every default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkimmerQualityConfigSerde {
    pub enabled: Option<bool>,
    pub compute_valid: Option<bool>,
    pub compute_busted: Option<bool>,
    pub compute_qsy: Option<bool>,
    pub gate_skimmer_output: Option<bool>,
    pub allow_valid: Option<bool>,
    pub allow_qsy: Option<bool>,
    pub allow_unknown: Option<bool>,
    pub allow_busted: Option<bool>,
    pub valid_required_distinct_skimmers: Option<u8>,
    pub valid_freq_window_hz: Option<i64>,
    pub lookback_window_secs: Option<u64>,
    pub busted_freq_window_hz: Option<i64>,
    pub similar_call_max_edit_distance: Option<u8>,
    pub qsy_freq_window_hz: Option<i64>,
    pub apply_only_to_skimmer: Option<bool>,
    pub max_tracked_observations: Option<usize>,
}

impl SkimmerQualityConfigSerde {
    /// Convert into dxfeed's `SkimmerQualityConfig`, filling missing fields
    /// from `SkimmerQualityConfig::default()`.
    pub fn to_dxfeed(&self) -> dxfeed::skimmer::config::SkimmerQualityConfig {
        let mut cfg = dxfeed::skimmer::config::SkimmerQualityConfig::default();
        if let Some(v) = self.enabled { cfg.enabled = v; }
        if let Some(v) = self.compute_valid { cfg.compute_valid = v; }
        if let Some(v) = self.compute_busted { cfg.compute_busted = v; }
        if let Some(v) = self.compute_qsy { cfg.compute_qsy = v; }
        if let Some(v) = self.gate_skimmer_output { cfg.gate_skimmer_output = v; }
        if let Some(v) = self.allow_valid { cfg.allow_valid = v; }
        if let Some(v) = self.allow_qsy { cfg.allow_qsy = v; }
        if let Some(v) = self.allow_unknown { cfg.allow_unknown = v; }
        if let Some(v) = self.allow_busted { cfg.allow_busted = v; }
        if let Some(v) = self.valid_required_distinct_skimmers {
            cfg.valid_required_distinct_skimmers = v;
        }
        if let Some(v) = self.valid_freq_window_hz { cfg.valid_freq_window_hz = v; }
        if let Some(v) = self.lookback_window_secs {
            cfg.lookback_window = Duration::from_secs(v);
        }
        if let Some(v) = self.busted_freq_window_hz { cfg.busted_freq_window_hz = v; }
        if let Some(v) = self.similar_call_max_edit_distance {
            cfg.similar_call_max_edit_distance = v;
        }
        if let Some(v) = self.qsy_freq_window_hz { cfg.qsy_freq_window_hz = v; }
        if let Some(v) = self.apply_only_to_skimmer { cfg.apply_only_to_skimmer = v; }
        if let Some(v) = self.max_tracked_observations { cfg.max_tracked_observations = v; }
        cfg
    }

    /// Sanity-check before passing to dxfeed: catch nonsensical values that
    /// dxfeed would silently accept but produce useless behavior.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(0) = self.valid_required_distinct_skimmers {
            anyhow::bail!(
                "[dxfeed.skimmer_quality] valid_required_distinct_skimmers must be >= 1"
            );
        }
        if let Some(0) = self.lookback_window_secs {
            anyhow::bail!("[dxfeed.skimmer_quality] lookback_window_secs must be > 0");
        }
        if let Some(v) = self.valid_freq_window_hz {
            if v <= 0 {
                anyhow::bail!("[dxfeed.skimmer_quality] valid_freq_window_hz must be > 0");
            }
        }
        if let Some(v) = self.busted_freq_window_hz {
            if v <= 0 {
                anyhow::bail!("[dxfeed.skimmer_quality] busted_freq_window_hz must be > 0");
            }
        }
        if let Some(v) = self.qsy_freq_window_hz {
            if v <= 0 {
                anyhow::bail!("[dxfeed.skimmer_quality] qsy_freq_window_hz must be > 0");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skimmer_serde_defaults_match_dxfeed_defaults() {
        let empty = SkimmerQualityConfigSerde::default();
        let converted = empty.to_dxfeed();
        let baseline = dxfeed::skimmer::config::SkimmerQualityConfig::default();
        assert_eq!(converted.enabled, baseline.enabled);
        assert_eq!(
            converted.valid_required_distinct_skimmers,
            baseline.valid_required_distinct_skimmers
        );
        assert_eq!(converted.valid_freq_window_hz, baseline.valid_freq_window_hz);
        assert_eq!(converted.lookback_window, baseline.lookback_window);
        assert_eq!(converted.allow_busted, baseline.allow_busted);
        assert_eq!(converted.apply_only_to_skimmer, baseline.apply_only_to_skimmer);
    }

    #[test]
    fn skimmer_serde_partial_override_keeps_other_defaults() {
        let toml = r#"
            valid_required_distinct_skimmers = 5
            lookback_window_secs = 90
        "#;
        let parsed: SkimmerQualityConfigSerde = toml::from_str(toml).expect("parse");
        let converted = parsed.to_dxfeed();
        let baseline = dxfeed::skimmer::config::SkimmerQualityConfig::default();
        assert_eq!(converted.valid_required_distinct_skimmers, 5);
        assert_eq!(converted.lookback_window, Duration::from_secs(90));
        // Untouched fields stay at the dxfeed default.
        assert_eq!(converted.valid_freq_window_hz, baseline.valid_freq_window_hz);
        assert_eq!(converted.busted_freq_window_hz, baseline.busted_freq_window_hz);
        assert_eq!(
            converted.similar_call_max_edit_distance,
            baseline.similar_call_max_edit_distance
        );
        assert_eq!(converted.allow_busted, baseline.allow_busted);
    }

    #[test]
    fn skimmer_serde_validates_zero_skimmer_requirement() {
        let toml = "valid_required_distinct_skimmers = 0";
        let parsed: SkimmerQualityConfigSerde = toml::from_str(toml).unwrap();
        let err = parsed.validate().unwrap_err();
        assert!(err.to_string().contains("valid_required_distinct_skimmers"));
    }

    #[test]
    fn skimmer_serde_validates_negative_freq_window() {
        let toml = "valid_freq_window_hz = -1";
        let parsed: SkimmerQualityConfigSerde = toml::from_str(toml).unwrap();
        let err = parsed.validate().unwrap_err();
        assert!(err.to_string().contains("valid_freq_window_hz"));
    }

    #[test]
    fn skimmer_serde_rejects_unknown_field() {
        // `deny_unknown_fields` should catch typos so users don't think a
        // setting is taking effect when it's silently ignored.
        let toml = "valid_requierd_distinct_skimmers = 5";
        let res: Result<SkimmerQualityConfigSerde, _> = toml::from_str(toml);
        assert!(res.is_err(), "typo'd field should fail to deserialize");
    }
}
