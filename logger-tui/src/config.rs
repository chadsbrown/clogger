use std::path::PathBuf;

use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "logger-tui", about = "Contest logger terminal UI")]
pub struct Cli {
    /// Path to TOML config file
    #[arg(short, long)]
    pub config: PathBuf,

    /// SQLite database file (overrides db_path in config)
    #[arg(short, long)]
    pub db: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub my_call: String,
    #[serde(default)]
    pub my_zone: u8,
    pub contest: String,
    #[serde(default = "default_rst_sent")]
    pub rst_sent: String,
    pub my_name: Option<String>,
    pub my_xchg: Option<String>,
    pub db_path: Option<PathBuf>,
    pub rig: Option<RigConfig>,
    pub keyer: Option<KeyerConfig>,
    pub dxfeed: Option<DxFeedConfig>,
}

fn default_rst_sent() -> String {
    "599".to_string()
}

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

pub fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(&cli.config)?;
    let config: Config = toml::from_str(&text)?;
    Ok(config)
}
