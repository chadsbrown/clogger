use crate::entry::state::Validation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryValidation {
    pub fields: Vec<Validation>,
    pub overall: Validation,
    pub first_invalid: Option<usize>,
}
