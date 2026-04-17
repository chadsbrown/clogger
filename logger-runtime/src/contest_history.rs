use std::collections::HashMap;

use logger_core::ContestHistoryLookup;
use qsolog::qso::QsoRecord;

use crate::log_adapter::decode_exchange_pairs;

/// Field ids that must NOT be replayed from prior QSOs:
/// - `nr`: the contacted station's sequence number; they advance it
///   every QSO they make, so an old value is always wrong.
/// - `rst`: usually the mode default; carrying CW 599 into an SSB form
///   (or vice-versa) hurts more than it helps.
///
/// The operator's own sent `serial` is never a received-exchange field
/// and has no form slot, so it doesn't need listing here.
const BLOCKED_FIELDS: &[&str] = &["nr", "rst"];

/// In-memory index of the most-recently-logged received exchange for
/// each callsign in the current contest. Drives auto-populate on
/// re-work: when the operator types a known call, the form is seeded
/// with the values that station sent previously.
///
/// Keying is call-only, last-write-wins. State-QP rovers whose LOC
/// varies by band will mis-populate on re-work; the operator corrects
/// manually. See `plans/contest-history-autopopulate.md`.
#[derive(Default)]
pub struct ContestHistoryIndex {
    rows: HashMap<String, Vec<(String, String)>>,
}

impl ContestHistoryIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay a batch of records (e.g. at open or after undo/redo).
    /// Non-voided records only; last write wins per callsign.
    pub fn rebuild(&mut self, records: &[QsoRecord]) {
        self.rows.clear();
        for rec in records.iter().filter(|r| !r.flags.is_void) {
            let Ok(pairs) = decode_exchange_pairs(&rec.exchange) else {
                continue;
            };
            let filtered = filter_exchange(pairs);
            if filtered.is_empty() {
                continue;
            }
            self.rows.insert(rec.callsign_norm.to_ascii_uppercase(), filtered);
        }
    }

    /// Upsert one QSO. Called from `LogAdapter::insert` after the scorer
    /// has been updated.
    pub fn on_inserted(&mut self, call_norm: &str, exchange_pairs: &[(String, String)]) {
        let filtered = filter_exchange(exchange_pairs.to_vec());
        if filtered.is_empty() {
            return;
        }
        self.rows
            .insert(call_norm.to_ascii_uppercase(), filtered);
    }
}

impl ContestHistoryLookup for ContestHistoryIndex {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
        self.rows
            .get(&call_norm.to_ascii_uppercase())
            .cloned()
    }
}

fn filter_exchange(pairs: Vec<(String, String)>) -> Vec<(String, String)> {
    pairs
        .into_iter()
        .filter(|(k, v)| {
            !BLOCKED_FIELDS.contains(&k.as_str())
                && !v.trim().is_empty()
                // `serial` is the operator's *sent* serial; not a
                // received field, but it ends up in `exchange_pairs`
                // for audit. Skip it here defensively — it has no
                // target form field, so it's a no-op either way.
                && k != "serial"
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_drops_nr_and_rst() {
        let input = vec![
            ("rst".to_string(), "599".to_string()),
            ("nr".to_string(), "137".to_string()),
            ("name".to_string(), "TIM".to_string()),
            ("loc".to_string(), "NH".to_string()),
            ("serial".to_string(), "042".to_string()),
        ];
        let out = filter_exchange(input);
        let keys: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["name", "loc"]);
    }

    #[test]
    fn filter_drops_empty_values() {
        let input = vec![
            ("name".to_string(), "TIM".to_string()),
            ("loc".to_string(), "  ".to_string()),
        ];
        let out = filter_exchange(input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "name");
    }

    #[test]
    fn lookup_uppercases_key() {
        let mut idx = ContestHistoryIndex::new();
        idx.on_inserted("k1abc", &[("name".to_string(), "TIM".to_string())]);
        assert!(idx.lookup("K1ABC").is_some());
        assert!(idx.lookup("k1abc").is_some());
    }
}
