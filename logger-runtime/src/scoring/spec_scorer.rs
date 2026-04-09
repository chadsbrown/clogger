use std::collections::{HashMap, HashSet};

use contest_engine::spec::{
    InMemoryDomainProvider, InMemoryResolver, Mode as CeMode, ResolvedStation, SpecSession, Value,
    embedded,
};
use contest_engine::types::{Band as CeBand, Callsign, Continent};
use qsolog::qso::QsoRecord;
use qsolog::types::{Band, Mode};

use super::{BandScore, ContestScorer, ScoreSummary, BAND_LABELS, band_label_from_qsolog};
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
                self.session = Some(session);
                self.resolved_calls.clear();
            }
            Err(_) => {
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
        if record.flags.is_void
            || record.contest_instance_id != self.contest_instance_id
        {
            return;
        }

        let raw_exchange = match raw_exchange_for_record(record) {
            Some(e) => e,
            None => return,
        };

        self.ensure_resolved(&record.callsign_norm);

        if let Some(session) = &mut self.session {
            if let Ok(summary) = session.apply_qso_with_mode(
                to_ce_band_from_qsolog(record.band),
                to_ce_mode_from_qsolog(record.mode),
                Callsign::new(&record.callsign_norm),
                &raw_exchange,
            ) {
                let band_label = band_label_from_qsolog(record.band);
                if !summary.is_dupe {
                    *self.qsos_by_band.entry(band_label.clone()).or_default() += 1;
                }
                let new_mult_count = summary.new_mults.len() as u32;
                if new_mult_count > 0 {
                    *self.mults_by_band.entry(band_label).or_default() += new_mult_count;
                }
            }
        }

        self.rebuild_summary_from_session();
    }

    fn rebuild(&mut self, records: &[QsoRecord]) {
        self.qsos_by_band.clear();
        self.mults_by_band.clear();
        self.init_session();

        let cid = self.contest_instance_id;
        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == cid)
        {
            let raw_exchange = match raw_exchange_for_record(rec) {
                Some(e) => e,
                None => continue,
            };

            self.ensure_resolved(&rec.callsign_norm);

            if let Some(session) = &mut self.session {
                if let Ok(summary) = session.apply_qso_with_mode(
                    to_ce_band_from_qsolog(rec.band),
                    to_ce_mode_from_qsolog(rec.mode),
                    Callsign::new(&rec.callsign_norm),
                    &raw_exchange,
                ) {
                    let band_label = band_label_from_qsolog(rec.band);
                    if !summary.is_dupe {
                        *self.qsos_by_band.entry(band_label.clone()).or_default() += 1;
                    }
                    let new_mult_count = summary.new_mults.len() as u32;
                    if new_mult_count > 0 {
                        *self.mults_by_band.entry(band_label).or_default() += new_mult_count;
                    }
                }
            }
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

    fn would_be_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        // classify_call_lite_with_mode is &self on SpecSession — no mutation.
        // But we need to ensure the call is in the resolver first. If it isn't,
        // we can't classify it without mutation. Check resolved_calls first;
        // if the call is unknown, we need a mutable borrow to insert it.
        //
        // Since would_be_new_mult takes &self, we handle the unresolved case
        // conservatively: if we haven't seen the call before, it can't be a
        // dupe, so it's likely a new mult (return true as a safe default that
        // just means the UI highlights it — the actual scoring at log time is
        // always correct).
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

fn raw_exchange_for_record(rec: &QsoRecord) -> Option<String> {
    let pairs = decode_exchange_pairs(&rec.exchange).ok()?;
    if pairs.is_empty() {
        return None;
    }
    Some(
        pairs
            .into_iter()
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
