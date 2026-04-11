use crate::{
    contest::{
        filtered_bandmap_spots, freq_to_band_label, normalize_mode,
        traits::{ContestEntry, EntryContext},
    },
    effects::Effect,
    entry::{
        esm::handle_esm,
        state::{EsmStep, OpMode},
    },
    events::{AppEvent, Key},
    macro_expand::expand_macro,
    state::{AppState, BandmapCursor, Macros, RadioId, RadioState},
};

pub trait DupeChecker {
    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool;
}

pub trait MultChecker {
    fn is_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool;
}

pub struct NoDupeChecker;

impl DupeChecker for NoDupeChecker {
    fn is_dupe(&self, _call_norm: &str, _band: &str, _mode: &str) -> bool {
        false
    }
}

pub struct NoMultChecker;

impl MultChecker for NoMultChecker {
    fn is_new_mult(&self, _call_norm: &str, _band: &str, _mode: &str) -> bool {
        false
    }
}

pub trait CallHistoryLookup {
    /// Exact match. Returns .ch column-name/value pairs, e.g. [("CqZone", "5")].
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>>;
}

pub struct NoCallHistory;

impl CallHistoryLookup for NoCallHistory {
    fn lookup(&self, _: &str) -> Option<Vec<(String, String)>> {
        None
    }
}

pub trait ScpLookup {
    fn partial_matches(&self, prefix: &str, limit: usize) -> Vec<String>;
    fn n_plus_one_matches(&self, _call: &str, _limit: usize) -> Vec<String> {
        Vec::new()
    }
}

pub struct NoScp;

