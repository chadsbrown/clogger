use std::collections::HashMap;

use contest_engine::spec::Value;

use crate::{
    entry::{spec::EntryFormSpec, state::EntryState, validation::EntryValidation},
    state::{Macros, QsoDraft, RadioState},
};

/// The mode category for the entry, used to select the correct Cabrillo
/// contest ID (e.g. CQ-WW-CW vs CQ-WW-SSB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryMode {
    CW,
    SSB,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryContext {
    pub my_call: String,
    pub my_zone: u8,
    pub rst_sent: String,
    pub rig: Option<RadioState>,
    pub serial: Option<u32>,
    /// Typed station config, used by `sent_exchange_pairs` to resolve
    /// `SentValue::Config(key)` and to evaluate conditional sent-variant
    /// `when` predicates (e.g. `my_is_fl = true` for state-QP in-state ops).
    pub station_config: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryError {
    pub message: String,
}

pub trait ContestEntry {
    fn contest_id(&self) -> &str;
    fn contest_name(&self) -> &str { self.contest_id() }
    fn contest_instance_id(&self) -> u64;
    fn default_macros(&self) -> Macros;
    fn form_spec(&self) -> EntryFormSpec;
    fn validate_entry(&self, input: &EntryState, ctx: &EntryContext) -> EntryValidation;
    fn build_qso_draft(
        &self,
        input: &EntryState,
        ctx: &EntryContext,
    ) -> Result<QsoDraft, EntryError>;

    /// Build one or more drafts for a single ESM-log action. Returning more
    /// than one handles the county-line rover pattern: when a state-QP
    /// operator receives a multi-county exchange like "DAD/GRN/POL", each
    /// county is logged as its own QsoRecord so scoring and Cabrillo output
    /// match the N-QSOs-per-transmission convention that state QP sponsors
    /// use. Default implementation wraps `build_qso_draft` in a Vec — for
    /// contests that don't need splitting, overriding is not necessary.
    fn build_qso_drafts(
        &self,
        input: &EntryState,
        ctx: &EntryContext,
    ) -> Result<Vec<QsoDraft>, EntryError> {
        Ok(vec![self.build_qso_draft(input, ctx)?])
    }

    /// Maps .ch column names to form field_ids for history pre-population.
    fn history_field_mapping(&self) -> Vec<(&str, u16)> {
        vec![]
    }

    /// Whether this contest uses auto-incrementing serial numbers in the sent exchange.
    fn uses_serial(&self) -> bool {
        false
    }

    /// The Cabrillo contest ID for this contest and mode category.
    /// Returns None if the contest doesn't support the given mode.
    fn cabrillo_id(&self, _mode: CategoryMode) -> Option<&'static str> {
        None
    }

    /// The RTC (Real Time Contest) server's contest identifier for this
    /// contest and mode category. Returns None when the RTC server
    /// doesn't accept uploads for this contest — the RTC adapter then
    /// skips posting. Distinct from `cabrillo_id`: the RTC server uses
    /// its own naming (e.g. "CW-Ops" where Cabrillo uses "CW-OPS").
    fn rtc_id(&self, _mode: CategoryMode) -> Option<&'static str> {
        None
    }

    /// Whether the operating mode should auto-toggle (RUN↔S&P) after logging a QSO.
    /// Used by Sprint-style contests where the run station must QSY after each contact.
    fn auto_toggle_mode(&self) -> bool {
        false
    }

    /// `field_id` of the RST field if this contest has one. Used to skip the
    /// field on Space and to auto-populate it after each log.
    fn rst_field_id(&self) -> Option<u16> {
        None
    }

    /// Default RST string for the given normalized mode (`"CW"`, `"SSB"`,
    /// `"DIGITAL"`, `"OTHER"`). For spec-driven contests this comes from the
    /// matching variant's `sent_rst_value`.
    fn default_rst(&self, _mode: &str) -> Option<String> {
        None
    }

    /// Resolved sent-exchange fields at log time, as `(sent_<field_id>,
    /// value)` pairs suitable for appending to `QsoDraft.exchange_pairs`.
    /// Does *not* include the sent serial (which is handled separately via
    /// the `serial` key from `assigned_serial`). Does *not* filter out
    /// signal reports — consumers decide per-format whether to drop RST.
    /// Default impl returns empty.
    fn sent_exchange_pairs(&self, _ctx: &EntryContext) -> Vec<(String, String)> {
        Vec::new()
    }
}
