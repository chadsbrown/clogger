use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Script {
    #[serde(default)]
    pub esm_policy: EsmPolicyConfig,
    pub events: Vec<ScriptEvent>,
    #[serde(default)]
    pub expectations: Expectations,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub enum ModeValue {
    Run,
    Sp,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Default, Deserialize)]
pub struct Expectations {
    #[serde(default)]
    pub qsos: Vec<ExpectedQso>,
    #[serde(default)]
    pub cw_sent_contains: Vec<String>,
    pub beep_error_count: Option<usize>,
    pub focus_field_id: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct ExpectedQso {
    pub call: String,
    pub rst: String,
    pub zone: u8,
}

#[derive(Debug, Default, Deserialize)]
pub struct EsmPolicyConfig {
    pub run_two_step: Option<bool>,
    pub sp_log_on_first_enter: Option<bool>,
    pub sp_send_tu: Option<bool>,
}
