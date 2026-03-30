use crate::{
    entry::{spec::EntryFormSpec, state::EntryState, validation::EntryValidation},
    state::{Macros, QsoDraft, RadioState},
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
    fn contest_id(&self) -> &str;
    fn contest_instance_id(&self) -> u64;
    fn default_macros(&self) -> Macros;
    fn form_spec(&self) -> EntryFormSpec;
    fn validate_entry(&self, input: &EntryState, ctx: &EntryContext) -> EntryValidation;
    fn build_qso_draft(
        &self,
        input: &EntryState,
        ctx: &EntryContext,
    ) -> Result<QsoDraft, EntryError>;

    /// Maps .ch column names to form field_ids for history pre-population.
    fn history_field_mapping(&self) -> Vec<(&str, u16)> {
        vec![]
    }
}
