use crate::{
    entry::{spec::EntryFormSpec, state::EntryState, validation::EntryValidation},
    state::{QsoDraft, RadioState},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryContext {
    pub my_call: String,
    pub my_zone: u8,
    pub rst_sent: String,
    pub rig: Option<RadioState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryError {
    pub message: String,
}

pub trait ContestEntry {
    fn form_spec(&self) -> EntryFormSpec;
    fn validate_entry(&self, input: &EntryState, ctx: &EntryContext) -> EntryValidation;
    fn build_qso_draft(&self, input: &EntryState, ctx: &EntryContext) -> Result<QsoDraft, EntryError>;
}
