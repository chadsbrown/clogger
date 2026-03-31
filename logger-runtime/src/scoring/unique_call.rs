use std::collections::{HashMap, HashSet};

use qsolog::qso::QsoRecord;

use super::{BandScore, ContestScorer, ScoreSummary, BAND_LABELS, band_label_from_qsolog};

pub struct UniqueCallScorer;

impl ContestScorer for UniqueCallScorer {
    fn is_new_mult(&self, records: &[QsoRecord], call_norm: &str, _band: &str, _mode: &str) -> bool {
        // New mult if callsign not yet worked on any band
        !records
            .iter()
            .any(|q| !q.flags.is_void && q.callsign_norm == call_norm)
    }

    fn score_summary(&self, records: &[QsoRecord]) -> ScoreSummary {
        // Dedupe QSOs by (call, band), attribute each new-call mult to first-worked band.
        let mut seen_qsos: HashSet<(String, String)> = HashSet::new();
        let mut seen_calls: HashSet<String> = HashSet::new();
        let mut qsos_by_band: HashMap<String, u32> = HashMap::new();
        let mut mults_by_band: HashMap<String, u32> = HashMap::new();

        for rec in records.iter().filter(|r| !r.flags.is_void) {
            let band_label = band_label_from_qsolog(rec.band);
            if seen_qsos.insert((rec.callsign_norm.clone(), band_label.clone())) {
                *qsos_by_band.entry(band_label.clone()).or_default() += 1;
                if seen_calls.insert(rec.callsign_norm.clone()) {
                    *mults_by_band.entry(band_label).or_default() += 1;
                }
            }
        }

        let by_band: Vec<(String, BandScore)> = BAND_LABELS
            .iter()
            .map(|label| {
                let qsos = qsos_by_band.get(*label).copied().unwrap_or(0);
                let mults = mults_by_band.get(*label).copied().unwrap_or(0);
                (label.to_string(), BandScore { qsos, mults })
            })
            .collect();
        let total_qsos = by_band.iter().map(|(_, bs)| bs.qsos).sum();
        let total_mults = by_band.iter().map(|(_, bs)| bs.mults).sum();
        let claimed_score = total_qsos as i64;

        ScoreSummary {
            by_band,
            total_qsos,
            total_mults,
            claimed_score,
        }
    }
}