impl ScpLookup for NoScp {
    fn partial_matches(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
}

pub fn reduce(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    macros: &Macros,
    dupe_checker: &dyn DupeChecker,
    mult_checker: &dyn MultChecker,
    call_history: &dyn CallHistoryLookup,
    scp: &dyn ScpLookup,
    ev: AppEvent,
) -> Vec<Effect> {
    match ev {
        AppEvent::TimerTick { now_ms } => {
            st.now_ms = now_ms;
            Vec::new()
        }
        AppEvent::RigStatus {
            radio,
            freq_hz,
            mode,
            is_ptt,
            filter_width_hz,
        } => {
            let prev = st.radios.get(&radio);
            let prev_freq = prev.map(|r| r.freq_hz);
            let cw_speed = prev
                .map(|r| r.cw_speed)
                .unwrap_or(st.default_cw_speed);
            let filter_width_hz = filter_width_hz
                .or_else(|| prev.and_then(|r| r.filter_width_hz));
            st.radios.insert(
                radio,
                RadioState {
                    freq_hz,
                    mode,
                    is_ptt,
                    cw_speed,
                    filter_width_hz,
                },
            );
            // Snap the per-radio bandmap cursor to the new freq only when
            // it actually changed. Suppressing the no-op case keeps manual
            // Ctrl+Up/Down navigation from being overwritten by the next
            // rig poll carrying the same frequency.
            if prev_freq != Some(freq_hz) {
                snap_bandmap_cursor_to_freq(st, radio);
            }
            if radio == st.focused_radio {
                recompute_feedback(st, dupe_checker, mult_checker);
                recompute_passband_warning(st);
            }
            Vec::new()
        }
        AppEvent::RigDisconnected { .. } => Vec::new(),
        // Hardware-task error events — purely TUI concerns (status bar,
        // error banner). The reducer doesn't track device health in the
        // contest state machine, so these are no-ops here.
        AppEvent::KeyerDisconnected
        | AppEvent::KeyerError { .. }
        | AppEvent::So2rDisconnected
        | AppEvent::So2rError { .. }
        | AppEvent::PersistError { .. } => Vec::new(),
        AppEvent::SpotReceived { spot } => {
            st.bandmap.push(spot);
            st.bandmap_version = st.bandmap_version.wrapping_add(1);
            recompute_passband_warning(st);
            Vec::new()
        }
        AppEvent::SpotWithdrawn { call } => {
            st.bandmap.retain(|s| s.call != call);
            st.bandmap_version = st.bandmap_version.wrapping_add(1);
            recompute_passband_warning(st);
            Vec::new()
        }
        AppEvent::SetOpMode { mode } => {
            st.focused_entry_mut().mode = mode;
            recompute_passband_warning(st);
            Vec::new()
        }
        AppEvent::ToggleOpMode => {
            let entry = st.focused_entry_mut();
            entry.mode = match entry.mode {
                OpMode::Run => OpMode::Sp,
                OpMode::Sp => OpMode::Run,
            };
            recompute_passband_warning(st);
            Vec::new()
        }
        AppEvent::FocusRadio { radio } => {
            // Entry focus only — does NOT touch OTRSP TX routing.
            // The runtime updates RX audio to follow focus.
            // Any in-flight CW on the previous radio continues uninterrupted.
            st.focused_radio = radio;
            recompute_feedback(st, dupe_checker, mult_checker);
            recompute_passband_warning(st);
            vec![Effect::So2rFocusChanged { radio }]
        }
        AppEvent::SwapRadios => {
            // Toggle between radio 1 and radio 2 (entry focus only)
            let new_radio = if st.focused_radio == 1 { 2 } else { 1 };
            st.focused_radio = new_radio;
            recompute_feedback(st, dupe_checker, mult_checker);
            recompute_passband_warning(st);
            vec![Effect::So2rFocusChanged { radio: new_radio }]
        }
        AppEvent::SetOperator { operator } => {
            st.active_operator = operator;
            Vec::new()
        }
        AppEvent::TextInput { s } => {
            let mut touched_call = false;
            if let Some(field) = st.focused_entry_mut().focused_mut() {
                touched_call = field.field_id == 1;
                if !touched_call {
                    field.from_history = false;
                }
                field.value.insert_str(field.cursor, &s);
                field.cursor += s.len();
            }
            revalidate_after_edit(st, contest);
            if touched_call {
                // Editing the call after exchange was sent forces a resend
                let entry = st.focused_entry_mut();
                if entry.esm_step == EsmStep::ExchSent {
                    entry.esm_step = EsmStep::Idle;
                }
                entry.scp_cycle_index = None;
                recompute_feedback(st, dupe_checker, mult_checker);
                apply_call_history(st, contest, call_history, scp);
                revalidate_after_edit(st, contest);
            }
            Vec::new()
        }
        AppEvent::KeyPress { key } => match key {
            Key::Space | Key::Tab => {
                let entry = st.focused_entry_mut();
                if !entry.fields.is_empty() {
                    entry.focus = (entry.focus + 1) % entry.fields.len();
                }
                Vec::new()
            }
            Key::Backspace => {
                let mut touched_call = false;
                if let Some(field) = st.focused_entry_mut().focused_mut() {
                    touched_call = field.field_id == 1;
                    if !touched_call {
                        field.from_history = false;
                    }
                    if field.cursor > 0 {
                        field.cursor -= 1;
                        field.value.remove(field.cursor);
                    }
                }
                revalidate_after_edit(st, contest);
                if touched_call {
                    // Editing the call after exchange was sent forces a resend
                    let entry = st.focused_entry_mut();
                    if entry.esm_step == EsmStep::ExchSent {
                        entry.esm_step = EsmStep::Idle;
                    }
                    entry.scp_cycle_index = None;
                    recompute_feedback(st, dupe_checker, mult_checker);
                    apply_call_history(st, contest, call_history, scp);
                    revalidate_after_edit(st, contest);
                }
                Vec::new()
            }
            Key::Left => {
                if let Some(field) = st.focused_entry_mut().focused_mut() {
                    field.cursor = field.cursor.saturating_sub(1);
                }
                Vec::new()
            }
            Key::Right => {
                if let Some(field) = st.focused_entry_mut().focused_mut() {
                    if field.cursor < field.value.len() {
                        field.cursor += 1;
                    }
                }
                Vec::new()
            }
            Key::Esc => vec![Effect::CwAbort],
            Key::F1 => vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro(&macros.f1, st),
            }],
            Key::F2 => vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro(&macros.f2, st),
            }],
            Key::F3 => vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro(&macros.f3, st),
            }],
            Key::F5 => vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro(&macros.f5, st),
            }],
            Key::F7 | Key::F8 | Key::F9 => {
                let text = match key {
                    Key::F7 => &macros.f7,
                    Key::F8 => &macros.f8,
                    Key::F9 => &macros.f9,
                    _ => unreachable!(),
                };
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::CwSend {
                        radio: st.focused_radio,
                        text: expand_macro(text, st),
                    }]
                }
            }
            Key::F12 => {
                let entry = st.focused_entry_mut();
                entry.clear_values();
                entry.esm_step = EsmStep::Idle;
                entry.scp_matches.clear();
                entry.scp_n1_matches.clear();
                entry.scp_cycle_index = None;
                Vec::new()
            }
            Key::Equal => {
                if st.focused_entry().scp_matches.is_empty() {
                    return Vec::new();
                }
                let (new_call, saved_matches, saved_n1, saved_index) = {
                    let entry = st.focused_entry_mut();
                    let len = entry.scp_matches.len();
                    let idx = match entry.scp_cycle_index {
                        None => 0,
                        Some(i) => (i + 1) % len,
                    };
                    entry.scp_cycle_index = Some(idx);
                    // Uppercase here to maintain the CALL-field invariant that
                    // `current_call()` relies on. SCP files are usually already
                    // uppercase but we don't trust them.
                    let new_call = entry.scp_matches[idx].to_ascii_uppercase();
                    if let Some(field) = entry.fields.iter_mut().find(|f| f.field_id == 1) {
                        field.cursor = new_call.len();
                        field.value = new_call.clone();
                    }
                    (
                        new_call,
                        entry.scp_matches.clone(),
                        entry.scp_n1_matches.clone(),
                        entry.scp_cycle_index,
                    )
                };
                let _ = new_call;
                // Skip SCP (we're cycling through existing SCP results)
                // and skip redundant first revalidation.
                recompute_feedback(st, dupe_checker, mult_checker);
                apply_history_only(st, contest, call_history);
                revalidate_after_edit(st, contest);
                let entry = st.focused_entry_mut();
                entry.scp_matches = saved_matches;
                entry.scp_n1_matches = saved_n1;
                entry.scp_cycle_index = saved_index;
                Vec::new()
            }
            Key::Enter => {
                if let Some(effects) = try_frequency_entry(st) {
                    effects
                } else {
                    handle_esm(st, contest, macros)
                }
            }
        },
        AppEvent::EsmTrigger => handle_esm(st, contest, macros),
        AppEvent::BandmapUp { radio: target } | AppEvent::BandmapDown { radio: target } => {
            let is_down = matches!(ev, AppEvent::BandmapDown { .. });

            let radio_state = st.radios.get(&target).filter(|r| r.freq_hz > 0);
            let band = radio_state
                .map(|r| freq_to_band_label(r.freq_hz))
                .unwrap_or("40m");
            let mode = radio_state
                .map(|r| normalize_mode(r.mode.as_str()))
                .unwrap_or("CW");

            let spots = filtered_bandmap_spots(&st.bandmap, band, mode);
            if spots.is_empty() {
                return Vec::new();
            }

            let len = spots.len();
            // For `On(i)` we start from spot `i`. For `Between(i)` — the rig
            // is parked between spots — Down lands on `i` (the spot just
            // above) and Up lands on `i-1` (the spot just below), so we seed
            // as if we were one past the target and let the step fall on
            // the right side naturally.
            let prev_idx: Option<usize> = st.bandmap_cursors.get(&target).map(|c| match *c {
                BandmapCursor::On(i) => i,
                BandmapCursor::Between(i) => {
                    if is_down {
                        (i + len - 1) % len
                    } else {
                        i % len
                    }
                }
            });
            let idx = match (is_down, prev_idx) {
                (true, None) => 0,
                (true, Some(i)) => (i + 1) % len,
                (false, None) => len - 1,
                (false, Some(i)) => (i + len - 1) % len,
            };

            let spot = &spots[idx];
            let freq_hz = spot.freq_hz;
            let call = spot.call.clone();

            st.bandmap_cursors.insert(target, BandmapCursor::On(idx));

            // Temporarily swap focus to target radio so helpers operate on it
            let original_focus = st.focused_radio;
            st.focused_radio = target;
            {
                let entry = st.focused_entry_mut();
                entry.mode = OpMode::Sp;

                if let Some(field) = entry.fields.iter_mut().find(|f| f.field_id == 1) {
                    field.cursor = call.len();
                    field.value = call;
                }

                entry.focus = 0;
                entry.scp_cycle_index = None;
            }

            recompute_feedback(st, dupe_checker, mult_checker);
            apply_history_only(st, contest, call_history);
            revalidate_after_edit(st, contest);
            st.focused_radio = original_focus;

            vec![Effect::RigSet { radio: target, freq_hz }]
        }
    }
}

