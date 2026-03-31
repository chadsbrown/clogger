use std::collections::{HashMap, HashSet};

use qsolog::qso::QsoRecord;

use super::{BandScore, ContestScorer, ScoreSummary, BAND_LABELS, band_label_from_qsolog};
use crate::log_adapter::decode_exchange_pairs;

/// Sweeps scoring (no contest-engine spec available):
/// - 2 points per QSO
/// - Multipliers: unique ARRL/RAC sections, counted once globally
/// - Score = QSO points × mults
pub struct SweepsScorer;

impl ContestScorer for SweepsScorer {
    fn is_new_mult(&self, records: &[QsoRecord], _call_norm: &str, _band: &str, _mode: &str) -> bool {
        // Sweeps mults are sections, which aren't derivable from callsign alone.
        // The MultChecker trait only receives call/band/mode, so we can't determine
        // the section here. Return false — the mult indicator won't fire for Sweeps.
        let _ = records;
        false
    }

    fn score_summary(&self, records: &[QsoRecord]) -> ScoreSummary {
        // Dedupe QSOs by (call, band), track section mults on first-worked band.
        let mut seen_qsos: HashSet<(String, String)> = HashSet::new();
        let mut seen_sections: HashSet<String> = HashSet::new();
        let mut qsos_by_band: HashMap<String, u32> = HashMap::new();
        let mut mults_by_band: HashMap<String, u32> = HashMap::new();

        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == 2)
        {
            let band_label = band_label_from_qsolog(rec.band);
            if seen_qsos.insert((rec.callsign_norm.clone(), band_label.clone())) {
                *qsos_by_band.entry(band_label.clone()).or_default() += 1;
                if let Ok(pairs) = decode_exchange_pairs(&rec.exchange) {
                    if let Some((_, section)) = pairs.iter().find(|(k, _)| k == "section") {
                        let section_upper = section.to_ascii_uppercase();
                        if !section_upper.is_empty() && seen_sections.insert(section_upper) {
                            *mults_by_band.entry(band_label).or_default() += 1;
                        }
                    }
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
        let total_qsos: u32 = by_band.iter().map(|(_, bs)| bs.qsos).sum();
        let total_mults: u32 = by_band.iter().map(|(_, bs)| bs.mults).sum();
        // Sweeps: 2 points per QSO, score = points × mults
        let claimed_score = (total_qsos as i64 * 2) * total_mults as i64;

        ScoreSummary {
            by_band,
            total_qsos,
            total_mults,
            claimed_score,
        }
    }
}
