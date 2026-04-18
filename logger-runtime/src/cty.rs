use std::path::Path;

use anyhow::Result;

/// cty.dat-backed DXCC/entity database, re-exported from `station-data`.
///
/// Kept as a thin wrapper so consumers depend on `logger_runtime::CtyDb`
/// rather than reaching into `station_data` directly — same pattern as
/// `ScpDb`. The underlying `station_data::CtyDb` is what dxfeed's entity
/// resolver adapter (`dxfeed_entity::CtyEntityResolver`) consults.
pub struct CtyDb {
    pub(crate) inner: station_data::CtyDb,
}

impl CtyDb {
    pub fn load(path: &Path) -> Result<Self> {
        let inner = station_data::CtyDb::from_path(path)?;
        Ok(Self { inner })
    }
}