fn try_frequency_entry(st: &mut AppState) -> Option<Vec<Effect>> {
    let call = st.current_call();
    if call.is_empty() {
        return None;
    }

    // Must be all digits with optional single decimal point
    let mut has_dot = false;
    for ch in call.chars() {
        if ch == '.' {
            if has_dot { return None; }
            has_dot = true;
        } else if !ch.is_ascii_digit() {
            return None;
        }
    }

    let khz: f64 = call.parse().ok()?;
    if !(1800.0..=30000.0).contains(&khz) {
        return None;
    }

    let freq_hz = (khz * 1000.0) as u64;
    let radio = st.focused_radio;

    // Clear the call field
    if let Some(field) = st
        .focused_entry_mut()
        .fields
        .iter_mut()
        .find(|f| f.field_id == 1)
    {
        field.value.clear();
        field.cursor = 0;
    }

    Some(vec![Effect::RigSet { radio, freq_hz }])
}

fn revalidate_after_edit(st: &mut AppState, contest: &dyn ContestEntry) {
    let ctx = EntryContext {
        my_call: st.my_call.clone(),
        my_zone: st.my_zone,
        rst_sent: st.rst_sent.clone(),
        rig: st.radios.get(&st.focused_radio).cloned(),
        serial: st.focused_entry().assigned_serial,
    };
    let validation = contest.validate_entry(st.focused_entry(), &ctx);

    let entry = st.focused_entry_mut();
    for (idx, status) in validation.fields.into_iter().enumerate() {
        if let Some(field) = entry.fields.get_mut(idx) {
            field.status = status;
        }
    }
    entry.overall = validation.overall;
}

