use std::{collections::HashMap, path::Path};

use anyhow::{Result, bail};
use logger_core::CallHistoryLookup;

/// In-memory call history database parsed from N1MM `.ch` files.
pub struct CallHistoryDb {
    records: HashMap<String, HashMap<String, String>>,
    sorted_calls: Vec<String>,
}

impl CallHistoryDb {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let mut columns: Vec<String> = Vec::new();
        let mut records: HashMap<String, HashMap<String, String>> = HashMap::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with("!!Order!!") {
                columns = trimmed.split(',').map(|s| s.trim().to_string()).collect();
                continue;
            }

            if columns.is_empty() {
                bail!("data line before !!Order!! header");
            }

            let fields: Vec<&str> = trimmed.split(',').collect();
            let mut record: HashMap<String, String> = HashMap::new();
            let mut call = String::new();

            for (i, col) in columns.iter().enumerate() {
                let value = fields.get(i).unwrap_or(&"").trim().to_string();
                if col.eq_ignore_ascii_case("Call") {
                    call = value.to_ascii_uppercase();
                } else if col != "!!Order!!" && !value.is_empty() {
                    record.insert(col.clone(), value);
                }
            }

            if !call.is_empty() {
                records.insert(call, record);
            }
        }

        let mut sorted_calls: Vec<String> = records.keys().cloned().collect();
        sorted_calls.sort();

        Ok(Self {
            records,
            sorted_calls,
        })
    }
}

impl CallHistoryLookup for CallHistoryDb {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
        self.records.get(call_norm).map(|rec| {
            rec.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
    }

    fn partial_matches(&self, prefix: &str, limit: usize) -> Vec<String> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let start = self.sorted_calls.partition_point(|c| c.as_str() < prefix);
        self.sorted_calls[start..]
            .iter()
            .take_while(|c| c.starts_with(prefix))
            .take(limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CH: &str = "\
# Call history file
!!Order!!,Call,Name,CqZone,Exch1
K1ABC,K1ABC,CHAD,5,1234
W2XYZ,W2XYZ,BOB,3,5678
K1ABD,K1ABD,ALICE,5,9999
DL1ABC,DL1ABC,HANS,14,100
";

    #[test]
    fn parse_basic() {
        let db = CallHistoryDb::parse(SAMPLE_CH).unwrap();
        assert_eq!(db.records.len(), 4);
        assert_eq!(db.sorted_calls.len(), 4);
    }

    #[test]
    fn exact_lookup_hit() {
        let db = CallHistoryDb::parse(SAMPLE_CH).unwrap();
        let pairs = db.lookup("K1ABC").unwrap();
        let map: HashMap<&str, &str> = pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("Name"), Some(&"CHAD"));
        assert_eq!(map.get("CqZone"), Some(&"5"));
        assert_eq!(map.get("Exch1"), Some(&"1234"));
    }

    #[test]
    fn exact_lookup_miss() {
        let db = CallHistoryDb::parse(SAMPLE_CH).unwrap();
        assert!(db.lookup("NOCALL").is_none());
    }

    #[test]
    fn prefix_matches() {
        let db = CallHistoryDb::parse(SAMPLE_CH).unwrap();
        let matches = db.partial_matches("K1AB", 10);
        assert_eq!(matches, vec!["K1ABC", "K1ABD"]);
    }

    #[test]
    fn prefix_matches_limit() {
        let db = CallHistoryDb::parse(SAMPLE_CH).unwrap();
        let matches = db.partial_matches("K1AB", 1);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let content = "\
# comment
!!Order!!,Call,Name

# another comment
K1ABC,K1ABC,BOB
";
        let db = CallHistoryDb::parse(content).unwrap();
        assert_eq!(db.records.len(), 1);
    }
}
