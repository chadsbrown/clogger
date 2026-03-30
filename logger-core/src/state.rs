use std::collections::HashMap;

use crate::entry::state::EntryState;

pub type RadioId = u8;
pub type OperatorId = u16;
pub type QsoRef = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EsmPolicy {
    pub run_two_step: bool,
    pub sp_log_on_first_enter: bool,
    pub sp_send_tu: bool,
}

impl Default for EsmPolicy {
    fn default() -> Self {
        Self {
            run_two_step: true,
            sp_log_on_first_enter: true,
            sp_send_tu: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioState {
    pub freq_hz: u64,
    pub mode: String,
    pub is_ptt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub call: String,
    pub freq_hz: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QsoDraft {
    pub contest_id: String,
    pub callsign: String,
    pub band: String,
    pub mode: String,
    pub freq_hz: u64,
    pub exchange_schema_id: u16,
    pub exchange_pairs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Macros {
    pub f1: String,
    pub f2: String,
    pub f3: String,
}

impl Default for Macros {
    fn default() -> Self {
        Self {
            f1: "CQ TEST {MYCALL}".to_string(),
            f2: "{CALL} {RST_SENT} {MYZONE}".to_string(),
            f3: "TU {CALL}".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub now_ms: i64,
    pub focused_radio: RadioId,
    pub active_operator: OperatorId,
    pub radios: HashMap<RadioId, RadioState>,
    pub entry: EntryState,
    pub bandmap: Vec<Spot>,
    pub last_logged: Option<QsoRef>,
    pub my_call: String,
    pub my_zone: u8,
    pub rst_sent: String,
    pub my_exchange: HashMap<String, String>,
    pub esm_policy: EsmPolicy,
}

impl AppState {
    pub fn current_call(&self) -> String {
        self.entry
            .get_field_value_by_id(1)
            .map(|v| v.trim().to_uppercase())
            .unwrap_or_default()
    }
}