fn recompute_feedback(
    st: &mut AppState,
    dupe_checker: &dyn DupeChecker,
    mult_checker: &dyn MultChecker,
) {
    // Compute is_dupe / is_new_mult while we still hold shared borrows of
    // `st.focused_entry()` (via `current_call`) and `st.radios`, then drop
    // them before taking the mut borrow to write the results back.
    let (is_dupe, is_new_mult) = {
        let call_norm = st.current_call();
        if call_norm.is_empty() {
            (false, false)
        } else {
            match st.radios.get(&st.focused_radio) {
                Some(r) => {
                    let band = crate::contest::freq_to_band_label(r.freq_hz);
                    let mode = normalize_mode(&r.mode);
                    (
                        dupe_checker.is_dupe(call_norm, &band, mode),
                        mult_checker.is_new_mult(call_norm, &band, mode),
                    )
                }
                None => (false, false),
            }
        }
    };
    let entry = st.focused_entry_mut();
    entry.is_dupe = is_dupe;
    entry.is_new_mult = is_new_mult;
}

/// Fallback receive-filter width used by the bandmap cursor snap when the
/// rig backend doesn't report one. Conservative defaults roughly matching
/// what most rigs ship with.
fn default_filter_width_hz(mode: &str) -> u32 {
    match mode {
        "CW" => 500,
        "SSB" => 2400,
        "DIGITAL" => 500,
        _ => 3000,
    }
}

/// Recompute the bandmap cursor for `radio` based on its current freq_hz,
/// mode, and filter width. If any spot falls inside the rig's passband,
/// snap to the nearest one (`On`); otherwise record the insertion index
/// so the renderer can draw a divider between the flanking spots
/// (`Between`). No-op when the filtered spot list is empty — leaves any
/// existing cursor alone on transient out-of-band excursions.
fn snap_bandmap_cursor_to_freq(st: &mut AppState, radio: RadioId) {
    let Some(rs) = st.radios.get(&radio) else {
        return;
    };
    if rs.freq_hz == 0 {
        return;
    }
    let band = freq_to_band_label(rs.freq_hz);
    let mode = normalize_mode(&rs.mode);
    let target = rs.freq_hz;
    let half_width = u64::from(
        rs.filter_width_hz
            .unwrap_or_else(|| default_filter_width_hz(mode)),
    ) / 2;

    let spots = filtered_bandmap_spots(&st.bandmap, band, mode);
    if spots.is_empty() {
        return;
    }

    let pos = spots.partition_point(|s| s.freq_hz < target);
    let nearest = match (pos == 0, pos == spots.len()) {
        (true, _) => 0,
        (_, true) => spots.len() - 1,
        _ => {
            let lo = pos - 1;
            let dlo = spots[lo].freq_hz.abs_diff(target);
            let dhi = spots[pos].freq_hz.abs_diff(target);
            if dhi < dlo { pos } else { lo }
        }
    };

    let cursor = if spots[nearest].freq_hz.abs_diff(target) <= half_width {
        BandmapCursor::On(nearest)
    } else {
        BandmapCursor::Between(pos)
    };
    st.bandmap_cursors.insert(radio, cursor);
}

