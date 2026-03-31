pub mod cqww;
pub mod unique_call;

use std::collections::HashMap;

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

pub fn scorer_for_contest(contest_id: &str, my_zone: u8) -> Box<dyn ContestScorer> {
    match contest_id {
        "cqww" => Box::new(cqww::CqwwScorer::new(my_zone)),
        _ => Box::new(unique_call::UniqueCallScorer),
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

pub(crate) fn count_qsos_by_band(records: &[QsoRecord]) -> HashMap<String, u32> {
    let mut map: HashMap<String, u32> = HashMap::new();
    for rec in records.iter().filter(|r| !r.flags.is_void) {
        *map.entry(band_label_from_qsolog(rec.band)).or_default() += 1;
    }
    map
}
