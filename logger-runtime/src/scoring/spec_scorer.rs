use std::collections::{HashMap, HashSet};

use contest_engine::spec::{
    InMemoryDomainProvider, InMemoryResolver, Mode as CeMode, ResolvedStation, SpecSession, Value,
    embedded,
};
use contest_engine::types::{Band as CeBand, Callsign, Continent};
use qsolog::qso::QsoRecord;
use qsolog::types::{Band, Mode};

use super::{
    BandScore, BreakdownRow, ContestScorer, ScoreBreakdown, ScoreSummary, BAND_LABELS,
    band_label_from_qsolog,
};
use crate::log_adapter::decode_exchange_pairs;

pub struct SpecScorer {
    spec_id: String,
    contest_instance_id: u64,
    config: HashMap<String, Value>,
    /// Persistent session — kept alive across inserts so we don't rebuild
    /// the engine from scratch on every QSO.
    session: Option<SpecSession<InMemoryResolver, InMemoryDomainProvider>>,
    /// Calls already inserted into the resolver, so we skip duplicate inserts.
    resolved_calls: HashSet<String>,
    /// Per-band QSO and mult counters, updated incrementally from ApplySummary.
    qsos_by_band: HashMap<String, u32>,
    mults_by_band: HashMap<String, u32>,
    /// Per-band QSO points, for breakdown.
    points_by_band: HashMap<String, i64>,
    /// Per-band, per-mult-type counts: band -> (mult_type -> count).
    mults_by_type_by_band: HashMap<String, HashMap<String, u32>>,
    /// Ordered multiplier type IDs from the contest spec (e.g. ["zone", "country"]).
    mult_type_ids: Vec<String>,
    cached_summary: ScoreSummary,
}

impl SpecScorer {
    pub fn new(
        spec_id: impl Into<String>,
        contest_instance_id: u64,
        config: HashMap<String, Value>,
    ) -> Self {
        let mut scorer = Self {
            spec_id: spec_id.into(),
            contest_instance_id,
            config,
            session: None,
            resolved_calls: HashSet::new(),
            qsos_by_band: HashMap::new(),
            mults_by_band: HashMap::new(),
            points_by_band: HashMap::new(),
            mults_by_type_by_band: HashMap::new(),
            mult_type_ids: Vec::new(),
            cached_summary: ScoreSummary::default(),
        };
        scorer.init_session();
        scorer
    }

    /// Build a fresh empty session. Called at construction and by rebuild.
    fn init_session(&mut self) {
        let spec = match embedded::spec_by_id(&self.spec_id) {
            Some(s) => s,
            None => return,
        };
        let domains = embedded::standard_domain_pack();
        let resolver = InMemoryResolver::new();
        let source = ResolvedStation::new("W", Continent::NA, true, true);

        match SpecSession::new(spec, source, self.config.clone(), resolver, domains) {
            Ok(session) => {
                self.mult_type_ids = session
                    .engine()
                    .multiplier_ids()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                self.session = Some(session);
                self.resolved_calls.clear();
            }
            Err(e) => {
                tracing::warn!("SpecScorer: session init failed for {}: {e}", self.spec_id);
                self.session = None;
            }
        }
    }

    /// Ensure a callsign is in the resolver. Uses resolver_mut() to insert
    /// on first sight of each call.
    fn ensure_resolved(&mut self, call: &str) {
        let call_upper = call.trim().to_ascii_uppercase();
        if self.resolved_calls.contains(&call_upper) {
            return;
        }
        if let Some(session) = &mut self.session {
            session
                .resolver_mut()
                .insert(&call_upper, resolved_station_for_call(&call_upper));
            self.resolved_calls.insert(call_upper);
        }
    }

