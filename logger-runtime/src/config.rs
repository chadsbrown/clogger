use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RigConfig {
    pub model: String,
    pub port: String,
    pub baud_rate: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct KeyerConfig {
    pub port: String,
    #[serde(default = "default_speed")]
    pub speed_wpm: u8,
    #[serde(default)]
    pub contest_spacing: bool,
}

fn default_speed() -> u8 {
    28
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