fn recompute_passband_warning(st: &mut AppState) {
    if !st.show_passband_qrm {
        st.focused_entry_mut().is_passband_qrm = false;
        return;
    }
    let entry_mode = st.focused_entry().mode;
    if entry_mode != OpMode::Run {
        st.focused_entry_mut().is_passband_qrm = false;
        return;
    }
    // Scan the bandmap with shared borrows only, then drop them before
    // writing `is_passband_qrm` back. No clones of `mode` or `my_call`.
    // Half-width comes from the rig's reported filter width, falling back
    // to a mode-dependent default when the backend doesn't report one.
    let found = {
        let Some(r) = st.radios.get(&st.focused_radio) else {
            st.focused_entry_mut().is_passband_qrm = false;
            return;
        };
        let freq = r.freq_hz;
        let radio_mode = normalize_mode(r.mode.as_str());
        let half_w = u64::from(
            r.filter_width_hz
                .unwrap_or_else(|| default_filter_width_hz(radio_mode)),
        ) / 2;
        let my_call = st.my_call.as_str();
        st.bandmap.iter().any(|s| {
            s.call != my_call
                && normalize_mode(s.mode.as_str()) == radio_mode
                && s.freq_hz.abs_diff(freq) <= half_w
        })
    };
    st.focused_entry_mut().is_passband_qrm = found;
}

fn apply_call_history(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    call_history: &dyn CallHistoryLookup,
    scp: &dyn ScpLookup,
) {
    // Capture `current_call` into scp lookups while the borrow is alive,
    // then drop it before mutating the entry state.
    let (partial, n_plus_one) = {
        let call_norm = st.current_call();
        if call_norm.is_empty() {
            clear_history_fields(st);
            return;
        }
        (
            scp.partial_matches(call_norm, 10),
            scp.n_plus_one_matches(call_norm, 10),
        )
    };

    // Update SCP matches
    {
        let entry = st.focused_entry_mut();
        entry.scp_matches = partial;
        entry.scp_n1_matches = n_plus_one;
    }

    apply_history_lookup(st, contest, call_history);
}

/// Call history lookup only — no SCP search. Used by bandmap navigation
/// and SCP cycle where the callsign is already known/complete.
fn apply_history_only(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    call_history: &dyn CallHistoryLookup,
) {
    if st.current_call().is_empty() {
        clear_history_fields(st);
        return;
    }

    apply_history_lookup(st, contest, call_history);
}

fn apply_history_lookup(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    call_history: &dyn CallHistoryLookup,
) {
    // Perform the lookup while the borrow from `current_call` is alive;
    // the `Option<Vec<..>>` return is fully owned so the borrow can be
    // dropped before any mutation below.
    let maybe_pairs = call_history.lookup(st.current_call());
    let Some(pairs) = maybe_pairs else {
        for field in &mut st.focused_entry_mut().fields {
            if field.from_history {
                field.value.clear();
                field.cursor = 0;
                field.from_history = false;
            }
        }
        return;
    };

    let mapping = contest.history_field_mapping();
    let pairs_map: std::collections::HashMap<&str, &str> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    for (col_name, field_id) in &mapping {
        if let Some(value) = pairs_map.get(col_name) {
            if let Some(field) = st
                .focused_entry_mut()
                .fields
                .iter_mut()
                .find(|f| f.field_id == *field_id)
            {
                if field.value.is_empty() || field.from_history {
                    field.value = if value.chars().all(|c| c.is_ascii_digit()) {
                        value.to_string()
                    } else {
                        value.to_ascii_uppercase()
                    };
                    field.cursor = field.value.len();
                    field.from_history = true;
                }
            }
        }
    }
}

fn clear_history_fields(st: &mut AppState) {
    let entry = st.focused_entry_mut();
    for field in &mut entry.fields {
        if field.from_history {
            field.value.clear();
            field.cursor = 0;
            field.from_history = false;
        }
    }
    entry.scp_matches.clear();
    entry.scp_n1_matches.clear();
}


