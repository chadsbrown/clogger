use crate::state::Macros;

use super::traits::CategoryMode;

/// Clogger-specific metadata for a spec-driven contest. Each entry pairs
/// a contest-engine spec ID with TUI/integration concerns that don't belong
/// in the scoring engine: field display widths, default CW macros, call
/// history column mappings, and serial number policy.
pub struct SpecContestMeta {
    /// Must match the contest-engine `spec.id` (e.g. "cqww", "cwt").
    pub contest_id: &'static str,
    /// Stable identifier used as the qsolog `contest_instance_id`.
    pub contest_instance_id: u64,
    /// (field_id, width) pairs for TUI display. field_id 1 is always CALL;
    /// received exchange fields start at 2.
    pub field_widths: &'static [(u16, u16)],
    /// Factory function for contest-specific default CW macros.
    pub default_macros_fn: fn() -> Macros,
    /// Maps call history `.ch` column names to form field_ids.
    pub history_mapping: &'static [(&'static str, u16)],
    /// Whether this contest uses auto-incrementing serial numbers.
    pub uses_serial: bool,
    /// Maps CategoryMode to Cabrillo contest ID string.
    pub cabrillo_id_fn: fn(CategoryMode) -> Option<&'static str>,
    /// Maps CategoryMode to the RTC (Real Time Contest) server's contest
    /// identifier, if the RTC server accepts uploads for this contest.
    /// Returns `None` for contests the RTC server doesn't support; the RTC
    /// adapter skips posting when this is `None`. This is intentionally
    /// separate from `cabrillo_id_fn` — the RTC server uses its own naming
    /// convention (e.g. "CW-Ops" for CWT, where Cabrillo uses "CW-OPS").
    pub rtc_id_fn: fn(CategoryMode) -> Option<&'static str>,
    /// Exchange schema ID for QsoDraft (matches qsolog convention).
    pub exchange_schema_id: u16,
    /// Whether the operating mode auto-toggles (RUN↔S&P) after logging a QSO.
    pub auto_toggle_mode: bool,
}

