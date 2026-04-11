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
// Hardware
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize)]
pub struct DxFeedSourceConfig {
    pub host: String,
    pub port: u16,
    pub callsign: String,
}
