use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Script {
    pub contest: Option<String>,
    #[serde(default)]
    pub esm_policy: EsmPolicyConfig,
    #[serde(default)]
    pub call_history: Vec<CallHistoryEntry>,
    pub events: Vec<ScriptEvent>,
    #[serde(default)]
    pub expectations: Expectations,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CallHistoryEntry {
    pub call: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum ScriptEvent {
    RigStatus {
        radio: u8,
        freq_hz: u64,
        mode: String,
        is_ptt: bool,
    },
    SetMode {
        mode: ModeValue,
    },
    FocusRadio {
        radio: u8,
    },
    Text {
        s: String,
    },
    Key {
        key: KeyValue,
    },
    Esm,
    Spot {
        call: String,
        freq_hz: u64,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ModeValue {
    Run,
    Sp,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum KeyValue {
    Space,
    Tab,
    Backspace,
    Esc,
    F1,
    F2,
    F3,
    Enter,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Expectations {
    #[serde(default)]
    pub qsos: Vec<ExpectedQso>,
    #[serde(default)]
    pub cw_sent_contains: Vec<String>,
    #[serde(default)]
    pub cw_sent_exact: Vec<String>,
    pub beep_error_count: Option<usize>,
    pub focus_field_id: Option<u16>,
    pub final_is_dupe: Option<bool>,
    pub final_is_new_mult: Option<bool>,
    pub final_field_values: Option<BTreeMap<u16, String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpectedQso {
    pub call: String,
    pub band: Option<String>,
    pub rst: Option<String>,
    pub zone: Option<u8>,
    pub exchange: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EsmPolicyConfig {
    pub run_two_step: Option<bool>,
    pub sp_log_on_first_enter: Option<bool>,
    pub sp_send_tu: Option<bool>,
}
