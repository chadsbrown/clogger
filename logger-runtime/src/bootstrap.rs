use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use logger_core::{
    AppEvent, AppState, CallHistoryLookup, ContestEntry, EntryState, EsmPolicy, Macros,
    NoCallHistory, NoScp, ScpLookup, contest_from_id,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::warn;

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
}

pub struct SessionConfig {
    pub contest_id: String,
    pub my_call: String,
    pub my_zone: u8,
    pub rst_sent: String,
    pub my_name: Option<String>,
    pub my_xchg: Option<String>,
    pub macro_overrides: Option<MacroOverrides>,
    pub default_cw_speed: u8,
    pub db_path: Option<PathBuf>,
    pub call_history_path: Option<PathBuf>,
    pub scp_path: Option<PathBuf>,
    pub start_serial: Option<u32>,
    /// Enable the passband QRM warning. The width comes from the rig's
    /// reported receive filter (or a mode-default fallback); this flag just
    /// toggles the feature on/off.
    pub show_passband_qrm: bool,
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
    pub call_history: Box<dyn CallHistoryLookup>,
    pub scp: Box<dyn ScpLookup>,
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

    let scorer =
        crate::scoring::scorer_for_contest(contest.as_ref(), config.my_zone, &my_exchange);

    // Initialize entries for both radios so SO2R works out of the box
    let mut entries = HashMap::new();
    entries.insert(1, EntryState::from_spec(&contest.form_spec()));
    entries.insert(2, EntryState::from_spec(&contest.form_spec()));

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
        esm_policy: EsmPolicy::default(),
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

    let call_history = load_call_history(config.call_history_path.as_deref());
    let scp = load_scp(config.scp_path.as_deref());

    Ok(Session {
        state,
        contest,
        macros,
        log_adapter,
        call_history,
        scp,
    })
}

fn load_call_history(path: Option<&Path>) -> Box<dyn CallHistoryLookup> {
    let Some(path) = path else {
        return Box::new(NoCallHistory);
    };
    match crate::CallHistoryDb::load(path) {
        Ok(db) => Box::new(db),
        Err(e) => {
            warn!("call history load failed, continuing without: {e}");
            Box::new(NoCallHistory)
        }
    }
}

fn load_scp(path: Option<&Path>) -> Box<dyn ScpLookup> {
    let Some(path) = path else {
        return Box::new(NoScp);
    };
    match crate::ScpDb::load(path) {
        Ok(db) => Box::new(db),
        Err(e) => {
            warn!("SCP file load failed, continuing without: {e}");
            Box::new(NoScp)
        }
    }
}
