mod spec_scorer;
pub mod unique_call;

use std::collections::HashMap;
use std::sync::Arc;

use contest_engine::spec::Value;
use logger_core::{CallHistoryLookup, ContestEntry};
use qsolog::qso::QsoRecord;
use qsolog::types::Band;

#[derive(Debug, Clone)]
pub struct BandScore {
    pub qsos: u32,
    pub mults: u32,
}

#[derive(Debug, Clone)]
#[derive(Default)]
pub struct ScoreSummary {
    pub by_band: Vec<(String, BandScore)>,
    pub total_qsos: u32,
    pub total_mults: u32,
    pub claimed_score: i64,
}


/// A single row in the score breakdown, keyed by (band, mode).
#[derive(Debug, Clone)]
pub struct BreakdownRow {
    pub band: String,
    pub mode: String,
    pub qsos: u32,
    pub points: i64,
    /// Per-multiplier-type counts, e.g. [("zone", 15), ("country", 45)].
    pub mults: Vec<(String, u32)>,
}

/// Per-(band, mode) score breakdown for scoreboard XML posting.
/// Always includes at least a total row (band="total", mode="ALL").
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct ScoreBreakdown {
    pub rows: Vec<BreakdownRow>,
    pub claimed_score: i64,
}


pub trait ContestScorer: Send + Sync {
    /// A new QSO has been appended to the log. Update incremental state.
    fn on_inserted(&mut self, record: &QsoRecord);

    /// One or more existing records have been mutated (void/unvoid/edit).
    /// The scorer must rebuild its state by replaying `records`.
    fn rebuild(&mut self, records: &[QsoRecord]);

    /// Cheap read of current totals/breakdown.
    fn score_summary(&self) -> ScoreSummary;

    /// Per-(band, mode) breakdown for scoreboard posting.
    fn score_breakdown(&self) -> ScoreBreakdown;

    /// Would this candidate be a dupe right now? Contest-specific rules apply.
    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool;

    /// Would this candidate be a new mult right now? Must not mutate.
    fn would_be_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool;
}

pub fn scorer_for_contest(
    contest: &dyn ContestEntry,
    config: HashMap<String, Value>,
    call_history: Arc<dyn CallHistoryLookup>,
    cty: Option<Arc<crate::cty::CtyDb>>,
) -> anyhow::Result<Box<dyn ContestScorer>> {
    let contest_id = contest.contest_id();
    let contest_instance_id = contest.contest_instance_id();

    // Try spec-based scorer; fall back for contests without an embedded spec.
    // spec_by_id() is a compile-time embedded lookup — no filesystem access,
    // so the release binary is self-contained. A failed `SpecScorer::new`
    // (e.g. contest.toml missing a required `my_is_<state>` field) propagates
    // out so `bootstrap` can refuse to start instead of silently dropping
    // every QSO into a dead session.
    if let Some(spec) = contest_engine::spec::embedded::spec_by_id(contest_id) {
        // Translate the contest's `.ch`-column → form-field-id mapping
        // into `.ch`-column → contest-engine field-id (string) pairs,
        // which the scorer uses when synthesizing hypothetical exchanges
        // for bandmap mult classification. Form field-ids >= 2 index into
        // the contest-engine received-exchange variant's field list.
        let engine_fields: Vec<String> = spec
            .exchange
            .received_variants
            .first()
            .map(|v| v.fields.iter().map(|f| f.id.clone()).collect())
            .unwrap_or_default();
        let history_mapping: Vec<(String, String)> = contest
            .history_field_mapping()
            .into_iter()
            .filter_map(|(col, field_id)| {
                let idx = field_id.checked_sub(2)? as usize;
                engine_fields
                    .get(idx)
                    .cloned()
                    .map(|engine_id| (col.to_string(), engine_id))
            })
            .collect();
        Ok(Box::new(spec_scorer::SpecScorer::new(
            contest_id,
            contest_instance_id,
            config,
            call_history,
            history_mapping,
            cty,
        )?))
    } else {
        Ok(Box::new(unique_call::UniqueCallScorer::new()))
    }
}

pub const BAND_LABELS: &[&str] = &["160m", "80m", "40m", "20m", "15m", "10m"];

pub(crate) fn band_label_from_qsolog(b: Band) -> String {
    match b {
        Band::B160m => "160m",
        Band::B80m => "80m",
        Band::B40m => "40m",
        Band::B20m => "20m",
        Band::B15m => "15m",
        Band::B10m => "10m",
        _ => "other",
    }
    .to_string()
}
