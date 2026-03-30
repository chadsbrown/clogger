use std::path::Path;

use anyhow::Result;
use logger_core::ScpLookup;
use station_data::SuperCheck;

/// SCP (Super Check Partial) database backed by the `station-data` crate.
pub struct ScpDb {
    inner: station_data::ScpDb,
}

impl ScpDb {
    pub fn load(path: &Path) -> Result<Self> {
        let inner = station_data::ScpDb::from_path(path)?;
        Ok(Self { inner })
    }
}

impl ScpLookup for ScpDb {
    fn partial_matches(&self, prefix: &str, limit: usize) -> Vec<String> {
        self.inner
            .suggest(prefix, limit)
            .into_iter()
            .map(|s| s.call)
            .collect()
    }
}
