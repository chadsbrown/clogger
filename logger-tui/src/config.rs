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

    /// Call history file (.ch format, overrides call_history_file in config)
    #[arg(long)]
    pub call_history: Option<PathBuf>,

    /// SCP file (.scp format, overrides scp_file in config)
    #[arg(long)]
    pub scp: Option<PathBuf>,
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
    pub call_history_file: Option<PathBuf>,
    pub scp_file: Option<PathBuf>,
    #[serde(default)]
    pub cursor_style: CursorStyle,
    pub macros: Option<MacroOverrides>,
    pub rig: Option<logger_runtime::RigConfig>,
    pub keyer: Option<logger_runtime::KeyerConfig>,
    pub dxfeed: Option<logger_runtime::DxFeedConfig>,
}

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    #[default]
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

#[derive(Debug, Deserialize, Default)]
pub struct MacroOverrides {
    pub f1: Option<String>,
    pub f2: Option<String>,
    pub f3: Option<String>,
    pub f5: Option<String>,
    pub f7: Option<String>,
    pub f8: Option<String>,
    pub f9: Option<String>,
}

fn default_rst_sent() -> String {
    "599".to_string()
}

pub fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(&cli.config)?;
    let config: Config = toml::from_str(&text)?;
    Ok(config)
}
