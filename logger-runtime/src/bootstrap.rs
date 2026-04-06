use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use logger_core::{
    AppState, CallHistoryLookup, ContestEntry, EntryState, EsmPolicy, Macros, NoCallHistory,
    NoScp, ScpLookup, contest_from_id,
};
use serde::Deserialize;
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
    }

    let scorer =
        crate::scoring::scorer_for_contest(contest.as_ref(), config.my_zone, &my_exchange);

    let mut state = AppState {
        now_ms: chrono::Utc::now().timestamp_millis(),
        focused_radio: 1,
        active_operator: 1,
        radios: HashMap::new(),
        entry: EntryState::from_spec(&contest.form_spec()),
        bandmap: Vec::new(),
        last_logged: None,
        my_call: config.my_call,
        my_zone: config.my_zone,
        rst_sent: config.rst_sent,
        my_exchange,
        esm_policy: EsmPolicy::default(),
        bandmap_cursor: None,
        default_cw_speed: config.default_cw_speed,
        serial_counter: None,
        last_logged_context: None,
    };

    let log_adapter = if let Some(db_path) = &config.db_path {
        LogAdapter::open_db(scorer, db_path)?
    } else {
        LogAdapter::new(scorer)
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
