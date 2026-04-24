use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use contest_engine::spec::{
    InMemoryDomainProvider, InMemoryResolver, Mode as CeMode, ResolvedStation, SpecSession, Value,
    embedded,
};
use contest_engine::types::{Band as CeBand, Callsign, Continent};
use logger_core::CallHistoryLookup;
use qsolog::qso::QsoRecord;
use qsolog::types::{Band, Mode};

use super::{
    BandScore, BreakdownRow, ContestScorer, ScoreBreakdown, ScoreSummary, BAND_LABELS,
    band_label_from_qsolog,
};
use crate::cty::CtyDb;
use crate::log_adapter::decode_exchange_pairs;

pub struct SpecScorer {
    spec_id: String,
    contest_instance_id: u64,
    config: HashMap<String, Value>,
    /// Persistent session — kept alive across inserts so we don't rebuild
    /// the engine from scratch on every QSO.
    session: Option<SpecSession<InMemoryResolver, InMemoryDomainProvider>>,
    /// Calls already inserted into the resolver, so we skip duplicate inserts.
    resolved_calls: HashSet<String>,
    /// Per-band QSO and mult counters, updated incrementally from ApplySummary.
    qsos_by_band: HashMap<String, u32>,
    mults_by_band: HashMap<String, u32>,
    /// Per-band QSO points, for breakdown.
    points_by_band: HashMap<String, i64>,
    /// Per-band, per-mult-type counts: band -> (mult_type -> count).
    mults_by_type_by_band: HashMap<String, HashMap<String, u32>>,
    /// Ordered multiplier type IDs from the contest spec (e.g. ["zone", "country"]).
    mult_type_ids: Vec<String>,
    cached_summary: ScoreSummary,
    /// Loose dupe set keyed on `(call_upper, band_lower, mode_upper)`.
    /// contest-engine's `classify_call_lite_with_mode` can't answer dupe
    /// queries for specs with `dupe_extra_rcvd_fields` set (state QPs use
    /// `["loc"]` for rover handling) because the extra-field dupe key needs
    /// the candidate exchange, which the lite-check API doesn't accept.
    /// So we maintain our own call/band/mode set and check it first in
    /// `is_dupe`. Accepts the rover false-positive: a return trip from the
    /// same call on the same band+mode will flag as dupe even if the rover
    /// has moved counties. The real dupe decision still fires at log time
    /// via `apply_qso_with_mode` with the full exchange.
    loose_dupes: HashSet<(String, String, String)>,
    /// Call-history DB handle. Shared with the event-loop reducer; used
    /// here only by `would_be_new_mult` to downgrade bandmap spots whose
    /// `.ch` row implies a mult that's already been worked.
    call_history: Arc<dyn CallHistoryLookup>,
    /// Pre-translated `(.ch column, contest-engine field-id)` pairs —
    /// derived from the contest's `history_field_mapping()` at scorer
    /// construction. Used to synthesize hypothetical received-exchange
    /// maps from `.ch` rows.
    history_mapping: Vec<(String, String)>,
    /// Parsed cty.dat, when the operator configured one. Used to
    /// populate the contest-engine resolver with accurate DXCC/continent
    /// data per call — without it, the hardcoded prefix fallback in
    /// `resolved_station_for_call` misclassifies every non-K/W/N/A/DL/
    /// JA/VE call as a US station, which silently skews DXCC-based
    /// mults (CQWW country mults, ARRL DX is_wve gating, etc.).
    cty: Option<Arc<CtyDb>>,
    /// Memoization of `(call_upper, band_lower, mode_upper) → verdict`
    /// for the hot bandmap analytics path. `compute_worked_calls` /
    /// `compute_avail` call `is_dupe` and `would_be_new_mult` for every
    /// filtered spot on each analytics recompute; without this, a busy
    /// bandmap re-runs contest-engine's classify (and the new .ch-driven
    /// hypothetical classify) per spot per recompute. Cleared in
    /// `on_inserted`/`rebuild` because the logged mult + dupe set changes
    /// shift verdicts. `Mutex` so the scorer stays `Sync`; contention is
    /// effectively zero since everything runs on one tokio task.
    classify_cache: Mutex<HashMap<(String, String, String), ClassifyVerdict>>,
}

#[derive(Clone, Copy)]
struct ClassifyVerdict {
    is_dupe: bool,
    is_new_mult: bool,
}

