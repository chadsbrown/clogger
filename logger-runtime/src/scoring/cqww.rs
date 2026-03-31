use std::collections::HashMap;

use contest_engine::spec::{
    ContestSpec, InMemoryDomainProvider, InMemoryResolver, Mode as CeMode, ResolvedStation,
    SpecSession, Value, domain_packs,
};
use contest_engine::types::{Band as CeBand, Callsign, Continent};
use qsolog::qso::QsoRecord;
use qsolog::types::{Band, Mode};

use super::{BandScore, ContestScorer, ScoreSummary, count_qsos_by_band};
use crate::log_adapter::decode_exchange_pairs;

const BANDS: &[(CeBand, &str)] = &[
    (CeBand::B160, "160m"),
    (CeBand::B80, "80m"),
    (CeBand::B40, "40m"),
    (CeBand::B20, "20m"),
    (CeBand::B15, "15m"),
    (CeBand::B10, "10m"),
];

pub struct CqwwScorer {
    my_zone: u8,
}

impl CqwwScorer {
    pub fn new(my_zone: u8) -> Self {
        Self { my_zone }
    }

    fn build_session(
        &self,
        records: &[QsoRecord],
    ) -> anyhow::Result<SpecSession<InMemoryResolver, InMemoryDomainProvider>> {
        let spec_path = format!(
            "{}/../../contest-engine/specs/cqww.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let domain_path = format!(
            "{}/../../contest-engine/specs/domains",
            env!("CARGO_MANIFEST_DIR")
        );
        let spec = ContestSpec::from_path(spec_path)
            .map_err(|e| anyhow::anyhow!("load cqww spec: {e}"))?;
        let domains =
            domain_packs::load_standard_domain_pack(domain_path).map_err(|e| anyhow::anyhow!(e))?;

        let mut resolver = InMemoryResolver::new();
        for rec in records {
            resolver.insert(
                &rec.callsign_norm,
                resolved_station_for_call(&rec.callsign_norm),
            );
        }

        let source = ResolvedStation::new("W", Continent::NA, true, true);
        let mut config: HashMap<String, Value> = HashMap::new();
        config.insert(
            "my_cq_zone".to_string(),
            Value::Int(i64::from(self.my_zone)),
        );

        SpecSession::new(spec, source, config, resolver, domains)
            .map_err(|e| anyhow::anyhow!("session: {e:?}"))
    }

    fn build_session_with_log(
        &self,
        records: &[QsoRecord],
    ) -> anyhow::Result<SpecSession<InMemoryResolver, InMemoryDomainProvider>> {
        let mut session = self.build_session(records)?;
        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == 1)
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
        Ok(session)
    }
}

impl ContestScorer for CqwwScorer {
    fn is_new_mult(&self, records: &[QsoRecord], call_norm: &str, band: &str, mode: &str) -> bool {
        let ce_band = to_ce_band(band);
        let ce_mode = to_ce_mode(mode);
        let session = match self.build_session_with_log(records) {
            Ok(s) => s,
            Err(_) => return false,
        };

        session
            .classify_call_lite_with_mode(ce_band, ce_mode, Callsign::new(call_norm))
            .map(|c| !c.new_mults.is_empty())
            .unwrap_or(false)
    }

    fn score_summary(&self, records: &[QsoRecord]) -> ScoreSummary {
        let qsos_by_band = count_qsos_by_band(records);

        let session = match self.build_session_with_log(records) {
            Ok(s) => s,
            Err(_) => return ScoreSummary::default(),
        };

        let mult_ids = session.engine().multiplier_ids();
        let by_band: Vec<(String, BandScore)> = BANDS
            .iter()
            .map(|(ce_band, label)| {
                let qsos = qsos_by_band.get(*label).copied().unwrap_or(0);
                let mults: u32 = mult_ids
                    .iter()
                    .map(|mid| session.worked_mults(mid, Some(*ce_band)).len() as u32)
                    .sum();
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
