//! CLI parsing and TOML config loading. Matches the shape of the TUI's
//! stable + per-contest config split so existing user configs work unchanged
//! (TUI-only fields like `cursor_style` / `theme` are silently ignored).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use logger_runtime::ConfigValue;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(name = "logger-gui", about = "Clogger GUI (iced)")]
pub struct Cli {
    /// Path to stable TOML config file (station identity, hardware).
    /// Optional: if omitted along with --contest, the GUI runs in demo mode.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Path to the per-contest TOML file.
    #[arg(long)]
    pub contest: Option<PathBuf>,
    /// SQLite database file (overrides db_path in the contest file).
    #[arg(short, long)]
    pub db: Option<PathBuf>,
    /// Call history file (.ch) override.
    #[arg(long)]
    pub call_history: Option<PathBuf>,
    /// SCP file override.
    #[arg(long)]
    pub scp: Option<PathBuf>,
    /// cty.dat file override.
    #[arg(long)]
    pub cty: Option<PathBuf>,
    /// Enable debug-level logging.
    #[arg(long)]
    pub debug: bool,
}

/// Slim mirror of `StableConfig` from `logger-tui`. Only fields the GUI
/// actually reads. Unknown fields in the user's TOML (e.g. `cursor_style`,
/// `theme`) are silently ignored — serde's default behavior.
#[derive(Debug, Deserialize, Default)]
pub struct StableConfig {
    pub my_call: String,
    #[serde(default)]
    pub my_zone: u8,
    #[serde(default = "default_rst_sent")]
    pub rst_sent: String,
    pub my_name: Option<String>,
    pub scp_file: Option<PathBuf>,
    pub cty_file: Option<PathBuf>,
    #[serde(default, rename = "rig")]
    pub rigs: Vec<logger_runtime::RigConfig>,
    pub keyer: Option<logger_runtime::KeyerConfig>,
    pub dxfeed: Option<logger_runtime::DxFeedConfig>,
    pub so2r: Option<logger_runtime::So2rConfig>,
    #[serde(default)]
    pub condx: logger_runtime::CondXConfig,
    #[serde(default)]
    pub scoreboard: ScoreboardSection,
    pub rtc: Option<logger_runtime::RtcConfig>,
    #[serde(default)]
    pub show_passband_qrm: bool,
    #[serde(default)]
    pub bandmap_skip_worked: bool,
    #[serde(default)]
    pub bandmap_high_at_top: bool,
    #[serde(default = "default_true")]
    pub esm_enabled: bool,
    #[serde(default)]
    pub block_dupes: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct ScoreboardSection {
    #[serde(default = "default_scoreboard_interval")]
    pub interval_secs: u64,
    #[serde(default)]
    pub endpoints: Vec<logger_runtime::ScoreboardEndpoint>,
}

fn default_scoreboard_interval() -> u64 {
    120
}

#[derive(Debug, Deserialize)]
pub struct ContestConfig {
    pub contest: String,
    pub my_xchg: Option<String>,
    pub db_path: Option<PathBuf>,
    pub call_history_file: Option<PathBuf>,
    #[serde(default)]
    pub station: BTreeMap<String, toml::Value>,
    pub macros: Option<logger_runtime::MacroOverrides>,
    /// Required when `[[scoreboard.endpoints]]` is configured — supplies
    /// the Cabrillo category used in the score uploads.
    pub category: Option<logger_runtime::CategoryConfig>,
}

/// Fields consumed by `logger_runtime::bootstrap`. Moved out of `Config`
/// once at startup; bootstrap takes ownership.
pub struct SessionBits {
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
    pub cty_file: Option<PathBuf>,
    pub macros: Option<logger_runtime::MacroOverrides>,
    pub default_cw_speed: u8,
    pub show_passband_qrm: bool,
    pub bandmap_skip_worked: bool,
    pub bandmap_high_at_top: bool,
    pub esm_enabled: bool,
    pub block_dupes: bool,
    /// Pre-composed scoreboard spawn bundle. `None` when scoreboard is
    /// not configured. Bootstrap spawns the adapter from this.
    pub scoreboard: Option<logger_runtime::ScoreboardConfig>,
    /// Pre-composed RTC spawn bundle. `None` when RTC is disabled or
    /// the current contest has no `rtc_id`.
    pub rtc: Option<logger_runtime::config::RtcSpawnConfig>,
}

/// Hardware adapter configuration. Held separately from `SessionBits` so
/// `bridge::spawn_adapters` can run its async setup after bootstrap returns.
#[derive(Debug, Default)]
pub struct AdapterBits {
    pub rigs: Vec<logger_runtime::RigConfig>,
    pub keyer: Option<logger_runtime::KeyerConfig>,
    pub dxfeed: Option<logger_runtime::DxFeedConfig>,
    pub so2r: Option<logger_runtime::So2rConfig>,
    pub condx: logger_runtime::CondXConfig,
    /// `true` when scoreboard has configured endpoints. Used purely
    /// for the status-bar footer badge; the adapter itself is spawned
    /// inside bootstrap from `SessionBits.scoreboard`.
    pub scoreboard_configured: bool,
    /// `true` when the RTC adapter was actually spawned by bootstrap
    /// — i.e. `[rtc] enabled = true` AND the contest has an `rtc_id`
    /// mapped. Drives the RTC badge; ignored otherwise.
    pub rtc_configured: bool,
}

/// Combined runtime configuration. Split into parts before use so each
/// half can be moved into the right consumer.
pub struct Config {
    pub session: SessionBits,
    pub adapters: AdapterBits,
}

impl Config {
    pub fn into_parts(self) -> (SessionBits, AdapterBits) {
        (self.session, self.adapters)
    }
}

pub fn load(cli: &Cli) -> Result<Option<Config>> {
    match (cli.config.as_ref(), cli.contest.as_ref()) {
        (Some(c), Some(ct)) => Ok(Some(load_pair(cli, c, ct)?)),
        (None, None) => Ok(None),
        _ => anyhow::bail!("--config and --contest must be given together (or neither, for demo mode)"),
    }
}

fn load_pair(cli: &Cli, config_path: &PathBuf, contest_path: &PathBuf) -> Result<Config> {
    let stable_text = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading config file {}", config_path.display()))?;
    let stable: StableConfig = toml::from_str(&stable_text)
        .with_context(|| format!("parsing {}", config_path.display()))?;

    let contest_text = std::fs::read_to_string(contest_path)
        .with_context(|| format!("reading contest file {}", contest_path.display()))?;
    let contest: ContestConfig = toml::from_str(&contest_text)
        .with_context(|| format!("parsing {}", contest_path.display()))?;

    let default_cw_speed = stable
        .rigs
        .iter()
        .find_map(|r| r.cw_speed)
        .or_else(|| stable.keyer.as_ref().map(|k| k.speed_wpm))
        .unwrap_or(28);

    // Resolve the contest object once so we can look up cabrillo_id
    // (for scoreboard) and rtc_id (for RTC) before handing ownership
    // to bootstrap. Bootstrap will resolve the same contest_id again
    // internally — cheap because contest-engine specs are embedded.
    let contest_obj = logger_core::contest_from_id(&contest.contest)
        .ok_or_else(|| anyhow::anyhow!("unknown contest: {}", contest.contest))?;

    let db_path = cli.db.as_ref().or(contest.db_path.as_ref()).cloned();
    let scoreboard_configured = !stable.scoreboard.endpoints.is_empty();
    let scoreboard_bundle = compose_scoreboard(
        &stable.scoreboard,
        contest.category.as_ref(),
        contest_obj.as_ref(),
        &contest.contest,
        &stable.my_call,
    )?;
    let rtc_bundle = compose_rtc(
        stable.rtc.as_ref(),
        contest.category.as_ref(),
        contest_obj.as_ref(),
        &contest.contest,
        &stable.my_call,
        db_path.as_deref(),
    )?;

    let session = SessionBits {
        my_call: stable.my_call.clone(),
        my_zone: stable.my_zone,
        contest: contest.contest.clone(),
        rst_sent: stable.rst_sent,
        my_name: stable.my_name,
        my_xchg: contest.my_xchg,
        station: contest.station,
        db_path,
        call_history_file: cli
            .call_history
            .as_ref()
            .or(contest.call_history_file.as_ref())
            .cloned(),
        scp_file: cli.scp.as_ref().or(stable.scp_file.as_ref()).cloned(),
        cty_file: cli.cty.as_ref().or(stable.cty_file.as_ref()).cloned(),
        macros: contest.macros,
        default_cw_speed,
        show_passband_qrm: stable.show_passband_qrm,
        bandmap_skip_worked: stable.bandmap_skip_worked,
        bandmap_high_at_top: stable.bandmap_high_at_top,
        esm_enabled: stable.esm_enabled,
        block_dupes: stable.block_dupes,
        scoreboard: scoreboard_bundle,
        rtc: rtc_bundle,
    };

    // `rtc_configured` reflects what `compose_rtc` actually produced:
    // `Some(_)` means bootstrap will spawn the adapter; `None` means
    // it's gated off (disabled, missing category, or the contest has
    // no rtc_id). The GUI footer reads this from the handles struct,
    // so it must match the spawn outcome exactly.
    let rtc_configured = session.rtc.is_some();

    let adapters = AdapterBits {
        rigs: stable.rigs,
        keyer: stable.keyer,
        dxfeed: stable.dxfeed,
        so2r: stable.so2r,
        condx: stable.condx,
        scoreboard_configured,
        rtc_configured,
    };

    Ok(Config { session, adapters })
}

fn compose_scoreboard(
    section: &ScoreboardSection,
    category: Option<&logger_runtime::CategoryConfig>,
    contest: &dyn logger_core::ContestEntry,
    contest_id: &str,
    my_call: &str,
) -> Result<Option<logger_runtime::ScoreboardConfig>> {
    if section.endpoints.is_empty() {
        return Ok(None);
    }
    let category = category.ok_or_else(|| {
        anyhow::anyhow!("[category] is required when [[scoreboard.endpoints]] is configured")
    })?;
    let cabrillo_id = contest
        .cabrillo_id(category.mode.to_category_mode())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "contest '{}' has no Cabrillo ID for mode '{}'",
                contest_id,
                category.mode.as_str()
            )
        })?;
    Ok(Some(logger_runtime::ScoreboardConfig {
        endpoints: section.endpoints.clone(),
        interval_secs: section.interval_secs,
        cabrillo_id,
        call: my_call.to_string(),
        ops: my_call.to_string(),
        category: category.clone(),
    }))
}

