use anyhow::{Context, Result};
use logger_core::QsoDraft;
use qsolog::{
    core::store::QsoStore,
    qso::{ExchangeBlob, QsoDraft as StoreDraft, QsoFlags, QsoRecord},
    types::{Band, Mode, QsoId},
};

#[derive(Debug, Default)]
pub struct QsoLogAdapter {
    store: QsoStore,
}

impl QsoLogAdapter {
    pub fn new() -> Self {
        Self {
            store: QsoStore::new(),
        }
    }

    pub fn insert(&mut self, draft: QsoDraft, ts_ms: u64, radio_id: u32, operator_id: u32) -> Result<QsoId> {
        let exchange = ExchangeBlob {
            bytes: encode_exchange_pairs(&draft.exchange_pairs)?,
        };

        let store_draft = StoreDraft {
            contest_instance_id: contest_instance_id(&draft.contest_id),
            callsign_raw: draft.callsign.clone(),
            callsign_norm: draft.callsign,
            band: to_band(draft.band.as_str()),
            mode: to_mode(draft.mode.as_str()),
            freq_hz: draft.freq_hz,
            ts_ms,
            radio_id,
            operator_id,
            exchange,
            flags: QsoFlags::default(),
        };

        let (id, _) = self.store.insert(store_draft).map_err(|e| anyhow::anyhow!("insert failed: {e:?}"))?;
        Ok(id)
    }

    pub fn ordered_records(&self) -> Vec<QsoRecord> {
        self.store
            .ordered_ids()
            .iter()
            .filter_map(|id| self.store.get_cloned(*id))
            .collect()
    }

    #[allow(dead_code)]
    pub fn undo(&mut self) -> Result<()> {
        self.store.undo().map_err(|e| anyhow::anyhow!("undo failed: {e:?}"))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn redo(&mut self) -> Result<()> {
        self.store.redo().map_err(|e| anyhow::anyhow!("redo failed: {e:?}"))?;
        Ok(())
    }
}

pub fn decode_exchange_pairs(blob: &ExchangeBlob) -> Result<Vec<(String, String)>> {
    serde_json::from_slice(&blob.bytes).context("decode exchange bytes")
}

fn encode_exchange_pairs(pairs: &[(String, String)]) -> Result<Vec<u8>> {
    serde_json::to_vec(pairs).context("encode exchange bytes")
}

fn contest_instance_id(contest_id: &str) -> u64 {
    match contest_id {
        "sweeps" => 2,
        _ => 1,
    }
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
    use super::QsoLogAdapter;

    #[test]
    fn undo_redo_placeholder_roundtrip() {
        let mut adapter = QsoLogAdapter::new();
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