impl SpecScorer {
    pub fn new(
        spec_id: impl Into<String>,
        contest_instance_id: u64,
        config: HashMap<String, Value>,
        call_history: Arc<dyn CallHistoryLookup>,
        history_mapping: Vec<(String, String)>,
        cty: Option<Arc<CtyDb>>,
    ) -> anyhow::Result<Self> {
        let mut scorer = Self {
            spec_id: spec_id.into(),
            contest_instance_id,
            config,
            session: None,
            resolved_calls: HashSet::new(),
            qsos_by_band: HashMap::new(),
            mults_by_band: HashMap::new(),
            points_by_band: HashMap::new(),
            mults_by_type_by_band: HashMap::new(),
            mult_type_ids: Vec::new(),
            cached_summary: ScoreSummary::default(),
            loose_dupes: HashSet::new(),
            call_history,
            history_mapping,
            classify_cache: Mutex::new(HashMap::new()),
            cty,
        };
        scorer.init_session()?;
        Ok(scorer)
    }

    fn cache_key(call_norm: &str, band: &str, mode: &str) -> (String, String, String) {
        (
            call_norm.trim().to_ascii_uppercase(),
            band.trim().to_ascii_lowercase(),
            mode.trim().to_ascii_uppercase(),
        )
    }

    fn cache_get(&self, key: &(String, String, String)) -> Option<ClassifyVerdict> {
        self.classify_cache.lock().ok().and_then(|g| g.get(key).copied())
    }

    fn cache_put(&self, key: (String, String, String), verdict: ClassifyVerdict) {
        if let Ok(mut g) = self.classify_cache.lock() {
            g.insert(key, verdict);
        }
    }

    fn clear_classify_cache(&mut self) {
        if let Ok(mut g) = self.classify_cache.lock() {
            g.clear();
        }
    }

    /// Build a fresh empty session. Called at construction and by rebuild.
    /// Returns an error when contest-engine rejects the config (e.g. a state
    /// QP config.toml missing a required `my_is_<state>` field) — at
    /// construction the error propagates out of `SpecScorer::new`, refusing
    /// to start the session. `rebuild` calls this through a warn-only shim
    /// so a mid-session re-init doesn't panic the TUI.
    fn init_session(&mut self) -> anyhow::Result<()> {
        let spec = embedded::spec_by_id(&self.spec_id)
            .ok_or_else(|| anyhow::anyhow!("contest spec `{}` not found in contest-engine embedded registry", self.spec_id))?;
        let domains = embedded::standard_domain_pack();
        let resolver = InMemoryResolver::new();
        let source = ResolvedStation::new("W", Continent::NA, true, true);

        match SpecSession::new(spec, source, self.config.clone(), resolver, domains) {
            Ok(session) => {
                self.mult_type_ids = session
                    .engine()
                    .multiplier_ids()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                self.session = Some(session);
                self.resolved_calls.clear();
                self.loose_dupes.clear();
                Ok(())
            }
            Err(e) => {
                self.session = None;
                Err(anyhow::anyhow!(
                    "contest-engine rejected session config for `{}`: {}. \
                     Check your contest.toml `[station]` section — required fields \
                     for state QSO parties include `my_is_<state>` (bool) plus \
                     `my_county` or `my_loc` depending on in-state vs out-of-state.",
                    self.spec_id,
                    e
                ))
            }
        }
    }

    /// Ensure a callsign is in the resolver. Uses resolver_mut() to insert
    /// on first sight of each call.
    fn ensure_resolved(&mut self, call: &str) {
        let call_upper = call.trim().to_ascii_uppercase();
        if self.resolved_calls.contains(&call_upper) {
            return;
        }
        let station = self.resolve_station(&call_upper);
        if let Some(session) = &mut self.session {
            session.resolver_mut().insert(&call_upper, station);
            self.resolved_calls.insert(call_upper);
        }
    }

    /// Best-effort DXCC/continent resolution for a call. Prefers cty.dat
    /// when loaded; falls back to the hardcoded prefix table so scoring
    /// stays defined for operators without a cty_file configured (and
    /// for callsigns cty.dat doesn't recognize).
    fn resolve_station(&self, call_upper: &str) -> ResolvedStation {
        if let Some(cty) = &self.cty
            && let Some(rs) = cty.resolve(call_upper) {
                return rs;
            }
        resolved_station_for_call(call_upper)
    }