fn compose_rtc(
    rtc_cfg: Option<&logger_runtime::RtcConfig>,
    category: Option<&logger_runtime::CategoryConfig>,
    contest: &dyn logger_core::ContestEntry,
    contest_id: &str,
    my_call: &str,
    db_path: Option<&std::path::Path>,
) -> Result<Option<logger_runtime::config::RtcSpawnConfig>> {
    let Some(rtc_cfg) = rtc_cfg else { return Ok(None) };
    if !rtc_cfg.enabled {
        return Ok(None);
    }
    rtc_cfg.validate().map_err(|e| anyhow::anyhow!(e))?;

    let category = category.ok_or_else(|| {
        anyhow::anyhow!("[category] is required when [rtc] is enabled")
    })?;
    let Some(contest_rtc_id) = contest.rtc_id(category.mode.to_category_mode()) else {
        tracing::info!(
            "RTC enabled but contest '{}' has no rtc_id for mode '{}' — skipping",
            contest_id,
            category.mode.as_str()
        );
        return Ok(None);
    };

    let db_path = db_path.ok_or_else(|| {
        anyhow::anyhow!("[rtc] requires a db_path — RTC's CFM state is persisted alongside the log")
    })?;
    let mut sidecar_path = db_path.to_path_buf();
    sidecar_path.set_extension("rtc-state.json");

    let user_agent = rtc_cfg
        .user_agent
        .clone()
        .unwrap_or_else(|| format!("clogger/{}", env!("CARGO_PKG_VERSION")));

    Ok(Some(logger_runtime::config::RtcSpawnConfig {
        http: rtc_cfg.clone(),
        contest_rtc_id,
        my_call: my_call.to_string(),
        contest_instance_id: contest.contest_instance_id(),
        sidecar_path,
        user_agent,
        category: category.clone(),
    }))
}

/// Convert the typed `[station]` table into contest-engine values.
/// Bool/Int/String only — anything else is a hard error, matching the TUI.
pub fn station_to_config_values(
    station: &BTreeMap<String, toml::Value>,
) -> Result<HashMap<String, ConfigValue>> {
    let mut out = HashMap::new();
    for (key, value) in station {
        let cv = match value {
            toml::Value::Boolean(b) => ConfigValue::Bool(*b),
            toml::Value::Integer(i) => ConfigValue::Int(*i),
            toml::Value::String(s) => ConfigValue::Text(s.clone()),
            other => anyhow::bail!(
                "[station] key `{}` has unsupported TOML type `{}` — only bool, integer, and string are allowed",
                key,
                other.type_str()
            ),
        };
        out.insert(key.clone(), cv);
    }
    Ok(out)
}

fn default_true() -> bool {
    true
}

fn default_rst_sent() -> String {
    "599".to_string()
}
