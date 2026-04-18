//! Bridges clogger's `CtyDb` (parsed cty.dat via `station-data`) to
//! dxfeed's `EntityResolver` so the filter pipeline can evaluate
//! `geo.dx.*` / `geo.spotter.*` rules (continent, CQ/ITU zone, entity).
//!
//! Without this adapter, every spot has `continent = None` in dxfeed's
//! filter pipeline; a `continent_allow` allowlist combined with dxfeed's
//! default `unknown_policy: Neutral` silently drops every spot.
//!
//! Fields not present in `station_data::ResolvedStation` (lat, lon,
//! human-readable entity name) are filled with best-effort placeholders —
//! `entity_name` uses the canonical DXCC prefix (e.g. `"W"`, `"JA"`).
//! Users running `geo.*.entity_allow` / `entity_deny` with full names
//! like `"United States"` will need the prefix form until station-data
//! exposes the name on lookup.

use std::sync::Arc;

use dxfeed::domain::Continent;
use dxfeed::resolver::entity::{EntityInfo, EntityResolver};

use crate::cty::CtyDb;

pub struct CtyEntityResolver {
    cty: Arc<CtyDb>,
}

impl CtyEntityResolver {
    pub fn new(cty: Arc<CtyDb>) -> Self {
        Self { cty }
    }
}

impl EntityResolver for CtyEntityResolver {
    fn resolve(&self, callsign: &str) -> Option<EntityInfo> {
        let resolved = self.cty.inner.lookup(callsign)?;
        let continent = parse_continent(&resolved.continent)?;
        Some(EntityInfo {
            entity_name: resolved.dxcc.clone(),
            continent,
            cq_zone: resolved.cq_zone.unwrap_or(0),
            itu_zone: resolved.itu_zone.unwrap_or(0),
            lat: 0.0,
            lon: 0.0,
            primary_prefix: resolved.dxcc,
        })
    }
}

fn parse_continent(s: &str) -> Option<Continent> {
    match s.trim().to_ascii_uppercase().as_str() {
        "NA" => Some(Continent::NA),
        "SA" => Some(Continent::SA),
        "EU" => Some(Continent::EU),
        "AF" => Some(Continent::AF),
        "AS" => Some(Continent::AS),
        "OC" => Some(Continent::OC),
        "AN" => Some(Continent::AN),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI_CTY: &str = "United States of America:           05:  08:  NA:   37.53:   97.00:     5.0:  K:
    K,W,N,AA,AB,AC,AD,AE,AF,AG,AH,AI,AJ,AK,AL;
Japan:                              25:  45:  AS:   36.24: -139.00:    -9.0:  JA:
    JA,JE,JF,JG,JH,JI,JJ,JK,JL,JM,JN,JO,JP,JQ,JR,JS,7J,7K,7L,7M,7N,8J,8K,8L,8M,8N;
";

    fn resolver() -> CtyEntityResolver {
        let inner = station_data::CtyDb::from_reader(MINI_CTY.as_bytes()).unwrap();
        CtyEntityResolver::new(Arc::new(CtyDb { inner }))
    }

    #[test]
    fn us_callsign_resolves_to_na() {
        let r = resolver();
        let info = r.resolve("W1AW").expect("W1AW should resolve");
        assert_eq!(info.continent, Continent::NA);
        assert_eq!(info.cq_zone, 5);
        assert_eq!(info.itu_zone, 8);
    }

    #[test]
    fn japanese_callsign_resolves_to_as() {
        let r = resolver();
        let info = r.resolve("JA1ABC").expect("JA1ABC should resolve");
        assert_eq!(info.continent, Continent::AS);
    }

    #[test]
    fn unknown_prefix_returns_none() {
        let r = resolver();
        assert!(r.resolve("ZZ9ZZZ").is_none());
    }
}