    /// Apply one QSO to the session and update all incremental counters.
    /// Returns true if the session accepted the QSO.
    fn apply_one(&mut self, rec: &QsoRecord) -> bool {
        // Populate `loose_dupes` unconditionally — the record is in the
        // log, so the operator worked this (call, band, mode) tuple.
        // Whether contest-engine accepts the stored exchange on replay
        // (spec drift, domain values changed, rover multi-value edge
        // cases) is a classifier question, not a "did we work them"
        // question. Skipping this insert on Err was the root cause of
        // a bug where restarting mid-contest painted some already-
        // worked stations as un-worked on the bandmap.
        let band_label = band_label_from_qsolog(rec.band);
        self.loose_dupes.insert((
            rec.callsign_norm.to_ascii_uppercase(),
            band_label.clone(),
            mode_label_for_loose(rec.mode),
        ));

        let raw_exchange = match raw_exchange_for_record(rec) {
            Some(e) => e,
            None => return false,
        };

        self.ensure_resolved(&rec.callsign_norm);

        let session = match &mut self.session {
            Some(s) => s,
            None => return false,
        };

        match session.apply_qso_with_mode(
            to_ce_band_from_qsolog(rec.band),
            to_ce_mode_from_qsolog(rec.mode),
            Callsign::new(&rec.callsign_norm),
            &raw_exchange,
        ) {
            Ok(summary) => {
                if !summary.is_dupe {
                    *self.qsos_by_band.entry(band_label.clone()).or_default() += 1;
                    *self.points_by_band.entry(band_label.clone()).or_default() +=
                        summary.qso_points;
                }
                for mult_str in &summary.new_mults {
                    let mult_type = mult_type_from_str(mult_str);
                    *self
                        .mults_by_type_by_band
                        .entry(band_label.clone())
                        .or_default()
                        .entry(mult_type)
                        .or_default() += 1;
                }
                let new_mult_count = summary.new_mults.len() as u32;
                if new_mult_count > 0 {
                    *self.mults_by_band.entry(band_label).or_default() += new_mult_count;
                }
                true
            }
            Err(e) => {
                tracing::warn!(
                    "scorer: apply_qso_with_mode rejected {} {} {:?}: {e}; \
                     dupe indicator still set, but score for this QSO may be off",
                    rec.callsign_norm,
                    raw_exchange,
                    rec.band,
                );
                false
            }
        }
    }

    fn rebuild_summary_from_session(&mut self) {
        let by_band: Vec<(String, BandScore)> = BAND_LABELS
            .iter()
            .map(|label| {
                let qsos = self.qsos_by_band.get(*label).copied().unwrap_or(0);
                let mults = self.mults_by_band.get(*label).copied().unwrap_or(0);
                (label.to_string(), BandScore { qsos, mults })
            })
            .collect();

        let total_qsos = by_band.iter().map(|(_, bs)| bs.qsos).sum();
        let total_mults = by_band.iter().map(|(_, bs)| bs.mults).sum();
        let claimed_score = self
            .session
            .as_ref()
            .map(|s| s.engine().claimed_score())
            .unwrap_or(0);

        self.cached_summary = ScoreSummary {
            by_band,
            total_qsos,
            total_mults,
            claimed_score,
        };
    }
}

impl ContestScorer for SpecScorer {
    fn on_inserted(&mut self, record: &QsoRecord) {
        if record.flags.is_void || record.contest_instance_id != self.contest_instance_id {
            return;
        }
        self.apply_one(record);
        self.rebuild_summary_from_session();
        self.clear_classify_cache();
    }

    fn rebuild(&mut self, records: &[Arc<QsoRecord>]) {
        self.qsos_by_band.clear();
        self.mults_by_band.clear();
        self.points_by_band.clear();
        self.mults_by_type_by_band.clear();
        // Mid-session re-init: if config somehow became invalid (shouldn't
        // happen — it was validated at construction), warn but keep going.
        // `self.session` will be None and subsequent apply_one calls return
        // false. Bootstrap-time init is the gate that refuses startup.
        if let Err(e) = self.init_session() {
            tracing::warn!("SpecScorer: rebuild init_session failed for {}: {e}", self.spec_id);
        }

        let cid = self.contest_instance_id;
        for rec in records
            .iter()
            .filter(|r| !r.flags.is_void && r.contest_instance_id == cid)
        {
            self.apply_one(rec);
        }

        self.rebuild_summary_from_session();
        self.clear_classify_cache();
    }

    fn score_summary(&self) -> ScoreSummary {
        ScoreSummary {
            by_band: self
                .cached_summary
                .by_band
                .iter()
                .map(|(label, bs)| {
                    (
                        label.clone(),
                        BandScore {
                            qsos: bs.qsos,
                            mults: bs.mults,
                        },
                    )
                })
                .collect(),
            total_qsos: self.cached_summary.total_qsos,
            total_mults: self.cached_summary.total_mults,
            claimed_score: self.cached_summary.claimed_score,
        }
    }