pub const SPEC_CONTESTS: &[SpecContestMeta] = &[
    SpecContestMeta {
        contest_id: "cqww",
        contest_instance_id: 1,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: default_macros,
        history_mapping: &[("CqZone", 3)],
        uses_serial: false,
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("CQ-WW-CW"),
            CategoryMode::SSB => Some("CQ-WW-SSB"),
            CategoryMode::Mixed => None,
        },
        rtc_id_fn: no_rtc,
        exchange_schema_id: 1,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "cwt",
        contest_instance_id: 3,
        field_widths: &[(1, 12), (2, 10), (3, 6)],
        default_macros_fn: cwt_macros,
        history_mapping: &[("Name", 2), ("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("CW-OPS"),
        // RTC server spec example uses "CW-Ops" as the contest identifier
        // for CWT postings (distinct from the Cabrillo "CW-OPS" naming).
        rtc_id_fn: |_| Some("CW-Ops"),
        exchange_schema_id: 3,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "naqp",
        contest_instance_id: 5,
        field_widths: &[(1, 12), (2, 10), (3, 4)],
        default_macros_fn: naqp_macros,
        history_mapping: &[("Name", 2)],
        uses_serial: false,
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("NAQP-CW"),
            CategoryMode::SSB => Some("NAQP-SSB"),
            CategoryMode::Mixed => None,
        },
        rtc_id_fn: no_rtc,
        exchange_schema_id: 5,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "arrl_dx",
        contest_instance_id: 6,
        field_widths: &[(1, 12), (2, 3), (3, 5)],
        default_macros_fn: default_macros,
        history_mapping: &[],
        uses_serial: false,
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("ARRL-DX-CW"),
            CategoryMode::SSB => Some("ARRL-DX-SSB"),
            CategoryMode::Mixed => None,
        },
        rtc_id_fn: no_rtc,
        exchange_schema_id: 6,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "mst",
        contest_instance_id: 4,
        field_widths: &[(1, 12), (2, 10), (3, 5)],
        default_macros_fn: mst_macros,
        history_mapping: &[("Name", 2)],
        uses_serial: true,
        cabrillo_id_fn: |_| Some("ICWC-MST"),
        // Matches the Cabrillo identifier; unconfirmed against
        // HamScore's accepted list. If the server rejects with
        // "Contest not supported", the RTC badge flips to gray
        // `Unsupported` — try "MST" or contact the server admin.
        rtc_id_fn: |_| Some("ICWC-MST"),
        exchange_schema_id: 4,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "ns_sprint",
        contest_instance_id: 7,
        field_widths: &[(1, 12), (2, 5), (3, 10), (4, 4)],
        default_macros_fn: ns_sprint_macros,
        history_mapping: &[("Name", 3), ("State", 4)],
        uses_serial: true,
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("NCCC-SPRINT"),
            _ => None,
        },
        rtc_id_fn: no_rtc,
        exchange_schema_id: 7,
        auto_toggle_mode: true,
    },
    SpecContestMeta {
        contest_id: "ss",
        contest_instance_id: 2,
        field_widths: &[(1, 12), (2, 5), (3, 2), (4, 3), (5, 4)],
        default_macros_fn: ss_macros,
        history_mapping: &[("Exch1", 3), ("CK", 4), ("Sect", 5)],
        uses_serial: true,
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("ARRL-SS-CW"),
            CategoryMode::SSB => Some("ARRL-SS-SSB"),
            CategoryMode::Mixed => None,
        },
        rtc_id_fn: no_rtc,
        exchange_schema_id: 2,
        auto_toggle_mode: false,
    },
    // State QSO parties. All share the same shape: CALL + RST + LOC (county or
    // state/province/DXCC depending on whether the op is inside the state).
    // In-state vs out-of-state behavior is driven by `my_is_<state>` config,
    // set via the `[station]` TOML section, not by the registry.
    SpecContestMeta {
        contest_id: "flqp",
        contest_instance_id: 8,
        // LOC width 8 leaves room for county-line mobiles sending "ALC/BAK".
        field_widths: &[(1, 12), (2, 3), (3, 8)],
        default_macros_fn: flqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("FL-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 8,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "gaqp",
        contest_instance_id: 9,
        field_widths: &[(1, 12), (2, 3), (3, 4)],
        default_macros_fn: gaqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("GA-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 9,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "inqp",
        contest_instance_id: 10,
        field_widths: &[(1, 12), (2, 3), (3, 5)],
        default_macros_fn: inqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("IN-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 10,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "miqp",
        contest_instance_id: 11,
        field_widths: &[(1, 12), (2, 3), (3, 4)],
        default_macros_fn: miqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("MI-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 11,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "moqp",
        contest_instance_id: 12,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: moqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("MO-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 12,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "ndqp",
        contest_instance_id: 13,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: ndqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("ND-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 13,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "neqp",
        contest_instance_id: 14,
        field_widths: &[(1, 12), (2, 3), (3, 5)],
        default_macros_fn: neqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        // New England QP uses the bare "NEQP" contest ID, not "NE-QSO-PARTY".
        cabrillo_id_fn: |_| Some("NEQP"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 14,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "nhqp",
        contest_instance_id: 15,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: nhqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        // NH QSO Party is CW-only.
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("NH-QSO-PARTY"),
            _ => None,
        },
        rtc_id_fn: no_rtc,
        exchange_schema_id: 15,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "nmqp",
        contest_instance_id: 16,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: nmqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("NM-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 16,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "neqsop",
        contest_instance_id: 17,
        field_widths: &[(1, 12), (2, 3), (3, 4)],
        default_macros_fn: neqsop_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("NE-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 17,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "onqp",
        contest_instance_id: 18,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: onqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("ON-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 18,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "qcqp",
        contest_instance_id: 19,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: qcqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("QC-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 19,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "deqp",
        contest_instance_id: 20,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: deqp_macros,
        history_mapping: &[("Exch1", 3)],
        uses_serial: false,
        cabrillo_id_fn: |_| Some("DE-QSO-PARTY"),
        rtc_id_fn: no_rtc,
        exchange_schema_id: 20,
        auto_toggle_mode: false,
    },
];

pub fn find_spec_contest(id: &str) -> Option<&'static SpecContestMeta> {
    let id_lower = id.to_ascii_lowercase();
    let effective_id = match id_lower.as_str() {
        "sweeps" => "ss",
        _ => &id_lower,
    };
    SPEC_CONTESTS.iter().find(|m| m.contest_id == effective_id)
}

/// Default `rtc_id_fn` value for contests the RTC server doesn't
/// (yet) accept — returning `None` causes the RTC adapter to skip
/// posting. Individual contests opt in by overriding `rtc_id_fn` with
/// their confirmed RTC identifier.
fn no_rtc(_: CategoryMode) -> Option<&'static str> {
    None
}

fn default_macros() -> Macros {
    Macros::default()
}

fn cwt_macros() -> Macros {
    Macros {
        f1: "CQ CWT {MYCALL}".to_string(),
        f2: "{MYNAME} {MYXCHG}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}

fn mst_macros() -> Macros {
    Macros {
        f1: "CQ MST {MYCALL}".to_string(),
        f2: "{MYNAME} {SERIAL}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}

fn naqp_macros() -> Macros {
    Macros {
        f1: "CQ NA {MYCALL}".to_string(),
        f2: "{MYNAME} {MYXCHG}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}

fn ns_sprint_macros() -> Macros {
    Macros {
        f1: "NS {MYCALL}".to_string(),
        f2: "{MYCALL} {SERIAL} {MYNAME} {MYXCHG}".to_string(),
        f3: "R".to_string(),
        sp_f2: Some("{CALL} {SERIAL} {MYNAME} {MYXCHG} {MYCALL}".to_string()),
        ..Macros::default()
    }
}

fn ss_macros() -> Macros {
    Macros {
        f1: "CQ SS {MYCALL}".to_string(),
        f2: "{NR} {PREC} {CK} {SEC}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}

/// Shared builder for state QSO party default macros. `state` is the call-up
/// prefix (e.g. "GA", "FL", "NEQP"). F2 uses `{MYXCHG}` so the existing
/// top-level `my_xchg` mechanism works for both in-state (county) and
/// out-of-state (state/province) operators — the user picks what MYXCHG means
/// for their entry.
fn state_qp_macros_for(state: &str) -> Macros {
    Macros {
        f1: format!("CQ {state} {{MYCALL}}"),
        f2: "{RST_SENT} {MYXCHG}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}

fn flqp_macros() -> Macros { state_qp_macros_for("FL") }
fn gaqp_macros() -> Macros { state_qp_macros_for("GA") }
fn inqp_macros() -> Macros { state_qp_macros_for("IN") }
fn miqp_macros() -> Macros { state_qp_macros_for("MI") }
fn moqp_macros() -> Macros { state_qp_macros_for("MO") }
fn ndqp_macros() -> Macros { state_qp_macros_for("ND") }
fn neqp_macros() -> Macros { state_qp_macros_for("NEQP") }
fn nhqp_macros() -> Macros { state_qp_macros_for("NH") }
fn nmqp_macros() -> Macros { state_qp_macros_for("NM") }
fn neqsop_macros() -> Macros { state_qp_macros_for("NE") }
fn onqp_macros() -> Macros { state_qp_macros_for("ON") }
fn qcqp_macros() -> Macros { state_qp_macros_for("QC") }
fn deqp_macros() -> Macros { state_qp_macros_for("DE") }

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registered contest must have a matching embedded spec in
    /// contest-engine, and every exchange_schema_id / contest_instance_id
    /// must be unique. Guards against typos in `contest_id` and against
    /// accidentally reusing an instance id when adding new contests.
    #[test]
    fn all_registered_contests_resolve_to_embedded_specs() {
        let mut seen_instance = std::collections::HashSet::new();
        let mut seen_schema = std::collections::HashSet::new();
        for meta in SPEC_CONTESTS {
            assert!(
                contest_engine::spec::embedded::spec_by_id(meta.contest_id).is_some(),
                "contest_id `{}` has no embedded spec in contest-engine",
                meta.contest_id
            );
            assert!(
                seen_instance.insert(meta.contest_instance_id),
                "duplicate contest_instance_id {} ({})",
                meta.contest_instance_id,
                meta.contest_id
            );
            assert!(
                seen_schema.insert(meta.exchange_schema_id),
                "duplicate exchange_schema_id {} ({})",
                meta.exchange_schema_id,
                meta.contest_id
            );
        }
    }
}
