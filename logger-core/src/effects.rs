use crate::state::{QsoDraft, RadioId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeepKind {
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    CwSend { radio: RadioId, text: String },
    LogInsert { draft: QsoDraft },
    Beep { kind: BeepKind },
    UiSetFocus { field_id: u16 },
    UiClearEntry,
}