    fn score_breakdown(&self) -> ScoreBreakdown {
        let mut rows: Vec<BreakdownRow> = BAND_LABELS
            .iter()
            .filter_map(|label| {
                let qsos = self.qsos_by_band.get(*label).copied().unwrap_or(0);
                if qsos == 0 {
                    return None;
                }
                let points = self.points_by_band.get(*label).copied().unwrap_or(0);
                let type_map = self.mults_by_type_by_band.get(*label);
                let mults: Vec<(String, u32)> = self
                    .mult_type_ids
                    .iter()
                    .map(|mt| {
                        let count = type_map
                            .and_then(|m| m.get(mt))
                            .copied()
                            .unwrap_or(0);
                        (mt.clone(), count)
                    })
                    .collect();
                Some(BreakdownRow {
                    band: label.to_ascii_uppercase(),
                    mode: "CW".to_string(),
                    qsos,
                    points,
                    mults,
                })
            })
            .collect();

        // Total row
        let total_points: i64 = self.points_by_band.values().sum();
        let mut total_mults_by_type: HashMap<String, u32> = HashMap::new();
        for type_map in self.mults_by_type_by_band.values() {
            for (mt, count) in type_map {
                *total_mults_by_type.entry(mt.clone()).or_default() += count;
            }
        }
        let total_mults: Vec<(String, u32)> = self
            .mult_type_ids
            .iter()
            .map(|mt| {
                let count = total_mults_by_type.get(mt).copied().unwrap_or(0);
                (mt.clone(), count)
            })
            .collect();

        rows.push(BreakdownRow {
            band: "total".to_string(),
            mode: "ALL".to_string(),
            qsos: self.cached_summary.total_qsos,
            points: total_points,
            mults: total_mults,
        });

        ScoreBreakdown {
            rows,
            claimed_score: self.cached_summary.claimed_score,
        }
    }

    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        let key = Self::cache_key(call_norm, band, mode);

        // Loose check first: for any contest, (call, band, mode) previously
        // logged → dupe. This is the only working signal for state QPs and
        // other contests with `dupe_extra_rcvd_fields`, where contest-engine
        // returns false on the lite-check API because the extra-field dupe
        // key needs the candidate exchange. For contests without extra
        // fields, this agrees with contest-engine's answer, so checking both
        // is redundant but harmless.
        if self.loose_dupes.contains(&key) {
            return true;
        }
        self.classify(&key, call_norm, band, mode).is_dupe
    }

    fn would_be_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool {
        let key = Self::cache_key(call_norm, band, mode);
        self.classify(&key, call_norm, band, mode).is_new_mult
    }
}

impl SpecScorer {
    /// Memoized classify lookup. Cache is invalidated on any log mutation
    /// (QSO insert, undo, redo, rebuild) by clearing `classify_cache` in
    /// `on_inserted`/`rebuild`. A single lookup populates both dupe and
    /// new-mult verdicts so `is_dupe` and `would_be_new_mult` don't pay
    /// twice for back-to-back calls on the same (call, band, mode).
    fn classify(
        &self,
        key: &(String, String, String),
        call_norm: &str,
        band: &str,
        mode: &str,
    ) -> ClassifyVerdict {
        if let Some(v) = self.cache_get(key) {
            return v;
        }

        let verdict = self.compute_classify(&key.0, call_norm, band, mode);
        self.cache_put(key.clone(), verdict);
        verdict
    }

    fn compute_classify(
        &self,
        call_upper: &str,
        call_norm: &str,
        band: &str,
        mode: &str,
    ) -> ClassifyVerdict {
        let session = match &self.session {
            Some(s) => s,
            None => return ClassifyVerdict { is_dupe: false, is_new_mult: false },
        };

        if self.resolved_calls.contains(call_upper) {
            return match session.classify_call_lite_with_mode(
                to_ce_band(band),
                to_ce_mode(mode),
                Callsign::new(call_norm),
            ) {
                Ok(c) => ClassifyVerdict {
                    is_dupe: c.is_dupe,
                    is_new_mult: !c.new_mults.is_empty(),
                },
                Err(_) => ClassifyVerdict { is_dupe: false, is_new_mult: false },
            };
        }

        // Unknown call — never logged, so not a dupe. Mult is optimistic by
        // default; call-history data may *downgrade* to false when the .ch
        // row implies the mult has already been worked. We never upgrade
        // on .ch — the default already covers "we don't know, assume
        // valuable", and .ch is stale/partial often enough that treating
        // it as authoritative both ways would mis-classify rovers and
        // operators who moved.
        let is_new_mult = self.hypothetical_is_new_mult(call_upper, band, mode).unwrap_or(true);
        ClassifyVerdict { is_dupe: false, is_new_mult }
    }

