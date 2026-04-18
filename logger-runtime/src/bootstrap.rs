use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use contest_engine::spec::Value as ConfigValue;
use logger_core::{
    AppEvent, AppState, CallHistoryLookup, ContestEntry, EntryState, Macros, NoCallHistory, NoScp,
    ScpLookup, apply_default_rst, contest_from_id,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::warn;

use crate::cty::CtyDb;
use crate::log_adapter::LogAdapter;

#[derive(Debug, Deserialize, Default)]
pub struct MacroOverrides {
    pub f1: Option<String>,
    pub f2: Option<String>,
    pub f3: Option<String>,
    pub f5: Option<String>,
    pub f7: Option<String>,
    pub f8: Option<String>,
    pub f9: Option<String>,
    pub ctrl_alt_f1: Option<String>,
    pub ctrl_alt_f2: Option<String>,
    pub ctrl_alt_f3: Option<String>,
    pub ctrl_alt_f4: Option<String>,
    pub ctrl_alt_f5: Option<String>,
    pub ctrl_alt_f6: Option<String>,
    pub ctrl_alt_f7: Option<String>,
    pub ctrl_alt_f8: Option<String>,
    pub ctrl_alt_f9: Option<String>,
    pub ctrl_alt_f10: Option<String>,
    pub ctrl_alt_f11: Option<String>,
    pub ctrl_alt_f12: Option<String>,
}

pub struct SessionConfig {
    pub contest_id: String,
    pub my_call: String,
    pub my_zone: u8,
    pub rst_sent: String,
    pub my_name: Option<String>,
    pub my_xchg: Option<String>,
    /// Typed station-config passthrough. Keys are fed verbatim to contest-engine
    /// as config values (e.g. `my_is_fl = Bool(true)` for state QSO parties).
    /// Merged on top of back-compat defaults from `my_zone`, `my_name`, `my_xchg`.
    pub station_config: HashMap<String, ConfigValue>,
    pub macro_overrides: Option<MacroOverrides>,
    pub default_cw_speed: u8,
    pub db_path: Option<PathBuf>,
    pub call_history_path: Option<PathBuf>,
    pub scp_path: Option<PathBuf>,
    pub cty_path: Option<PathBuf>,
    pub start_serial: Option<u32>,
    /// Enable the passband QRM warning. The width comes from the rig's
    /// reported receive filter (or a mode-default fallback); this flag just
    /// toggles the feature on/off.
    pub show_passband_qrm: bool,
    /// Enable Enter Sends Message (ESM). When false, Enter only logs (no
    /// CW is sent automatically); operators send CW via the F-key macros.
    /// Default: true.
    pub esm_enabled: bool,
    /// When true, ESM (Enter) refuses to send or log if the call in the
    /// entry box is a known dupe for the current band/mode. Operators can
    /// still send CW manually with F-keys to confirm the dupe out loud.
    /// Default: false.
    pub block_dupes: bool,
    /// Channel for hardware-task error events (persist, keyer, rig, so2r).
    /// Required: the TUI must pass its `app_tx` so per-device tasks can
    /// surface errors back to the main event loop.
    pub app_tx: mpsc::Sender<AppEvent>,
}

pub struct Session {
    pub state: AppState,
    pub contest: Box<dyn ContestEntry>,
    pub macros: Macros,
    pub log_adapter: LogAdapter,
    /// Arc so the scorer (for bandmap mult downgrade via call history) and
    /// the event-loop reducer can share one loaded .ch DB without double-
    /// loading or double-parsing.
    pub call_history: Arc<dyn CallHistoryLookup>,
    /// Arc so the dxfeed enrichment resolver can share the same SCP DB
    /// that feeds call-completion in the reducer — one file load, two
    /// consumers.
    pub scp: Arc<dyn ScpLookup>,
    /// Parsed cty.dat (DXCC/entity database). `None` when no `cty_file`
    /// is configured. Consumed by `spawn_dxfeed_adapter` to drive
    /// `geo.*` filter rules (continent/zone/entity). Not used elsewhere
    /// yet; future callers (e.g. bandmap entity display) can take a clone.
    pub cty: Option<Arc<CtyDb>>,
}

pub fn bootstrap(config: SessionConfig) -> Result<Session> {
    let contest = contest_from_id(&config.contest_id)
        .ok_or_else(|| anyhow::anyhow!("unknown contest: {}", config.contest_id))?;

    let mut macros = contest.default_macros();
    if let Some(overrides) = &config.macro_overrides {
        if let Some(ref v) = overrides.f1 { macros.f1 = v.clone(); }
        if let Some(ref v) = overrides.f2 { macros.f2 = v.clone(); }
        if let Some(ref v) = overrides.f3 { macros.f3 = v.clone(); }
        if let Some(ref v) = overrides.f5 { macros.f5 = v.clone(); }
        if let Some(ref v) = overrides.f7 { macros.f7 = v.clone(); }
        if let Some(ref v) = overrides.f8 { macros.f8 = v.clone(); }
        if let Some(ref v) = overrides.f9 { macros.f9 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f1 { macros.ctrl_alt_f1 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f2 { macros.ctrl_alt_f2 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f3 { macros.ctrl_alt_f3 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f4 { macros.ctrl_alt_f4 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f5 { macros.ctrl_alt_f5 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f6 { macros.ctrl_alt_f6 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f7 { macros.ctrl_alt_f7 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f8 { macros.ctrl_alt_f8 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f9 { macros.ctrl_alt_f9 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f10 { macros.ctrl_alt_f10 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f11 { macros.ctrl_alt_f11 = v.clone(); }
        if let Some(ref v) = overrides.ctrl_alt_f12 { macros.ctrl_alt_f12 = v.clone(); }
    }

    let mut my_exchange = HashMap::new();
    if let Some(name) = &config.my_name {
        my_exchange.insert("NAME".to_string(), name.clone());
    }
    if let Some(xchg) = &config.my_xchg {
        my_exchange.insert("XCHG".to_string(), xchg.clone());
        // Also insert as LOC — specs like NAQP and NS Sprint reference my_loc
        // for the location field, while the TOML config uses the generic my_xchg.
        my_exchange.insert("LOC".to_string(), xchg.clone());
    }

    // Typed config map for contest-engine: back-compat defaults from my_zone +
    // my_name/my_xchg, then overlay [station] passthrough so users can set
    // typed config keys (e.g. `my_is_fl = true`) that specs require.
    let mut scorer_config: HashMap<String, ConfigValue> = HashMap::new();
    scorer_config.insert(
        "my_cq_zone".to_string(),
        ConfigValue::Int(i64::from(config.my_zone)),
    );
    if let Some(name) = &config.my_name {
        scorer_config.insert("my_name".to_string(), ConfigValue::Text(name.clone()));
    }
    if let Some(xchg) = &config.my_xchg {
        scorer_config.insert("my_xchg".to_string(), ConfigValue::Text(xchg.clone()));
        scorer_config.insert("my_loc".to_string(), ConfigValue::Text(xchg.clone()));
    }
    for (key, value) in &config.station_config {
        scorer_config.insert(key.clone(), value.clone());
    }

    let call_history = load_call_history(config.call_history_path.as_deref());

    let scorer = crate::scoring::scorer_for_contest(
        contest.as_ref(),
        scorer_config,
        Arc::clone(&call_history),
    )
    .with_context(|| {
        format!(
            "contest scorer init failed for `{}` — fix your contest.toml [station] section",
            config.contest_id
        )
    })?;

    // Initialize entries for both radios so SO2R works out of the box.
    // RST defaults to the CW value at bootstrap; once a rig reports its mode
    // and the operator logs a QSO, subsequent entries are populated from the
    // current rig mode (see `apply_default_rst` calls in `log_and_clear`).
    let mut entries = HashMap::new();
    let mut entry1 = EntryState::from_spec(&contest.form_spec());
    let mut entry2 = EntryState::from_spec(&contest.form_spec());
    entry1.esm_enabled = config.esm_enabled;
    entry2.esm_enabled = config.esm_enabled;
    entry1.block_dupes = config.block_dupes;
    entry2.block_dupes = config.block_dupes;
    apply_default_rst(&mut entry1, contest.as_ref(), "CW");
    apply_default_rst(&mut entry2, contest.as_ref(), "CW");
    entries.insert(1, entry1);
    entries.insert(2, entry2);

    let mut state = AppState {
        now_ms: chrono::Utc::now().timestamp_millis(),
        focused_radio: 1,
        active_operator: 1,
        radios: HashMap::new(),
        entries,
        bandmap: Vec::new(),
        last_logged: None,
        my_call: config.my_call,
        my_zone: config.my_zone,
        rst_sent: config.rst_sent,
        my_exchange,
        bandmap_cursors: HashMap::new(),
        default_cw_speed: config.default_cw_speed,
        serial_counter: None,
        show_passband_qrm: config.show_passband_qrm,
        bandmap_version: 0,
    };

    let contest_instance_id = contest.contest_instance_id();
    let log_adapter = if let Some(db_path) = &config.db_path {
        // Persistence runs in a dedicated task; LogAdapter sends ops over
        // an mpsc channel instead of blocking the event loop on disk I/O.
        LogAdapter::open_db_async(scorer, contest_instance_id, db_path, config.app_tx.clone())?
    } else {
        LogAdapter::new(scorer, contest_instance_id)
    };

    // Initialize serial counter for contests that use it
    if contest.uses_serial() {
        let start = config
            .start_serial
            .unwrap_or_else(|| log_adapter.max_sent_serial() + 1);
        state.serial_counter = Some(start);
    }

    let scp = load_scp(config.scp_path.as_deref());
    let cty = load_cty(config.cty_path.as_deref())?;

    Ok(Session {
        state,
        contest,
        macros,
        log_adapter,
        call_history,
        scp,
        cty,
    })
}

fn load_call_history(path: Option<&Path>) -> Arc<dyn CallHistoryLookup> {
    let Some(path) = path else {
        return Arc::new(NoCallHistory);
    };
    match crate::CallHistoryDb::load(path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            warn!("call history load failed, continuing without: {e}");
            Arc::new(NoCallHistory)
        }
    }
}

fn load_scp(path: Option<&Path>) -> Arc<dyn ScpLookup> {
    let Some(path) = path else {
        return Arc::new(NoScp);
    };
    match crate::ScpDb::load(path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            warn!("SCP file load failed, continuing without: {e}");
            Arc::new(NoScp)
        }
    }
}

/// Load cty.dat when configured. Unlike SCP (which degrades to an empty
/// `NoScp` on failure), a bad cty.dat path is a hard error: the operator
/// explicitly asked for geo data, and silently running without it would
/// cause the same silent-drop failure mode that motivated wiring this up
/// in the first place. Omit `cty_path` entirely to skip.
fn load_cty(path: Option<&Path>) -> Result<Option<Arc<CtyDb>>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let db = CtyDb::load(path)
        .with_context(|| format!("loading cty_file {}", path.display()))?;
    Ok(Some(Arc::new(db)))
}
