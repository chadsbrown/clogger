use logger_core::BandmapCursor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Script {
    pub contest: Option<String>,
    #[serde(default)]
    pub esm_policy: EsmPolicyConfig,
    #[serde(default)]
    pub uses_serial: bool,
    #[serde(default)]
    pub macro_overrides: MacroOverrides,
    #[serde(default)]
    pub call_history: Vec<CallHistoryEntry>,
    #[serde(default)]
    pub scp_calls: Vec<String>,
    /// Station's own exchange fields (keys become `my_<key>` config for the scorer).
    #[serde(default)]
    pub my_exchange: BTreeMap<String, String>,
    /// Passband QRM warning width in hertz. `None` disables the warning.
    #[serde(default)]
    pub passband_qrm_width_hz: Option<u32>,
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
        #[serde(default)]
        filter_width_hz: Option<u32>,
    },
    SetMode {
        mode: ModeValue,
    },
    FocusRadio {
        radio: u8,
    },
    SwapRadios,
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
        #[serde(default = "default_mode")]
        mode: String,
    },
    SpotWithdraw {
        call: String,
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
    Equal,
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
    pub final_is_passband_qrm: Option<bool>,
    pub final_field_values: Option<BTreeMap<u16, String>>,
    pub final_serial_counter: Option<u32>,
    pub final_focused_radio: Option<u8>,
    /// Per-radio entry expectations: keyed by radio_id
    #[serde(default)]
    pub final_radio_entries: BTreeMap<u8, RadioEntryExpectation>,
    /// Expected per-radio bandmap cursor state at end of script.
    pub final_bandmap_cursors: Option<BTreeMap<u8, BandmapCursor>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RadioEntryExpectation {
    pub op_mode: Option<String>,
    pub esm_step: Option<String>,
    pub field_values: Option<BTreeMap<u16, String>>,
    pub focus_field_id: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpectedQso {
    pub call: String,
    pub band: Option<String>,
    pub rst: Option<String>,
    pub zone: Option<u8>,
    pub exchange: Option<BTreeMap<String, String>>,
}

fn default_mode() -> String {
    "CW".to_string()
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EsmPolicyConfig {
    pub run_two_step: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MacroOverrides {
    pub f1: Option<String>,
    pub f2: Option<String>,
    pub f3: Option<String>,
    pub f5: Option<String>,
    pub f7: Option<String>,
    pub f8: Option<String>,
    pub f9: Option<String>,
}