    /// Returns `Some(answer)` when call-history evidence lets us answer
    /// authoritatively, `None` to fall back to the optimistic default.
    /// See the `compute_classify` comment for the upgrade/downgrade policy.
    fn hypothetical_is_new_mult(
        &self,
        call_upper: &str,
        band: &str,
        mode: &str,
    ) -> Option<bool> {
        if self.history_mapping.is_empty() {
            return None;
        }
        let pairs = self.call_history.lookup(call_upper)?;

        let ch_row: HashMap<&str, &str> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let mut hypothetical_rcvd: HashMap<String, String> = HashMap::new();
        for (ch_col, engine_field_id) in &self.history_mapping {
            if let Some(v) = ch_row.get(ch_col.as_str())
                && !v.trim().is_empty() {
                    hypothetical_rcvd.insert(engine_field_id.clone(), v.to_string());
                }
        }
        if hypothetical_rcvd.is_empty() {
            return None;
        }

        let dest = self.resolve_station(call_upper);
        let session = self.session.as_ref()?;
        match session.classify_hypothetical_with_mode(
            to_ce_band(band),
            to_ce_mode(mode),
            Callsign::new(call_upper),
            &dest,
            &hypothetical_rcvd,
        ) {
            Ok(summary) => Some(!summary.would_be_new_mults.is_empty()),
            Err(_) => None,
        }
    }
}

/// Extract the multiplier type from a "type:value" string.
/// e.g. "COUNTRY:W" → "country", "ZONE:5" → "zone".
fn mult_type_from_str(s: &str) -> String {
    s.split_once(':')
        .map(|(t, _)| t.to_ascii_lowercase())
        .unwrap_or_else(|| s.to_ascii_lowercase())
}

fn raw_exchange_for_record(rec: &QsoRecord) -> Option<String> {
    let pairs = decode_exchange_pairs(&rec.exchange).ok()?;
    if pairs.is_empty() {
        return None;
    }
    // Exclude the sent serial and any persisted sent-exchange fields
    // (keys prefixed with `sent_`, appended by ESM for Cabrillo / RTC
    // fidelity) — they are not part of the received exchange that
    // contest-engine scores against.
    Some(
        pairs
            .into_iter()
            .filter(|(k, _)| k != "serial" && !k.starts_with("sent_"))
            .map(|(_, v)| v)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn resolved_station_for_call(call: &str) -> ResolvedStation {
    let upper = call.trim().to_ascii_uppercase();
    if upper.starts_with("DL") {
        return ResolvedStation::new("DL", Continent::EU, false, false);
    }
    if upper.starts_with("JA") {
        return ResolvedStation::new("JA", Continent::AS, false, false);
    }
    if upper.starts_with("VE") {
        return ResolvedStation::new("VE", Continent::NA, true, true);
    }
    if upper.starts_with('K')
        || upper.starts_with('W')
        || upper.starts_with('N')
        || upper.starts_with('A')
    {
        return ResolvedStation::new("W", Continent::NA, true, true);
    }

    ResolvedStation::new("W", Continent::NA, true, true)
}

fn to_ce_band(s: &str) -> CeBand {
    match s.to_ascii_lowercase().as_str() {
        "160m" => CeBand::B160,
        "80m" => CeBand::B80,
        "40m" => CeBand::B40,
        "20m" => CeBand::B20,
        "15m" => CeBand::B15,
        _ => CeBand::B10,
    }
}

fn to_ce_mode(s: &str) -> CeMode {
    match s.to_ascii_uppercase().as_str() {
        "SSB" => CeMode::SSB,
        _ => CeMode::CW,
    }
}

/// Canonicalize a qsolog Mode to the same uppercase string shape the reducer
/// passes into `is_dupe`, so loose_dupes lookups line up with inserts.
fn mode_label_for_loose(m: Mode) -> String {
    match m {
        Mode::CW => "CW".to_string(),
        Mode::SSB => "SSB".to_string(),
        Mode::Digital => "DIGITAL".to_string(),
        _ => "OTHER".to_string(),
    }
}

fn to_ce_band_from_qsolog(b: Band) -> CeBand {
    match b {
        Band::B160m => CeBand::B160,
        Band::B80m => CeBand::B80,
        Band::B40m => CeBand::B40,
        Band::B20m => CeBand::B20,
        Band::B15m => CeBand::B15,
        _ => CeBand::B10,
    }
}

fn to_ce_mode_from_qsolog(m: Mode) -> CeMode {
    match m {
        Mode::SSB => CeMode::SSB,
        _ => CeMode::CW,
    }
}
