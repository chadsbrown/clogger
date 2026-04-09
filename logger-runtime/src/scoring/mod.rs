pub mod mst;
mod spec_scorer;
pub mod sweeps;
pub mod unique_call;

use std::collections::HashMap;

use contest_engine::spec::Value;
use logger_core::ContestEntry;
use qsolog::qso::QsoRecord;
use qsolog::types::Band;

pub struct BandScore {
    pub qsos: u32,
    pub mults: u32,
}

pub struct ScoreSummary {
    pub by_band: Vec<(String, BandScore)>,
    pub total_qsos: u32,
    pub total_mults: u32,
    pub claimed_score: i64,
}

impl Default for ScoreSummary {
    fn default() -> Self {
        Self {
            by_band: Vec::new(),
            total_qsos: 0,
            total_mults: 0,
            claimed_score: 0,
        }
    }
}

/// A single row in the score breakdown, keyed by (band, mode).
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
pub struct ScoreBreakdown {
    pub rows: Vec<BreakdownRow>,
    pub claimed_score: i64,
}

impl Default for ScoreBreakdown {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            claimed_score: 0,
        }
    }
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

    /// Would this candidate be a new mult right now? Must not mutate.
    fn would_be_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool;
}

pub fn scorer_for_contest(
    contest: &dyn ContestEntry,
    my_zone: u8,
    my_exchange: &HashMap<String, String>,
) -> Box<dyn ContestScorer> {
    let contest_id = contest.contest_id();
    let contest_instance_id = contest.contest_instance_id();

    // Build contest-engine config from my_zone + my_exchange
    let mut config: HashMap<String, Value> = HashMap::new();
    config.insert(
        "my_cq_zone".to_string(),
        Value::Int(i64::from(my_zone)),
    );
    for (k, v) in my_exchange {
        config.insert(format!("my_{}", k.to_ascii_lowercase()), Value::Text(v.clone()));
    }

    // Try spec-based scorer; fall back for contests without an embedded spec.
    // spec_by_id() is a compile-time embedded lookup — no filesystem access,
    // so the release binary is self-contained.
    if contest_engine::spec::embedded::spec_by_id(contest_id).is_some() {
        Box::new(spec_scorer::SpecScorer::new(contest_id, contest_instance_id, config))
    } else if contest_id == "sweeps" {
        Box::new(sweeps::SweepsScorer::new(contest_instance_id))
    } else {
        Box::new(unique_call::UniqueCallScorer::new())
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