    /// Apply one QSO to the session and update all incremental counters.
    /// Returns true if the session accepted the QSO.
    fn apply_one(&mut self, rec: &QsoRecord) -> bool {
        let raw_exchange = match raw_exchange_for_record(rec) {
            Some(e) => e,
            None => return false,
        };

        self.ensure_resolved(&rec.callsign_norm);

        let session = match &mut self.session {
            Some(s) => s,
            None => return false,
        };

        match session.apply_qso_with_mode(
            to_ce_band_from_qsolog(rec.band),
            to_ce_mode_from_qsolog(rec.mode),
            Callsign::new(&rec.callsign_norm),
            &raw_exchange,
        ) {
            Ok(summary) => {
                let band_label = band_label_from_qsolog(rec.band);
                if !summary.is_dupe {
                    *self.qsos_by_band.entry(band_label.clone()).or_default() += 1;
                    *self.points_by_band.entry(band_label.clone()).or_default() +=
                        summary.qso_points;
                }
                for mult_str in &summary.new_mults {
                    let mult_type = mult_type_from_str(mult_str);
                    *self
                        .mults_by_type_by_band
                        .entry(band_label.clone())
                        .or_default()
                        .entry(mult_type)
                        .or_default() += 1;
                }
                let new_mult_count = summary.new_mults.len() as u32;
                if new_mult_count > 0 {
                    *self.mults_by_band.entry(band_label).or_default() += new_mult_count;
                }
                true
            }
            Err(_) => false,
        }
    }

    fn rebuild_summary_from_session(&mut self) {
        let by_band: Vec<(String, BandScore)> = BAND_LABELS
            .iter()
            .map(|label| {
                let qsos = self.qsos_by_band.get(*label).copied().unwrap_or(0);
                let mults = self.mults_by_band.get(*label).copied().unwrap_or(0);
                (label.to_string(), BandScore { qsos, mults })
            })
            .collect();

        let total_qsos = by_band.iter().map(|(_, bs)| bs.qsos).sum();
        let total_mults = by_band.iter().map(|(_, bs)| bs.mults).sum();
        let claimed_score = self
            .session
            .as_ref()
            .map(|s| s.engine().claimed_score())
            .unwrap_or(0);

        self.cached_summary = ScoreSummary {
            by_band,
            total_qsos,
            total_mults,
            claimed_score,
        };
    }
}

impl ContestScorer for SpecScorer {
    fn on_inserted(&mut self, record: &QsoRecord) {
        if record.flags.is_void || record.contest_instance_id != self.contest_instance_id {
            return;
        }
        self.apply_one(record);
        self.rebuild_summary_from_session();
    }

    fn rebuild(&mut self, records: &[QsoRecord]) {
        self.qsos_by_band.clear();
        self.mults_by_band.clear();
        self.points_by_band.clear();
        self.mults_by_type_by_band.clear();
        self.init_session();

        let cid = self.contest_instance_id;
        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == cid)
        {
            self.apply_one(rec);
        }

        self.rebuild_summary_from_session();
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

    fn score_breakdown(&self) -> ScoreBreakdown {
        let mut rows: Vec<BreakdownRow> = BAND_LABELS
            .iter()
            .filter_map(|label| {
                let qsos = self.qsos_by_band.get(*label).copied().unwrap_or(0);
                if qsos == 0 {
                    return None;
                }
                let points = self.points_by_band.get(*label).copied().unwrap_or(0);
                let type_map = self.mults_by_type_by_band.get(*label);
                let mults: Vec<(String, u32)> = self
                    .mult_type_ids
                    .iter()
                    .map(|mt| {
                        let count = type_map
                            .and_then(|m| m.get(mt))
                            .copied()
                            .unwrap_or(0);
                        (mt.clone(), count)
                    })
                    .collect();
                Some(BreakdownRow {
                    band: label.to_ascii_uppercase(),
                    mode: "CW".to_string(),
                    qsos,
                    points,
                    mults,
                })
            })
            .collect();

        // Total row
        let total_points: i64 = self.points_by_band.values().sum();
        let mut total_mults_by_type: HashMap<String, u32> = HashMap::new();
        for type_map in self.mults_by_type_by_band.values() {
            for (mt, count) in type_map {
                *total_mults_by_type.entry(mt.clone()).or_default() += count;
            }
        }
        let total_mults: Vec<(String, u32)> = self
            .mult_type_ids
            .iter()
            .map(|mt| {
                let count = total_mults_by_type.get(mt).copied().unwrap_or(0);
                (mt.clone(), count)
            })
            .collect();

        rows.push(BreakdownRow {
            band: "total".to_string(),
            mode: "ALL".to_string(),
            qsos: self.cached_summary.total_qsos,
            points: total_points,
            mults: total_mults,
        });

        ScoreBreakdown {
            rows,
            claimed_score: self.cached_summary.claimed_score,
        }
    }

    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        let session = match &self.session {
            Some(s) => s,
            None => return false,
        };

