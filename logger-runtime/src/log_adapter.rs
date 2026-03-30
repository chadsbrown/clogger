use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use contest_engine::spec::{
    ContestSpec, InMemoryDomainProvider, InMemoryResolver, Mode as CeMode, ResolvedStation,
    SpecSession, Value, domain_packs,
};
use contest_engine::types::{Band as CeBand, Callsign, Continent};
use logger_core::{DupeChecker, MultChecker, QsoDraft};
use qsolog::{
    core::store::QsoStore,
    persist::{OpSink, sqlite::SqliteOpSink},
    qso::{ExchangeBlob, QsoDraft as StoreDraft, QsoFlags, QsoRecord},
    types::{Band, Mode, QsoId},
};
use tracing::info;

pub struct LogAdapter {
    store: QsoStore,
    sink: Option<SqliteOpSink>,
    contest_id: String,
    my_zone: u8,
}

impl LogAdapter {
    pub fn new(contest_id: impl Into<String>, my_zone: u8) -> Self {
        Self {
            store: QsoStore::new(),
            sink: None,
            contest_id: contest_id.into(),
            my_zone,
        }
    }

    pub fn open_db(contest_id: impl Into<String>, my_zone: u8, path: &Path) -> Result<Self> {
        let sink = SqliteOpSink::open(path).map_err(|e| anyhow::anyhow!("open db: {e:?}"))?;
        let store = sink
            .load_store()
            .map_err(|e| anyhow::anyhow!("load store: {e:?}"))?;
        let count = store.ordered_ids().len();
        info!("loaded {count} QSOs from {}", path.display());
        Ok(Self {
            store,
            sink: Some(sink),
            contest_id: contest_id.into(),
            my_zone,
        })
    }

    pub fn insert(
        &mut self,
        draft: QsoDraft,
        ts_ms: u64,
        radio_id: u32,
        operator_id: u32,
    ) -> Result<QsoId> {
        let exchange = ExchangeBlob {
            bytes: encode_exchange_pairs(&draft.exchange_pairs)?,
        };

        let store_draft = StoreDraft {
            contest_instance_id: draft.exchange_schema_id as u64,
            callsign_raw: draft.callsign.clone(),
            callsign_norm: draft.callsign,
            band: to_band(&draft.band),
            mode: to_mode(&draft.mode),
            freq_hz: draft.freq_hz,
            ts_ms,
            radio_id,
            operator_id,
            exchange,
            flags: QsoFlags::default(),
        };

        let (id, _) = self
            .store
            .insert(store_draft)
            .map_err(|e| anyhow::anyhow!("insert failed: {e:?}"))?;

        // Persist to SQLite if available
        if let Some(sink) = &mut self.sink {
            let ops = self.store.drain_pending_ops();
            if !ops.is_empty() {
                sink.append_ops(&ops)
                    .map_err(|e| anyhow::anyhow!("persist failed: {e:?}"))?;
            }
        }

        Ok(id)
    }

    pub fn ordered_records(&self) -> Vec<QsoRecord> {
        self.store
            .ordered_ids()
            .iter()
            .filter_map(|id| self.store.get_cloned(*id))
            .collect()
    }

    pub fn undo(&mut self) -> Result<()> {
        self.store
            .undo()
            .map_err(|e| anyhow::anyhow!("undo failed: {e:?}"))?;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<()> {
        self.store
            .redo()
            .map_err(|e| anyhow::anyhow!("redo failed: {e:?}"))?;
        Ok(())
    }

    fn build_cqww_session(&self) -> Result<SpecSession<InMemoryResolver, InMemoryDomainProvider>> {
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
        for rec in self.ordered_records() {
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
}

impl DupeChecker for LogAdapter {
    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        let band = to_band(band);
        let mode = to_mode(mode);
        self.store
            .by_call(call_norm)
            .into_iter()
            .any(|q| !q.flags.is_void && q.band == band && q.mode == mode)
    }
}

impl MultChecker for LogAdapter {
    fn is_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        if self.contest_id != "cqww" {
            return false;
        }

        let ce_band = to_ce_band(band);
        let ce_mode = to_ce_mode(mode);
        let mut session = match self.build_cqww_session() {
            Ok(s) => s,
            Err(_) => return false,
        };

        for rec in self
            .ordered_records()
            .into_iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == 1)
        {
            if let Some(raw_exchange) = raw_exchange_for_record(&rec)
                && session
                    .apply_qso_with_mode(
                        to_ce_band_from_qsolog(rec.band),
                        to_ce_mode_from_qsolog(rec.mode),
                        Callsign::new(&rec.callsign_norm),
                        &raw_exchange,
                    )
                    .is_err()
            {
                // Ignore bad historical records for indicator-only queries.
            }
        }

        session
            .classify_call_lite_with_mode(ce_band, ce_mode, Callsign::new(call_norm))
            .map(|c| !c.new_mults.is_empty())
            .unwrap_or(false)
    }
}

pub fn decode_exchange_pairs(blob: &ExchangeBlob) -> Result<Vec<(String, String)>> {
    serde_json::from_slice(&blob.bytes).context("decode exchange bytes")
}

fn encode_exchange_pairs(pairs: &[(String, String)]) -> Result<Vec<u8>> {
    serde_json::to_vec(pairs).context("encode exchange bytes")
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

fn to_band(s: &str) -> Band {
    match s.to_ascii_lowercase().as_str() {
        "160m" => Band::B160m,
        "80m" => Band::B80m,
        "40m" => Band::B40m,
        "20m" => Band::B20m,
        "15m" => Band::B15m,
        "10m" => Band::B10m,
        _ => Band::Other,
    }
}

fn to_mode(s: &str) -> Mode {
    match s.to_ascii_uppercase().as_str() {
        "CW" => Mode::CW,
        "SSB" => Mode::SSB,
        "DIGITAL" => Mode::Digital,
        _ => Mode::Other,
    }
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

#[cfg(test)]
mod tests {
    use super::LogAdapter;

    #[test]
    fn undo_redo_placeholder_roundtrip() {
        let mut adapter = LogAdapter::new("cqww", 4);
        let draft = logger_core::QsoDraft {
            contest_id: "cqww".to_string(),
            callsign: "K1ABC".to_string(),
            band: "20m".to_string(),
            mode: "CW".to_string(),
            freq_hz: 14_025_000,
            exchange_schema_id: 1,
            exchange_pairs: vec![
                ("rst".to_string(), "599".to_string()),
                ("zone".to_string(), "5".to_string()),
            ],
        };

        adapter.insert(draft, 1, 1, 1).expect("insert");
        assert_eq!(adapter.ordered_records().len(), 1);
        adapter.undo().expect("undo");
        assert!(adapter.ordered_records()[0].flags.is_void);
        adapter.redo().expect("redo");
        assert!(!adapter.ordered_records()[0].flags.is_void);
        assert_eq!(adapter.ordered_records().len(), 1);
    }
}
