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
        exchange_schema_id: 6,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "mst",
        contest_instance_id: 4,
        field_widths: &[(1, 12), (2, 10)],
        default_macros_fn: mst_macros,
        history_mapping: &[("Name", 2)],
        uses_serial: true,
        cabrillo_id_fn: |_| Some("ICWC-MST"),
        exchange_schema_id: 4,
        auto_toggle_mode: false,
    },
    SpecContestMeta {
        contest_id: "ns_sprint",
        contest_instance_id: 7,
        field_widths: &[(1, 12), (2, 5), (3, 10), (4, 4)],
        default_macros_fn: ns_sprint_macros,
        history_mapping: &[("Name", 3)],
        uses_serial: true,
        cabrillo_id_fn: |mode| match mode {
            CategoryMode::CW => Some("NCCC-SPRINT"),
            _ => None,
        },
        exchange_schema_id: 7,
        auto_toggle_mode: true,
    },
];

pub fn find_spec_contest(id: &str) -> Option<&'static SpecContestMeta> {
    let id_lower = id.to_ascii_lowercase();
    SPEC_CONTESTS.iter().find(|m| m.contest_id == id_lower)
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