        let call_upper = call_norm.trim().to_ascii_uppercase();
        if !self.resolved_calls.contains(&call_upper) {
            // Unknown call — never logged, so not a dupe.
            return false;
        }

        session
            .classify_call_lite_with_mode(
                to_ce_band(band),
                to_ce_mode(mode),
                Callsign::new(call_norm),
            )
            .map(|c| c.is_dupe)
            .unwrap_or(false)
    }

    fn would_be_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        let session = match &self.session {
            Some(s) => s,
            None => return false,
        };

        let call_upper = call_norm.trim().to_ascii_uppercase();
        if !self.resolved_calls.contains(&call_upper) {
            // Unknown call — can't resolve without mutation. Since we haven't
            // seen it in the log, it's definitionally not a dupe and very
            // likely a new mult. Return true so the UI highlights it.
            return true;
        }

        session
            .classify_call_lite_with_mode(
                to_ce_band(band),
                to_ce_mode(mode),
                Callsign::new(call_norm),
            )
            .map(|c| !c.new_mults.is_empty())
            .unwrap_or(false)
    }
}

/// Extract the multiplier type from a "type:value" string.
/// e.g. "COUNTRY:W" → "country", "ZONE:5" → "zone".
fn mult_type_from_str(s: &str) -> String {
    s.split_once(':')
        .map(|(t, _)| t.to_ascii_lowercase())
        .unwrap_or_else(|| s.to_ascii_lowercase())
}

fn raw_exchange_for_record(rec: &QsoRecord) -> Option<String> {
    let pairs = decode_exchange_pairs(&rec.exchange).ok()?;
    if pairs.is_empty() {
        return None;
    }
    // Exclude the sent serial — it's appended by ESM but is not part of the
    // received exchange that the contest-engine scores against.
    Some(
        pairs
            .into_iter()
            .filter(|(k, _)| k != "serial")
            .map(|(_, v)| v)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn resolved_station_for_call(call: &str) -> ResolvedStation {
    let upper = call.trim().to_ascii_uppercase();
    if upper.starts_with("DL") {
        return ResolvedStation::new("DL", Continent::EU, false, false);
    }
    if upper.starts_with("JA") {
        return ResolvedStation::new("JA", Continent::AS, false, false);
    }
    if upper.starts_with("VE") {
        return ResolvedStation::new("VE", Continent::NA, true, true);
    }
    if upper.starts_with('K')
        || upper.starts_with('W')
        || upper.starts_with('N')
        || upper.starts_with('A')
    {
        return ResolvedStation::new("W", Continent::NA, true, true);
    }

    ResolvedStation::new("W", Continent::NA, true, true)
}

fn to_ce_band(s: &str) -> CeBand {
    match s.to_ascii_lowercase().as_str() {
        "160m" => CeBand::B160,
        "80m" => CeBand::B80,
        "40m" => CeBand::B40,
        "20m" => CeBand::B20,
        "15m" => CeBand::B15,
        _ => CeBand::B10,
    }
}

fn to_ce_mode(s: &str) -> CeMode {
    match s.to_ascii_uppercase().as_str() {
        "SSB" => CeMode::SSB,
        _ => CeMode::CW,
    }
}

fn to_ce_band_from_qsolog(b: Band) -> CeBand {
    match b {
        Band::B160m => CeBand::B160,
        Band::B80m => CeBand::B80,
        Band::B40m => CeBand::B40,
        Band::B20m => CeBand::B20,
        Band::B15m => CeBand::B15,
        _ => CeBand::B10,
    }
}

fn to_ce_mode_from_qsolog(m: Mode) -> CeMode {
    match m {
        Mode::SSB => CeMode::SSB,
        _ => CeMode::CW,
    }
}
