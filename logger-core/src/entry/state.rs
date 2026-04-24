use crate::entry::spec::EntryFormSpec;
use crate::state::LastLoggedContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMode {
    Run,
    Sp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsmStep {
    Idle,
    CallSent,
    ExchSent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    Unknown,
    Valid,
    Invalid(String),
}

impl Validation {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    pub fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFieldState {
    pub field_id: u16,
    pub label: String,
    pub value: String,
    pub cursor: usize,
    pub required: bool,
    pub width: u16,
    pub status: Validation,
    pub from_history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryState {
    pub fields: Vec<EntryFieldState>,
    pub focus: usize,
    pub overall: Validation,
    pub is_dupe: bool,
    pub is_new_mult: bool,
    pub is_passband_qrm: bool,
    pub mode: OpMode,
    pub esm_enabled: bool,
    /// When true, ESM (Enter) refuses to send or log if the current call is a
    /// known dupe. F-key macros remain available as override.
    pub block_dupes: bool,
    pub esm_step: EsmStep,
    pub scp_matches: Vec<String>,
    pub scp_n1_matches: Vec<String>,
    pub scp_cycle_index: Option<usize>,
    pub assigned_serial: Option<u32>,
    pub last_logged_context: Option<LastLoggedContext>,
}

impl EntryState {
    pub fn from_spec(spec: &EntryFormSpec) -> Self {
        let fields = spec
            .fields
            .iter()
            .map(|f| EntryFieldState {
                field_id: f.field_id,
                label: f.label.clone(),
                value: String::new(),
                cursor: 0,
                required: f.required,
                width: f.width,
                status: Validation::Unknown,
                from_history: false,
            })
            .collect();

        Self {
            fields,
            focus: 0,
            overall: Validation::Unknown,
            is_dupe: false,
            is_new_mult: false,
            is_passband_qrm: false,
            mode: OpMode::Run,
            esm_enabled: true,
            block_dupes: false,
            esm_step: EsmStep::Idle,
            scp_matches: Vec::new(),
            scp_n1_matches: Vec::new(),
            scp_cycle_index: None,
            assigned_serial: None,
            last_logged_context: None,
        }
    }

    pub fn clear_values(&mut self) {
        for field in &mut self.fields {
            field.value.clear();
            field.cursor = 0;
            field.status = Validation::Unknown;
            field.from_history = false;
        }
        self.focus = 0;
        self.overall = Validation::Unknown;
        self.is_dupe = false;
        self.is_new_mult = false;
        self.is_passband_qrm = false;
        self.scp_matches.clear();
        self.scp_n1_matches.clear();
        self.scp_cycle_index = None;
        self.assigned_serial = None;
        self.esm_step = EsmStep::Idle;
    }

    pub fn focused_mut(&mut self) -> Option<&mut EntryFieldState> {
        self.fields.get_mut(self.focus)
    }

    pub fn get_field_value_by_id(&self, field_id: u16) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.field_id == field_id)
            .map(|f| f.value.as_str())
    }

    pub fn first_invalid_index(&self) -> Option<usize> {
        self.fields.iter().position(|f| f.status.is_invalid())
    }
}
