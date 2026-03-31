use std::collections::HashMap;

use contest_engine::spec::{
    ContestSpec, InMemoryDomainProvider, InMemoryResolver, Mode as CeMode, ResolvedStation,
    SpecSession, Value, domain_packs,
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
}

impl SpecScorer {
    pub fn new(
        spec_id: impl Into<String>,
        contest_instance_id: u64,
        config: HashMap<String, Value>,
    ) -> Self {
        Self {
            spec_id: spec_id.into(),
            contest_instance_id,
            config,
        }
    }

    fn build_session(
        &self,
        records: &[QsoRecord],
        extra_calls: &[&str],
    ) -> anyhow::Result<SpecSession<InMemoryResolver, InMemoryDomainProvider>> {
        let spec_path = format!(
            "{}/../../contest-engine/specs/{}.json",
            env!("CARGO_MANIFEST_DIR"),
            self.spec_id
        );
        let domain_path = format!(
            "{}/../../contest-engine/specs/domains",
            env!("CARGO_MANIFEST_DIR")
        );
        let spec = ContestSpec::from_path(spec_path)
            .map_err(|e| anyhow::anyhow!("load {} spec: {e}", self.spec_id))?;
        let domains =
            domain_packs::load_standard_domain_pack(domain_path).map_err(|e| anyhow::anyhow!(e))?;

        let mut resolver = InMemoryResolver::new();
        for rec in records {
            resolver.insert(
                &rec.callsign_norm,
                resolved_station_for_call(&rec.callsign_norm),
            );
        }
        for call in extra_calls {
            resolver.insert(call, resolved_station_for_call(call));
        }

        let source = ResolvedStation::new("W", Continent::NA, true, true);

        SpecSession::new(spec, source, self.config.clone(), resolver, domains)
            .map_err(|e| anyhow::anyhow!("session: {e:?}"))
    }
}

impl ContestScorer for SpecScorer {
    fn is_new_mult(&self, records: &[QsoRecord], call_norm: &str, band: &str, mode: &str) -> bool {
        let mut session = match self.build_session(records, &[call_norm]) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Replay log so the engine knows what's already worked
        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == self.contest_instance_id)
        {
            if let Some(raw_exchange) = raw_exchange_for_record(rec) {
                let _ = session.apply_qso_with_mode(
                    to_ce_band_from_qsolog(rec.band),
                    to_ce_mode_from_qsolog(rec.mode),
                    Callsign::new(&rec.callsign_norm),
                    &raw_exchange,
                );
            }
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

    fn score_summary(&self, records: &[QsoRecord]) -> ScoreSummary {
        let mut session = match self.build_session(records, &[]) {
            Ok(s) => s,
            Err(_) => return ScoreSummary::default(),
        };

        // Replay log, capturing ApplySummary to build per-band breakdown.
        let mut qsos_by_band: HashMap<String, u32> = HashMap::new();
        let mut mults_by_band: HashMap<String, u32> = HashMap::new();

        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == self.contest_instance_id)
        {
            if let Some(raw_exchange) = raw_exchange_for_record(rec) {
                if let Ok(summary) = session.apply_qso_with_mode(
                    to_ce_band_from_qsolog(rec.band),
                    to_ce_mode_from_qsolog(rec.mode),
                    Callsign::new(&rec.callsign_norm),
                    &raw_exchange,
                ) {
                    let band_label = band_label_from_qsolog(rec.band);
                    if !summary.is_dupe {
                        *qsos_by_band.entry(band_label.clone()).or_default() += 1;
                    }
                    let new_mult_count = summary.new_mults.len() as u32;
                    if new_mult_count > 0 {
                        *mults_by_band.entry(band_label).or_default() += new_mult_count;
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

        let total_qsos = by_band.iter().map(|(_, bs)| bs.qsos).sum();
        let total_mults = by_band.iter().map(|(_, bs)| bs.mults).sum();
        let claimed_score = session.engine().claimed_score();

        ScoreSummary {
            by_band,
            total_qsos,
            total_mults,
            claimed_score,
        }
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
