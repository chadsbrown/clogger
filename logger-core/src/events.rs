use crate::{
    entry::state::OpMode,
    state::{OperatorId, RadioId, Spot},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    TimerTick {
        now_ms: i64,
    },
    RigStatus {
        radio: RadioId,
        freq_hz: u64,
        mode: String,
        is_ptt: bool,
    },
    SpotReceived {
        spot: Spot,
    },
    SetOpMode {
        mode: OpMode,
    },
    FocusRadio {
        radio: RadioId,
    },
    SetOperator {
        operator: OperatorId,
    },
    TextInput {
        s: String,
    },
    KeyPress {
        key: Key,
    },
    EsmTrigger,
}
