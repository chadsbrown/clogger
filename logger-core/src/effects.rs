use crate::state::{QsoDraft, RadioId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeepKind {
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum So2rRxMode {
    /// Selected radio audio in both ears.
    Mono,
    /// Radio 1 left ear, Radio 2 right ear.
    Stereo,
    /// Radio 1 right ear, Radio 2 left ear.
    ReverseStereo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    CwSend { radio: RadioId, text: String },
    CwAbort,
    LogInsert { draft: QsoDraft },
    Beep { kind: BeepKind },
    UiSetFocus { field_id: u16 },
    RigSet { radio: RadioId, freq_hz: u64 },
    UiClearEntry,
    /// Operator's entry focus changed. Runtime updates OTRSP RX audio routing
    /// to follow the new focus. TX routing is NOT changed — TX is purely a
    /// function of where CW is being sent (handled implicitly by CwSend dispatch).
    So2rFocusChanged { radio: RadioId },
    /// Reprogram the keyer's current WPM immediately. Issued when the
    /// operator adjusts speed via PageUp/PageDown so a plain (non-macro) CW
    /// send picks up the new rate. Macro-driven sends still embed their own
    /// `{NN}` markers from `<`/`>` expansion.
    KeyerSetSpeed { wpm: u8 },
}
