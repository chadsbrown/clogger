use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "logger-tui", about = "Contest logger terminal UI")]
pub struct Cli {
    /// Path to stable TOML config file (station identity, hardware, scoreboard).
    /// This file usually doesn't change between contests.
    #[arg(short, long)]
    pub config: PathBuf,

    /// Path to the per-contest TOML file (contest id, macros, category,
    /// call history, state-QP [station] passthrough). Switch this file when
    /// you switch contests.
    #[arg(long)]
    pub contest: PathBuf,

    /// SQLite database file (overrides db_path in contest file)
    #[arg(short, long)]
    pub db: Option<PathBuf>,

    /// Call history file (.ch format, overrides call_history_file in contest file)
    #[arg(long)]
    pub call_history: Option<PathBuf>,

    /// SCP file (.scp format, overrides scp_file in config file)
    #[arg(long)]
    pub scp: Option<PathBuf>,

    /// Enable debug-level logging to clogger.log (default: info)
    #[arg(long)]
    pub debug: bool,
}

/// Stable station/hardware configuration. Lives in `config.toml` and rarely
/// changes between contests.
#[derive(Debug, Deserialize)]
pub struct StableConfig {
    pub my_call: String,
    #[serde(default)]
    pub my_zone: u8,
    #[serde(default = "default_rst_sent")]
    pub rst_sent: String,
    pub my_name: Option<String>,
    pub scp_file: Option<PathBuf>,
    #[serde(default)]
    pub cursor_style: CursorStyle,
    /// Multiple rigs supported via `[[rig]]` array of tables in TOML.
    /// For backward compatibility, a single `[rig]` table is also accepted.
    #[serde(default, rename = "rig")]
    pub rigs: Vec<logger_runtime::RigConfig>,
    pub keyer: Option<logger_runtime::KeyerConfig>,
    pub dxfeed: Option<logger_runtime::DxFeedConfig>,
    pub so2r: Option<logger_runtime::So2rConfig>,
    #[serde(default)]
    pub cabrillo: logger_runtime::CabrilloConfig,
    #[serde(default)]
    pub scoreboard: ScoreboardSection,
    #[serde(default)]
    pub bandmap: BandmapMode,
    /// Show the passband QRM warning. When true, a `QRM` badge appears while
    /// in Run mode if any bandmap spot (same mode) lies within the focused
    /// radio's current receive passband. The width is taken from the rig's
    /// reported filter width — set this flag and let the radio tell us.
    #[serde(default)]
    pub show_passband_qrm: bool,
    /// Enable Enter Sends Message (ESM). Default: true. Set to false if you
    /// prefer to send CW only via F-key macros and have Enter just log.
    #[serde(default = "default_esm_enabled")]
    pub esm_enabled: bool,
    /// Refuse to send or log via Enter when the call in the entry box is a
    /// known dupe for the current band/mode. Default: false. F-key macros
    /// remain unaffected and can be used to confirm the dupe over the air.
    #[serde(default)]
    pub block_dupes: bool,
}

/// Per-contest configuration. Lives in `contest.toml` and changes when you
/// switch contests. Holds the contest id, sent-exchange value, database path,
/// call-history file, typed `[station]` passthrough for contest-engine, CW
/// macro overrides, and Cabrillo category.
#[derive(Debug, Deserialize)]
pub struct ContestConfig {
    pub contest: String,
    pub my_xchg: Option<String>,
    pub db_path: Option<PathBuf>,
    pub call_history_file: Option<PathBuf>,
    /// Typed station-config passthrough for contest-engine. Keys are fed
    /// verbatim to the scorer (e.g. `my_is_fl = true`, `my_county = "ALC"`,
    /// `my_power_class = "LOW"`). Needed for state QSO parties and any spec
    /// that requires config predicates beyond the basic zone/name/xchg set.
    #[serde(default)]
    pub station: BTreeMap<String, toml::Value>,
    pub macros: Option<logger_runtime::MacroOverrides>,
    pub category: Option<logger_runtime::CategoryConfig>,
}

/// Merged runtime configuration. Assembled from `StableConfig` + `ContestConfig`
/// at load time; the rest of the TUI and `bootstrap()` read from this flat
/// shape so the split is purely a loading-boundary concern.
#[derive(Debug)]
pub struct Config {
    pub my_call: String,
    pub my_zone: u8,
    pub contest: String,
    pub rst_sent: String,
    pub my_name: Option<String>,
    pub my_xchg: Option<String>,
    pub station: BTreeMap<String, toml::Value>,
    pub db_path: Option<PathBuf>,
    pub call_history_file: Option<PathBuf>,
    pub scp_file: Option<PathBuf>,
    pub cursor_style: CursorStyle,
    pub macros: Option<logger_runtime::MacroOverrides>,
    pub rigs: Vec<logger_runtime::RigConfig>,
    pub keyer: Option<logger_runtime::KeyerConfig>,
    pub dxfeed: Option<logger_runtime::DxFeedConfig>,
    pub so2r: Option<logger_runtime::So2rConfig>,
    pub category: Option<logger_runtime::CategoryConfig>,
    pub cabrillo: logger_runtime::CabrilloConfig,
    pub scoreboard: ScoreboardSection,
    pub bandmap: BandmapMode,
    pub show_passband_qrm: bool,
    pub esm_enabled: bool,
    pub block_dupes: bool,
}

impl Config {
    fn from_parts(stable: StableConfig, contest: ContestConfig) -> Self {
        Self {
            my_call: stable.my_call,
            my_zone: stable.my_zone,
            contest: contest.contest,
            rst_sent: stable.rst_sent,
            my_name: stable.my_name,
            my_xchg: contest.my_xchg,
            station: contest.station,
            db_path: contest.db_path,
            call_history_file: contest.call_history_file,
            scp_file: stable.scp_file,
            cursor_style: stable.cursor_style,
            macros: contest.macros,
            rigs: stable.rigs,
            keyer: stable.keyer,
            dxfeed: stable.dxfeed,
            so2r: stable.so2r,
            category: contest.category,
            cabrillo: stable.cabrillo,
            scoreboard: stable.scoreboard,
            bandmap: stable.bandmap,
            show_passband_qrm: stable.show_passband_qrm,
            esm_enabled: stable.esm_enabled,
            block_dupes: stable.block_dupes,
        }
    }
}

fn default_esm_enabled() -> bool {
    true
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandmapMode {
    #[default]
    Dual,
    R1,
    R2,
}

#[derive(Debug, Default, Deserialize)]
pub struct ScoreboardSection {
    /// Posting interval in seconds (default: 120).
    #[serde(default = "default_scoreboard_interval")]
    pub interval_secs: u64,
    /// Scoreboard endpoints; repeat `[[scoreboard.endpoints]]` to post to
    /// multiple services.
    #[serde(default)]
    pub endpoints: Vec<logger_runtime::ScoreboardEndpoint>,
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

fn default_scoreboard_interval() -> u64 {
    120
}

fn default_rst_sent() -> String {
    "599".to_string()
}

pub fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let stable_text = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("reading config file {}", cli.config.display()))?;
    let stable: StableConfig = toml::from_str(&stable_text)
        .with_context(|| format!("parsing {}", cli.config.display()))?;

    let contest_text = std::fs::read_to_string(&cli.contest)
        .with_context(|| format!("reading contest file {}", cli.contest.display()))?;
    let contest: ContestConfig = toml::from_str(&contest_text)
        .with_context(|| format!("parsing {}", cli.contest.display()))?;

    Ok(Config::from_parts(stable, contest))
}
