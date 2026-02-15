#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFieldSpec {
    pub field_id: u16,
    pub label: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFormSpec {
    pub fields: Vec<EntryFieldSpec>,
}
