use std::path::Path;

use anyhow::Result;
use logger_core::ScpLookup;

/// In-memory SCP (Super Check Partial) database parsed from `.scp` files.
///
/// An `.scp` file is a plain-text list of active contester callsigns,
/// one uppercase call per line, no headers.
pub struct ScpDb {
    sorted_calls: Vec<String>,
}

impl ScpDb {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let mut sorted_calls: Vec<String> = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_ascii_uppercase())
            .collect();
        sorted_calls.sort();
        sorted_calls.dedup();
        Ok(Self { sorted_calls })
    }
}

impl ScpLookup for ScpDb {
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

    #[test]
    fn parse_basic() {
        let db = ScpDb::parse("K1ABC\nW2XYZ\nDL1ABC\n").unwrap();
        assert_eq!(db.sorted_calls.len(), 3);
        assert_eq!(db.sorted_calls[0], "DL1ABC");
    }

    #[test]
    fn parse_deduplicates_and_uppercases() {
        let db = ScpDb::parse("k1abc\nK1ABC\nw2xyz\n").unwrap();
        assert_eq!(db.sorted_calls.len(), 2);
    }

    #[test]
    fn parse_skips_empty_lines() {
        let db = ScpDb::parse("\n\nK1ABC\n\n").unwrap();
        assert_eq!(db.sorted_calls.len(), 1);
    }

    #[test]
    fn prefix_matches() {
        let db = ScpDb::parse("K1ABC\nK1ABD\nK2ABC\nW2XYZ\n").unwrap();
        let matches = db.partial_matches("K1AB", 10);
        assert_eq!(matches, vec!["K1ABC", "K1ABD"]);
    }

    #[test]
    fn prefix_matches_limit() {
        let db = ScpDb::parse("K1ABC\nK1ABD\nK1ABE\n").unwrap();
        let matches = db.partial_matches("K1AB", 2);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn empty_prefix_returns_empty() {
        let db = ScpDb::parse("K1ABC\nW2XYZ\n").unwrap();
        let matches = db.partial_matches("", 10);
        assert!(matches.is_empty());
    }

    #[test]
    fn comments_ignored() {
        let db = ScpDb::parse("# comment\nK1ABC\n# another\nW2XYZ\n").unwrap();
        assert_eq!(db.sorted_calls.len(), 2);
        assert!(!db.sorted_calls.iter().any(|c| c.starts_with('#')));
    }

    #[test]
    fn no_match_returns_empty() {
        let db = ScpDb::parse("K1ABC\nW2XYZ\n").unwrap();
        let matches = db.partial_matches("VE3", 10);
        assert!(matches.is_empty());
    }
}
