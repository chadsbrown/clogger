use std::collections::HashMap;

use crate::entry::state::EntryState;

pub type RadioId = u8;
pub type OperatorId = u16;
pub type QsoRef = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EsmPolicy {
    pub run_two_step: bool,
}

impl Default for EsmPolicy {
    fn default() -> Self {
        Self {
            run_two_step: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioState {
    pub freq_hz: u64,
    pub mode: String,
    pub is_ptt: bool,
    pub cw_speed: u8,
    /// Current receiver passband (filter width) in hertz, if known.
    pub filter_width_hz: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastLoggedContext {
    pub call: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub call: String,
    pub freq_hz: u64,
    pub mode: String,
}

/// Per-radio bandmap cursor.
///
/// The bandmap is a discrete list but the rig is a continuous tuner, so
/// two visual states are needed. `On` means a spot falls inside the rig's
/// current receive passband — highlight that row. `Between` means the rig
/// is parked in clear air — draw a divider at this insertion index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "index")]
pub enum BandmapCursor {
    On(usize),
    Between(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QsoDraft {
    pub contest_id: String,
    pub callsign: String,
    pub band: String,
    pub mode: String,
    pub freq_hz: u64,
    pub exchange_schema_id: u16,
    pub exchange_pairs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Macros {
    pub f1: String,
    pub f2: String,
    pub f3: String,
    pub f5: String,
    pub f7: String,
    pub f8: String,
    pub f9: String,
    /// If set, S&P ESM sends this instead of f2 for the exchange step.
    pub sp_f2: Option<String>,
}

impl Default for Macros {
    fn default() -> Self {
        Self {
            f1: "CQ TEST {MYCALL}".to_string(),
            f2: "{RST_SENT} {MYZONE}".to_string(),
            f3: "TU {MYCALL}".to_string(),
            f5: "{CALL}".to_string(),
            f7: String::new(),
            f8: String::new(),
            f9: String::new(),
            sp_f2: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub now_ms: i64,
    pub focused_radio: RadioId,
    pub active_operator: OperatorId,
    pub radios: HashMap<RadioId, RadioState>,
    pub entries: HashMap<RadioId, EntryState>,
    pub bandmap: Vec<Spot>,
    pub last_logged: Option<QsoRef>,
    pub my_call: String,
    pub my_zone: u8,
    pub rst_sent: String,
    pub my_exchange: HashMap<String, String>,
    pub esm_policy: EsmPolicy,
    pub bandmap_cursors: HashMap<RadioId, BandmapCursor>,
    pub default_cw_speed: u8,
    pub serial_counter: Option<u32>,
    /// Passband QRM warning width in hertz. When `Some(n)`, the warning fires
    /// in Run mode if any bandmap spot (matching the focused radio's mode, not
    /// `my_call`) sits within ±(n/2) of the focused radio's frequency. `None`
    /// disables the warning entirely. Not derived from rig-reported filter
    /// width — it's an operator-chosen threshold.
    pub passband_qrm_width_hz: Option<u32>,
    /// Monotonic counter that increments on every bandmap mutation (spot
    /// received or withdrawn). Consumers — notably the TUI `BandmapCache` —
    /// compare stored versions against this to detect when cached filtered
    /// results have gone stale.
    pub bandmap_version: u64,
}

impl AppState {
    /// Get the entry state for the focused radio.
    /// Panics if the focused radio's entry is missing — entries must be initialized at bootstrap.
    pub fn focused_entry(&self) -> &EntryState {
        self.entries
            .get(&self.focused_radio)
            .expect("entries must contain the focused radio")
    }

    /// Get a mutable reference to the entry state for the focused radio.
    pub fn focused_entry_mut(&mut self) -> &mut EntryState {
        self.entries
            .get_mut(&self.focused_radio)
            .expect("entries must contain the focused radio")
    }

    /// Get the entry state for a specific radio, if it exists.
    pub fn entry_for(&self, radio: RadioId) -> Option<&EntryState> {
        self.entries.get(&radio)
    }

    /// Get a mutable reference to the entry state for a specific radio.
    pub fn entry_for_mut(&mut self, radio: RadioId) -> Option<&mut EntryState> {
        self.entries.get_mut(&radio)
    }

    /// Return the focused radio's callsign as a borrowed `&str`.
    ///
    /// Relies on the invariant that field[0] (CALL) always holds an
    /// already-uppercased value. Writers must enforce this:
    ///
    /// - `TextInput` chars are uppercased by the terminal adapter before
    ///   reaching the reducer, so `insert_str` preserves the invariant.
    /// - SCP and bandmap-navigation writes uppercase explicitly at the
    ///   write site (see `reducer.rs`).
    /// - `clear()` trivially preserves it.
    ///
    /// This method only trims whitespace (O(1), no allocation) and returns
    /// a slice into the underlying field value. Used on the keystroke hot
    /// path via `recompute_feedback`, `recompute_passband_warning`,
    /// `apply_call_history`, `macro_expand`, and ESM guards.
    pub fn current_call(&self) -> &str {
        self.focused_entry()
            .get_field_value_by_id(1)
            .map(|v| v.trim())
            .unwrap_or("")
    }
}
