//! Bridges clogger's `ScpLookup` (backed by the SCP master database) to
//! dxfeed's `EnrichmentResolver` so the filter pipeline can reject spots
//! whose callsign is not in the contest master. A call that isn't in SCP
//! is almost certainly a busted RBN copy; this is the highest-leverage
//! structural filter for noisy cluster feeds.
//!
//! Only `in_master_db` is implemented. LoTW, callbook, and club memberships
//! return `None`, which combined with `unknown_policy: Neutral` (dxfeed's
//! default) means those filter rules pass every spot through. When operator
//! data files for those signals get wired in later, add the corresponding
//! methods.

use std::collections::BTreeSet;
use std::sync::Arc;

use dxfeed::resolver::enrichment::EnrichmentResolver;
use logger_core::ScpLookup;

pub struct ScpEnrichment {
    scp: Arc<dyn ScpLookup>,
}

impl ScpEnrichment {
    pub fn new(scp: Arc<dyn ScpLookup>) -> Self {
        Self { scp }
    }
}

impl EnrichmentResolver for ScpEnrichment {
    fn in_master_db(&self, callsign: &str) -> Option<bool> {
        Some(self.scp.contains(callsign))
    }

    fn lotw_user(&self, _callsign: &str) -> Option<bool> {
        None
    }

    fn in_callbook(&self, _callsign: &str) -> Option<bool> {
        None
    }

    fn memberships(&self, _callsign: &str) -> Option<BTreeSet<String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logger_core::NoScp;

    struct FakeScp;

    impl ScpLookup for FakeScp {
        fn partial_matches(&self, _: &str, _: usize) -> Vec<String> {
            Vec::new()
        }
        fn contains(&self, call: &str) -> bool {
            matches!(call, "K5ZD" | "W1AW" | "JA1ABC")
        }
    }

    #[test]
    fn in_master_db_true_for_known_call() {
        let er = ScpEnrichment::new(Arc::new(FakeScp));
        assert_eq!(er.in_master_db("K5ZD"), Some(true));
        assert_eq!(er.in_master_db("W1AW"), Some(true));
    }

    #[test]
    fn in_master_db_false_for_busted_call() {
        let er = ScpEnrichment::new(Arc::new(FakeScp));
        // Typical RBN bust of K5ZD — not in master DB.
        assert_eq!(er.in_master_db("K5ZE"), Some(false));
        assert_eq!(er.in_master_db("JUNK"), Some(false));
    }

    #[test]
    fn other_enrichment_methods_return_none() {
        // No data sources wired yet; None lets the filter's unknown_policy
        // decide (defaults to Neutral, which passes the spot through).
        let er = ScpEnrichment::new(Arc::new(FakeScp));
        assert_eq!(er.lotw_user("K5ZD"), None);
        assert_eq!(er.in_callbook("K5ZD"), None);
        assert_eq!(er.memberships("K5ZD"), None);
    }

    #[test]
    fn no_scp_always_reports_not_in_master() {
        // With no SCP loaded, every call reports false — which combined
        // with a `RequireTrue` filter would drop every spot. Operators who
        // don't load an SCP file should either not set the in_master_db
        // filter, or load one.
        let er = ScpEnrichment::new(Arc::new(NoScp));
        assert_eq!(er.in_master_db("K5ZD"), Some(false));
    }
}
