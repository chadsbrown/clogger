use serde::Deserialize;

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
}

fn default_speed() -> u8 {
    28
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
