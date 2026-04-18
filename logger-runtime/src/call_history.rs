use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, bail};
use logger_core::CallHistoryLookup;

/// In-memory call history database parsed from N1MM `.ch` files.
pub struct CallHistoryDb {
    records: HashMap<String, HashMap<String, String>>,
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
                // Header line: "!!Order!!,Call,Name,Exch1,..."
                // Strip the sentinel and parse the rest as CSV.
                let body = trimmed.strip_prefix("!!Order!!").unwrap().trim_start_matches(',');
                columns = parse_csv_line(body)?
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                continue;
            }

            if columns.is_empty() {
                bail!("data line before !!Order!! header");
            }

            let fields = parse_csv_line(trimmed)?;
            let mut record: HashMap<String, String> = HashMap::new();
            let mut call = String::new();

            for (i, col) in columns.iter().enumerate() {
                let value = fields.get(i).map(|s| s.trim()).unwrap_or("").to_string();
                if col.eq_ignore_ascii_case("Call") {
                    call = value.to_ascii_uppercase();
                } else if !value.is_empty() {
                    record.insert(col.clone(), value);
                }
            }

            if !call.is_empty() {
                // Merge per-column so a later record doesn't wipe useful
                // columns from an earlier one. N1MM call history files for
                // state QSO parties commonly list an in-state station in the
                // "counties" section (Exch1=county) and again in the
                // "out-of-state" section (Exch1 empty or a state abbrev).
                // Last non-empty value wins per column.
                records.entry(call).or_default().extend(record);
            }
        }

        Ok(Self { records })
    }
}

/// Parse a single CSV line (RFC 4180 quoting), returning the field values.
/// Uses the `csv` crate so quoted fields containing commas are handled correctly.
fn parse_csv_line(line: &str) -> Result<Vec<String>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(line.as_bytes());
    let mut iter = reader.records();
    match iter.next() {
        Some(record) => {
            let record = record.context("parsing CSV line")?;
            Ok(record.iter().map(|s| s.to_string()).collect())
        }
        None => Ok(Vec::new()),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CH: &str = "\
# Call history file
!!Order!!,Call,Name,CqZone,Exch1
K1ABC,CHAD,5,1234
W2XYZ,BOB,3,5678
K1ABD,ALICE,5,9999
DL1ABC,HANS,14,100
";

    #[test]
    fn parse_basic() {
        let db = CallHistoryDb::parse(SAMPLE_CH).unwrap();
        assert_eq!(db.records.len(), 4);
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
    fn trailing_comma_in_header() {
        let content = "\
!!Order!!,Call,Name,Exch1,UserText,
K1ABC,CHAD,1234,Some State
";
        let db = CallHistoryDb::parse(content).unwrap();
        let pairs = db.lookup("K1ABC").unwrap();
        let map: HashMap<&str, &str> =
            pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("Name"), Some(&"CHAD"));
        assert_eq!(map.get("Exch1"), Some(&"1234"));
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let content = "\
# comment
!!Order!!,Call,Name

# another comment
K1ABC,BOB
";
        let db = CallHistoryDb::parse(content).unwrap();
        assert_eq!(db.records.len(), 1);
    }

    #[test]
    fn duplicate_call_merges_columns() {
        // N1MM QSO-party files often list a station twice: once in the
        // in-state section with a county, once in the out-of-state section
        // with a first name. Both pieces of data should survive.
        let content = "\
!!Order!!,Call,Name,Exch1,UserText,
NA8V,,STCL,
NA8V,GREG,,
";
        let db = CallHistoryDb::parse(content).unwrap();
        let pairs = db.lookup("NA8V").unwrap();
        let map: HashMap<&str, &str> =
            pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("Exch1"), Some(&"STCL"));
        assert_eq!(map.get("Name"), Some(&"GREG"));
    }

    #[test]
    fn quoted_field_with_comma_preserves_alignment() {
        // "Smith, John" contains a comma inside quotes; without quote-aware
        // parsing this would be split into two fields and shift the rest.
        let content = "\
!!Order!!,Call,Name,Exch1
K1ABC,\"Smith, John\",1234
W2XYZ,Bob,5678
";
        let db = CallHistoryDb::parse(content).unwrap();
        assert_eq!(db.records.len(), 2);

        let k1abc = db.lookup("K1ABC").unwrap();
        let map: HashMap<&str, &str> =
            k1abc.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("Name"), Some(&"Smith, John"));
        assert_eq!(map.get("Exch1"), Some(&"1234"));

        // Verify the next row wasn't shifted by the embedded comma
        let w2xyz = db.lookup("W2XYZ").unwrap();
        let map: HashMap<&str, &str> =
            w2xyz.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("Name"), Some(&"Bob"));
        assert_eq!(map.get("Exch1"), Some(&"5678"));
    }
}
