use std::collections::{HashMap, HashSet};

use qsolog::qso::QsoRecord;

use super::{BandScore, ContestScorer, ScoreSummary, BAND_LABELS, band_label_from_qsolog};

pub struct UniqueCallScorer {
    seen_qsos: HashSet<(String, String)>,
    seen_calls: HashSet<String>,
    qsos_by_band: HashMap<String, u32>,
    mults_by_band: HashMap<String, u32>,
    cached_summary: ScoreSummary,
}

impl UniqueCallScorer {
    pub fn new() -> Self {
        Self {
            seen_qsos: HashSet::new(),
            seen_calls: HashSet::new(),
            qsos_by_band: HashMap::new(),
            mults_by_band: HashMap::new(),
            cached_summary: ScoreSummary::default(),
        }
    }

    fn apply_record(&mut self, rec: &QsoRecord) {
        if rec.flags.is_void {
            return;
        }
        let band_label = band_label_from_qsolog(rec.band);
        if self.seen_qsos.insert((rec.callsign_norm.clone(), band_label.clone())) {
            *self.qsos_by_band.entry(band_label.clone()).or_default() += 1;
            if self.seen_calls.insert(rec.callsign_norm.clone()) {
                *self.mults_by_band.entry(band_label).or_default() += 1;
            }
        }
    }

    fn rebuild_summary(&mut self) {
        let by_band: Vec<(String, BandScore)> = BAND_LABELS
            .iter()
            .map(|label| {
                let qsos = self.qsos_by_band.get(*label).copied().unwrap_or(0);
                let mults = self.mults_by_band.get(*label).copied().unwrap_or(0);
                (label.to_string(), BandScore { qsos, mults })
            })
            .collect();
        let total_qsos: u32 = by_band.iter().map(|(_, bs)| bs.qsos).sum();
        let total_mults: u32 = by_band.iter().map(|(_, bs)| bs.mults).sum();
        let claimed_score = total_qsos as i64;

        self.cached_summary = ScoreSummary {
            by_band,
            total_qsos,
            total_mults,
            claimed_score,
        };
    }
}

impl ContestScorer for UniqueCallScorer {
    fn on_inserted(&mut self, record: &QsoRecord) {
        self.apply_record(record);
        self.rebuild_summary();
    }

    fn rebuild(&mut self, records: &[QsoRecord]) {
        self.seen_qsos.clear();
        self.seen_calls.clear();
        self.qsos_by_band.clear();
        self.mults_by_band.clear();
        for rec in records {
            self.apply_record(rec);
        }
        self.rebuild_summary();
    }

    fn score_summary(&self) -> ScoreSummary {
        ScoreSummary {
            by_band: self
                .cached_summary
                .by_band
                .iter()
                .map(|(label, bs)| {
                    (
                        label.clone(),
                        BandScore {
                            qsos: bs.qsos,
                            mults: bs.mults,
                        },
                    )
                })
                .collect(),
            total_qsos: self.cached_summary.total_qsos,
            total_mults: self.cached_summary.total_mults,
            claimed_score: self.cached_summary.claimed_score,
        }
    }

    fn would_be_new_mult(&self, call_norm: &str, _band: &str, _mode: &str) -> bool {
        !self.seen_calls.contains(call_norm)
    }
}
