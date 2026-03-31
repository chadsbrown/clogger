use std::path::Path;

use anyhow::{Context, Result};
use logger_core::{DupeChecker, MultChecker, QsoDraft};
use qsolog::{
    core::store::QsoStore,
    persist::{OpSink, sqlite::SqliteOpSink},
    qso::{ExchangeBlob, QsoDraft as StoreDraft, QsoFlags, QsoRecord},
    types::{Band, Mode, QsoId},
};
use tracing::info;

use crate::scoring::{ContestScorer, ScoreSummary};

pub struct LogAdapter {
    store: QsoStore,
    sink: Option<SqliteOpSink>,
    scorer: Box<dyn ContestScorer>,
}

impl LogAdapter {
    pub fn new(scorer: Box<dyn ContestScorer>) -> Self {
        Self {
            store: QsoStore::new(),
            sink: None,
            scorer,
        }
    }

    pub fn open_db(scorer: Box<dyn ContestScorer>, path: &Path) -> Result<Self> {
        let sink = SqliteOpSink::open(path).map_err(|e| anyhow::anyhow!("open db: {e:?}"))?;
        let store = sink
            .load_store()
            .map_err(|e| anyhow::anyhow!("load store: {e:?}"))?;
        let count = store.ordered_ids().len();
        info!("loaded {count} QSOs from {}", path.display());
        Ok(Self {
            store,
            sink: Some(sink),
            scorer,
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

    pub fn score_summary(&self) -> ScoreSummary {
        self.scorer.score_summary(&self.ordered_records())
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
        self.scorer
            .is_new_mult(&self.ordered_records(), call_norm, band, mode)
    }
}

pub fn decode_exchange_pairs(blob: &ExchangeBlob) -> Result<Vec<(String, String)>> {
    serde_json::from_slice(&blob.bytes).context("decode exchange bytes")
}

fn encode_exchange_pairs(pairs: &[(String, String)]) -> Result<Vec<u8>> {
    serde_json::to_vec(pairs).context("encode exchange bytes")
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

#[cfg(test)]
mod tests {
    use super::LogAdapter;
    use crate::scoring::scorer_for_contest;

    #[test]
    fn undo_redo_placeholder_roundtrip() {
        let scorer = scorer_for_contest("cqww", 4);
        let mut adapter = LogAdapter::new(scorer);
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