#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        contest::contest_from_id,
        contest::traits::ContestEntry,
        effects::Effect,
        entry::state::{EntryState, EsmStep, OpMode, Validation},
        events::{AppEvent, Key},
        reducer::{
            DupeChecker, MultChecker, NoCallHistory, NoDupeChecker, NoMultChecker, NoScp,
        },
        state::{AppState, EsmPolicy, Macros, Spot},
    };

    fn reduce(
        st: &mut AppState,
        contest: &dyn ContestEntry,
        macros: &Macros,
        ev: AppEvent,
    ) -> Vec<Effect> {
        crate::reducer::reduce(
            st,
            contest,
            macros,
            &NoDupeChecker,
            &NoMultChecker,
            &NoCallHistory,
            &NoScp,
            ev,
        )
    }

    struct MatchDupeChecker;

    impl DupeChecker for MatchDupeChecker {
        fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool {
            call_norm == "K5ZD" && band == "20m" && mode == "CW"
        }
    }

    struct MatchMultChecker;

    impl MultChecker for MatchMultChecker {
        fn is_new_mult(&self, call_norm: &str, band: &str, mode: &str) -> bool {
            call_norm == "DL1ABC" && band == "20m" && mode == "CW"
        }
    }

    fn mk_state() -> AppState {
        let contest = contest_from_id("cqww").unwrap();
        let mut entries = HashMap::new();
        entries.insert(1, EntryState::from_spec(&contest.form_spec()));
        entries.insert(2, EntryState::from_spec(&contest.form_spec()));
        AppState {
            now_ms: 0,
            focused_radio: 1,
            active_operator: 1,
            radios: HashMap::new(),
            entries,
            bandmap: Vec::new(),
            last_logged: None,
            my_call: "N0CALL".to_string(),
            my_zone: 4,
            rst_sent: "599".to_string(),
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursors: HashMap::new(),
            default_cw_speed: 28,
            serial_counter: None,
            show_passband_qrm: false,
            bandmap_version: 0,
        }
    }

    #[test]
    fn space_focus_wraps() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );

        assert_eq!(st.focused_entry_mut().focus, 0);
    }

    #[test]
    fn validation_updates_per_field_status() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "59".to_string(),
            },
        );

        assert_eq!(st.focused_entry_mut().fields[0].status, Validation::Valid);
        assert_eq!(st.focused_entry_mut().fields[1].status, Validation::Valid);
        assert!(st.focused_entry_mut().fields[2].status.is_invalid());
    }

    #[test]
    fn editing_resets_esm_step() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        st.focused_entry_mut().esm_step = EsmStep::ExchSent;
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "K".to_string() },
        );

        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::Idle);
    }

    #[test]
    fn run_two_step_state_transition() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "5".to_string() },
        );

        let effects1 = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::ExchSent);
        assert!(effects1.iter().any(|e| matches!(e, Effect::CwSend { .. })));

        let effects2 = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::Idle);
        assert!(
            effects2
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );
    }

    #[test]
    fn sp_three_step_esm() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Sp;

        // Type call
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );

        // Enter 1: send MYCALL, step → CallSent
        let effects1 = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::CallSent);
        assert!(
            effects1
                .iter()
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text == "N0CALL"))
        );

        // Fill exchange
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "5".to_string() },
        );

        // Enter 2: send exchange (sp_exch, no callsign), step → ExchSent
        let effects2 = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::ExchSent);
        assert!(
            effects2
                .iter()
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text == "599 4"))
        );
        assert!(
            !effects2
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );

        // Enter 3: log silently, no CW
        let effects3 = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::Idle);
        assert!(
            effects3
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );
        assert!(
            !effects3
                .iter()
                .any(|e| matches!(e, Effect::CwSend { .. }))
        );
    }

    #[test]
    fn run_enter_with_call_sends_exchange() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        let effects = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );

        // In RUN mode, Enter sends exchange immediately (no validation gate)
        assert!(effects.iter().any(|e| matches!(e, Effect::CwSend { .. })));
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::ExchSent);
    }

    #[test]
    fn space_advances_without_inserting_literal_space() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );

        assert_eq!(st.focused_entry_mut().focus, 1);
        assert_eq!(st.focused_entry_mut().fields[0].value, "K1ABC");
        assert!(st.focused_entry_mut().fields[0].value.chars().all(|c| c != ' '));
    }

    #[test]
    fn dupe_recomputes_on_call_edit_and_focused_rig_changes() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        crate::reducer::reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            &MatchDupeChecker,
            &NoMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::TextInput {
                s: "K5ZD".to_string(),
            },
        );
        assert!(!st.focused_entry_mut().is_dupe);

        crate::reducer::reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            &MatchDupeChecker,
            &NoMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::RigStatus {
                radio: 1,
                freq_hz: 14_025_000,
                mode: "CW".to_string(),
                is_ptt: false,
                filter_width_hz: None,
            },
        );
        assert!(st.focused_entry_mut().is_dupe);

        crate::reducer::reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            &MatchDupeChecker,
            &NoMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::FocusRadio { radio: 2 },
        );
        assert!(!st.focused_entry_mut().is_dupe);
    }

    #[test]
    fn mult_recomputes_on_call_and_focus_context_changes() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        crate::reducer::reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            &NoDupeChecker,
            &MatchMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::TextInput {
                s: "DL1ABC".to_string(),
            },
        );
        assert!(!st.focused_entry_mut().is_new_mult);

        crate::reducer::reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            &NoDupeChecker,
            &MatchMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::RigStatus {
                radio: 1,
                freq_hz: 14_025_000,
                mode: "CW".to_string(),
                is_ptt: false,
                filter_width_hz: None,
            },
        );
        assert!(st.focused_entry_mut().is_new_mult);

        crate::reducer::reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            &NoDupeChecker,
            &MatchMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::FocusRadio { radio: 2 },
        );
        assert!(!st.focused_entry_mut().is_new_mult);
    }

    #[test]
    fn run_exchsent_logs_without_resending_exch() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "5".to_string() },
        );

        let _ = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        let effects = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text.starts_with("TU ")))
        );
        assert!(!effects.iter().any(|e| {
            matches!(e, Effect::CwSend { text, .. } if text.contains("599 4") && text.contains("K1ABC"))
        }));
    }

    #[test]
    fn run_edit_received_exch_does_not_reset_esm() {
        // Editing the received exchange fields should NOT reset ESM —
        // only editing the CALL field should force a resend.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "05".to_string(),
            },
        );

        let _ = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::ExchSent);

        // Edit zone field (received exchange) — ESM should stay ExchSent
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress {
                key: Key::Backspace,
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "4".to_string() },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::ExchSent);

        // Enter should log (not resend)
        let effects = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::LogInsert { .. })));
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::CwSend { text, .. } if text.starts_with("TU "))));
    }

    #[test]
    fn run_edit_call_after_exch_sent_resets_esm() {
        // Editing the CALL field after exchange sent should force a resend.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput {
                s: "05".to_string(),
            },
        );

        let _ = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::ExchSent);

        // Move focus back to call field and edit it
        st.focused_entry_mut().focus = 0;
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress {
                key: Key::Backspace,
            },
        );
        assert_eq!(st.focused_entry_mut().esm_step, EsmStep::Idle);

        // Enter should resend (not log)
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "D".to_string() },
        );
        let effects = reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::CwSend { .. })));
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::LogInsert { .. })));
    }

    #[test]
    fn passband_qrm_warning() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        st.show_passband_qrm = true;
        let macros = Macros::default();

        // Set up radio on a known frequency, Run mode, with a 500 Hz CW
        // filter — matching half-width (250) preserves the pre-rename
        // pass/fail boundary of this test's spot placements.
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::RigStatus {
                radio: 1,
                freq_hz: 14_025_000,
                mode: "CW".to_string(),
                is_ptt: false,
                filter_width_hz: Some(500),
            },
        );
        assert!(!st.focused_entry().is_passband_qrm);

        // Spot within passband triggers warning
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotReceived {
                spot: Spot {
                    call: "K5ZD".to_string(),
                    freq_hz: 14_025_100,
                    mode: "CW".to_string(),
                },
            },
        );
        assert!(st.focused_entry().is_passband_qrm);

        // Spot outside passband does not trigger alone
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotWithdrawn {
                call: "K5ZD".to_string(),
            },
        );
        assert!(!st.focused_entry().is_passband_qrm);

        // Spot outside passband (too far)
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotReceived {
                spot: Spot {
                    call: "DL1ABC".to_string(),
                    freq_hz: 14_030_000,
                    mode: "CW".to_string(),
                },
            },
        );
        assert!(!st.focused_entry().is_passband_qrm);

        // Own call in passband is excluded
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotReceived {
                spot: Spot {
                    call: "N0CALL".to_string(),
                    freq_hz: 14_025_000,
                    mode: "CW".to_string(),
                },
            },
        );
        assert!(!st.focused_entry().is_passband_qrm);

        // S&P mode suppresses warning
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotReceived {
                spot: Spot {
                    call: "W1AW".to_string(),
                    freq_hz: 14_025_050,
                    mode: "CW".to_string(),
                },
            },
        );
        assert!(st.focused_entry().is_passband_qrm);
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SetOpMode {
                mode: OpMode::Sp,
            },
        );
        assert!(!st.focused_entry().is_passband_qrm);
    }

    fn add_spot(st: &mut AppState, call: &str, freq_hz: u64, mode: &str) {
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotReceived {
                spot: Spot {
                    call: call.to_string(),
                    freq_hz,
                    mode: mode.to_string(),
                },
            },
        );
    }

    fn rig_status(
        st: &mut AppState,
        radio: u8,
        freq_hz: u64,
        mode: &str,
        filter_width_hz: Option<u32>,
    ) {
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            st,
            contest.as_ref(),
            &macros,
            AppEvent::RigStatus {
                radio,
                freq_hz,
                mode: mode.to_string(),
                is_ptt: false,
                filter_width_hz,
            },
        );
    }

    #[test]
    fn bandmap_snap_inside_passband_highlights_spot() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
    }

    #[test]
    fn bandmap_snap_outside_passband_shows_between() {
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_000, "CW", Some(500));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::Between(1))
        );
    }

    #[test]
    fn bandmap_snap_tie_prefers_lower_freq() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_026_000, "CW");
        // With a wide 2400 Hz filter both spots are inside the passband;
        // the tie-break prefers the lower-frequency spot (index 0).
        rig_status(&mut st, 1, 14_025_500, "CW", Some(2400));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
    }

    #[test]
    fn bandmap_snap_unchanged_on_same_freq() {
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        // Manual navigation: move cursor up (wraps to last spot).
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::BandmapUp { radio: 1 },
        );
        let manual = st.bandmap_cursors.get(&1).copied();
        assert!(matches!(manual, Some(crate::state::BandmapCursor::On(_))));
        // Another RigStatus at the *same* freq — BandmapUp's RigSet effect
        // isn't applied in unit tests, so the rig freq is unchanged. The
        // "freq didn't change" guard must preserve the manual cursor.
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        assert_eq!(st.bandmap_cursors.get(&1).copied(), manual);
    }

    #[test]
    fn bandmap_snap_band_change() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 7_025_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
        // Move to 40m — filtered list becomes just the 40m spot, at index 0
        // in that list.
        rig_status(&mut st, 1, 7_025_100, "CW", Some(500));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
    }

    #[test]
    fn bandmap_snap_empty_filter_preserves_cursor() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        let before = st.bandmap_cursors.get(&1).copied();
        assert!(before.is_some());
        // 15m has no spots; snap should leave the cursor alone.
        rig_status(&mut st, 1, 21_025_000, "CW", Some(500));
        assert_eq!(st.bandmap_cursors.get(&1).copied(), before);
    }

    #[test]
    fn bandmap_snap_per_radio_independent() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        rig_status(&mut st, 2, 14_027_050, "CW", Some(500));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
        assert_eq!(
            st.bandmap_cursors.get(&2).copied(),
            Some(crate::state::BandmapCursor::On(1))
        );
    }

    #[test]
    fn bandmap_snap_falls_back_to_mode_default_when_no_filter() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        // No filter_width_hz reported. CW fallback is 500 Hz, so half-width
        // is 250. A 200 Hz delta should still be "On".
        rig_status(&mut st, 1, 14_025_200, "CW", None);
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
    }

    #[test]
    fn bandmap_snap_normalizes_mode_usb_to_ssb() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_200_000, "SSB");
        // Rig reports "USB" — must normalize to "SSB" before filtering or
        // the spot list is empty and the snap is a no-op.
        rig_status(&mut st, 1, 14_200_500, "USB", Some(2400));
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
    }

    #[test]
    fn bandmap_up_normalizes_mode_usb_to_ssb() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_200_000, "SSB");
        // Seed the rig on 20m USB so BandmapUp finds the band+mode.
        rig_status(&mut st, 1, 14_200_000, "USB", Some(2400));
        // Clear the auto-snap cursor so we can observe BandmapUp in isolation.
        st.bandmap_cursors.clear();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::BandmapUp { radio: 1 },
        );
        assert_eq!(
            st.bandmap_cursors.get(&1).copied(),
            Some(crate::state::BandmapCursor::On(0))
        );
    }
}
