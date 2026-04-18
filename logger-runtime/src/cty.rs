use std::path::Path;

use anyhow::Result;
use contest_engine::spec::{ResolvedStation, StationResolver as _};
use contest_engine::types::Callsign;

/// cty.dat-backed DXCC/entity database, re-exported from `station-data`.
///
/// Kept as a thin wrapper so consumers depend on `logger_runtime::CtyDb`
/// rather than reaching into `station_data` directly — same pattern as
/// `ScpDb`. Consumed by `dxfeed_entity::CtyEntityResolver` (for the
/// dxfeed filter pipeline) and by the spec scorer (for DXCC/continent-
/// based mult classification — without this, every non-K/W/N/A/DL/JA/VE
/// call falls back to a US prefix guess).
pub struct CtyDb {
    pub(crate) inner: station_data::CtyDb,
}

impl CtyDb {
    pub fn load(path: &Path) -> Result<Self> {
        let inner = station_data::CtyDb::from_path(path)?;
        Ok(Self { inner })
    }

    /// Resolve a callsign to a contest-engine `ResolvedStation` (dxcc,
    /// continent, is_wve, is_na). Returns `None` when cty.dat has no
    /// matching prefix *or* when the continent code is unrecognized —
    /// the caller is expected to fall back to the hardcoded prefix
    /// table so scoring continues to work for unknown calls.
    pub fn resolve(&self, call: &str) -> Option<ResolvedStation> {
        let cs = Callsign::new(call);
        self.inner.resolve(&cs).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contest_engine::types::Continent;

    const MINI_CTY: &str = "United States of America:           05:  08:  NA:   37.53:   97.00:     5.0:  K:
    K,W,N,AA,AB,AC,AD,AE,AF,AG,AH,AI,AJ,AK,AL;
England:                            14:  27:  EU:   52.00:   -2.00:     0.0:  G:
    G,M,2E,GX,MX;
Japan:                              25:  45:  AS:   36.24: -139.00:    -9.0:  JA:
    JA,JE,JF,JG,JH,JI,JJ,JK,JL,JM,JN,JO,JP,JQ,JR,JS;
";

    fn load() -> CtyDb {
        let inner = station_data::CtyDb::from_reader(MINI_CTY.as_bytes()).unwrap();
        CtyDb { inner }
    }

    #[test]
    fn resolves_us_call_to_na() {
        let rs = load().resolve("W1AW").expect("resolve W1AW");
        assert_eq!(rs.continent, Continent::NA);
        assert!(rs.is_na);
        assert!(rs.is_wve);
    }

    /// The critical regression this whole feature guards against: without
    /// cty.dat, `resolved_station_for_call`'s hardcoded fallback classifies
    /// G-prefix calls as US (`is_na=true`). With cty.dat, they correctly
    /// resolve to EU.
    #[test]
    fn resolves_english_call_to_eu_not_us_fallback() {
        let rs = load().resolve("G3FXB").expect("resolve G3FXB");
        assert_eq!(rs.continent, Continent::EU);
        assert!(!rs.is_na);
        assert!(!rs.is_wve);
    }

    #[test]
    fn unknown_prefix_returns_none_so_caller_can_fall_back() {
        assert!(load().resolve("ZZ9ZZZ").is_none());
    }
}
