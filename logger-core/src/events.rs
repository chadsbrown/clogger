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
    Left,
    Right,
    F1,
    F2,
    F3,
    F4,
    F5,
    F7,
    F8,
    F9,
    F12,
    CtrlAltF1,
    CtrlAltF2,
    CtrlAltF3,
    CtrlAltF4,
    CtrlAltF5,
    CtrlAltF6,
    CtrlAltF7,
    CtrlAltF8,
    CtrlAltF9,
    CtrlAltF10,
    CtrlAltF11,
    CtrlAltF12,
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
        filter_width_hz: Option<u32>,
    },
    SpotReceived {
        spot: Spot,
    },
    SpotWithdrawn {
        call: String,
    },
    SetOpMode {
        mode: OpMode,
    },
    ToggleOpMode,
    FocusRadio {
        radio: RadioId,
    },
    SwapRadios,
    SetOperator {
        operator: OperatorId,
    },
    TextInput {
        s: String,
    },
    KeyPress {
        key: Key,
    },
    RigDisconnected {
        radio: RadioId,
    },
    /// WinKey / CW keyer lost its serial connection (detected by the keyer
    /// task). The reducer treats this as a no-op; the TUI updates its
    /// status indicator and error banner.
    KeyerDisconnected,
    /// Transient keyer failure (e.g., `send_raw` returned an error but the
    /// connection may still be usable). Reducer no-op; TUI shows a toast.
    KeyerError {
        message: String,
    },
    /// OTRSP SO2R switch lost its serial connection.
    So2rDisconnected,
    /// Transient OTRSP failure.
    So2rError {
        message: String,
    },
    /// SQLite persist task failed to append ops. QSO is still in memory;
    /// operator should be warned that the log is drifting from disk.
    PersistError {
        message: String,
    },
    QuickLog,
    EsmTrigger,
    BandmapUp { radio: RadioId },
    BandmapDown { radio: RadioId },
}
