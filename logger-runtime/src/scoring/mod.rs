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

pub trait ContestScorer: Send + Sync {
    fn is_new_mult(&self, records: &[QsoRecord], call_norm: &str, band: &str, mode: &str) -> bool;
    fn score_summary(&self, records: &[QsoRecord]) -> ScoreSummary;
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

    // Try spec-based scorer; fall back for contests without a spec
    let spec_path = format!(
        "{}/../../contest-engine/specs/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        contest_id
    );
    if std::path::Path::new(&spec_path).exists() {
        Box::new(spec_scorer::SpecScorer::new(contest_id, contest_instance_id, config))
    } else if contest_id == "sweeps" {
        Box::new(sweeps::SweepsScorer)
    } else {
        Box::new(unique_call::UniqueCallScorer)
    }
}

pub(crate) const BAND_LABELS: &[&str] = &["160m", "80m", "40m", "20m", "15m", "10m"];

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
