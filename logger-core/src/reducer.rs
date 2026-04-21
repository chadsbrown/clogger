use crate::{
    contest::{
        filtered_bandmap_spots, freq_to_band_label, normalize_mode,
        traits::{ContestEntry, EntryContext},
    },
    effects::Effect,
    entry::{
        esm::{handle_esm, quick_log},
        state::{EsmStep, OpMode},
    },
    events::{AppEvent, Key},
    macro_expand::expand_macro,
    state::{AppState, BandmapCursor, Macros, RadioId, RadioState, Spot},
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

pub trait CallHistoryLookup: Send + Sync {
    /// Exact match. Returns .ch column-name/value pairs, e.g. [("CqZone", "5")].
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>>;
}

pub struct NoCallHistory;

impl CallHistoryLookup for NoCallHistory {
    fn lookup(&self, _: &str) -> Option<Vec<(String, String)>> {
        None
    }
}

/// Auto-populate source for exchange fields from *this* contest's
/// previously-logged QSOs. Complements `CallHistoryLookup` (which pulls
/// pre-contest `.ch` data) by preferring live contest data when
/// available — e.g. the station sent a different name than the `.ch`
/// file has, or isn't in the `.ch` file at all.
///
/// Returns exchange pairs keyed by **contest-engine spec field id**
/// (e.g. `"name"`, `"loc"`). The reducer maps spec ids to form field
/// ids by matching on the uppercased label (`SpecDrivenContest` sets
/// `label = spec_id.to_ascii_uppercase()`).
///
/// No `Send + Sync` bound: the concrete implementor in production is
/// `LogAdapter`, which transitively owns a SQLite connection and thus
/// isn't `Sync`. The reducer only holds a `&dyn ContestHistoryLookup`
/// for the duration of one synchronous `reduce()` call — no
/// cross-thread sharing of the reference.
pub trait ContestHistoryLookup {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>>;
}

pub struct NoContestHistory;

impl ContestHistoryLookup for NoContestHistory {
    fn lookup(&self, _: &str) -> Option<Vec<(String, String)>> {
        None
    }
}

pub trait ScpLookup: Send + Sync {
    fn partial_matches(&self, prefix: &str, limit: usize) -> Vec<String>;
    fn n_plus_one_matches(&self, _call: &str, _limit: usize) -> Vec<String> {
        Vec::new()
    }
    /// Exact-match membership check. Used by the dxfeed enrichment resolver
    /// to drop spots whose callsign isn't in the contest master database.
    /// Default `false` is a safe no-op for `NoScp`.
    fn contains(&self, _call: &str) -> bool {
        false
    }
}

pub struct NoScp;

impl ScpLookup for NoScp {
    fn partial_matches(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn reduce(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    macros: &Macros,
    dupe_checker: &dyn DupeChecker,
    mult_checker: &dyn MultChecker,
    call_history: &dyn CallHistoryLookup,
    contest_history: &dyn ContestHistoryLookup,
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
            // Drop self-spots before they touch the bandmap. A spot for
            // the operator's own call is never a workable target, and
            // if it slipped past upstream filters it would anchor the
            // bandmap cursor on `my_call` and make nav feel stuck —
            // every keypress would spend a hop landing on yourself.
            if spot.call.trim().eq_ignore_ascii_case(&st.my_call) {
                return Vec::new();
            }
            st.bandmap.push(spot);
            st.bandmap_version = st.bandmap_version.wrapping_add(1);
            snap_all_bandmap_cursors(st);
            recompute_passband_warning(st);
            Vec::new()
        }
        AppEvent::SpotWithdrawn { call } => {
            st.bandmap.retain(|s| s.call != call);
            st.bandmap_version = st.bandmap_version.wrapping_add(1);
            snap_all_bandmap_cursors(st);
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
                if !touched_call && field.from_history {
                    // First real keystroke into an auto-populated field: drop
                    // the autopop value so operator's typing replaces it
                    // cleanly. Without this, typing "14" on top of an
                    // autopop'd "14" would yield "1414". Applies to both
                    // call-history and contest-history fills (both use the
                    // same `from_history` flag).
                    field.value.clear();
                    field.cursor = 0;
                }
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
                apply_call_history(st, contest, call_history, contest_history, scp);
                revalidate_after_edit(st, contest);
            }
            Vec::new()
        }
        AppEvent::KeyPress { key } => match key {
            Key::Tab => {
                let entry = st.focused_entry_mut();
                if !entry.fields.is_empty() {
                    entry.focus = (entry.focus + 1) % entry.fields.len();
                }
                Vec::new()
            }
            Key::Space => {
                let rst_id = contest.rst_field_id();
                let entry = st.focused_entry_mut();
                if !entry.fields.is_empty() {
                    entry.focus = (entry.focus + 1) % entry.fields.len();
                    // Skip the auto-populated RST field. Tab is the way to land on it.
                    if let Some(rst_id) = rst_id
                        && entry.fields.get(entry.focus).map(|f| f.field_id) == Some(rst_id) {
                            entry.focus = (entry.focus + 1) % entry.fields.len();
                        }
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
                    apply_call_history(st, contest, call_history, contest_history, scp);
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
                if let Some(field) = st.focused_entry_mut().focused_mut()
                    && field.cursor < field.value.len() {
                        field.cursor += 1;
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
            Key::F4 => vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro("{MYCALL}", st),
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
            Key::CtrlAltF1 | Key::CtrlAltF2 | Key::CtrlAltF3 | Key::CtrlAltF4
            | Key::CtrlAltF5 | Key::CtrlAltF6 | Key::CtrlAltF7 | Key::CtrlAltF8
            | Key::CtrlAltF9 | Key::CtrlAltF10 | Key::CtrlAltF11 | Key::CtrlAltF12 => {
                let text = match key {
                    Key::CtrlAltF1 => &macros.ctrl_alt_f1,
                    Key::CtrlAltF2 => &macros.ctrl_alt_f2,
                    Key::CtrlAltF3 => &macros.ctrl_alt_f3,
                    Key::CtrlAltF4 => &macros.ctrl_alt_f4,
                    Key::CtrlAltF5 => &macros.ctrl_alt_f5,
                    Key::CtrlAltF6 => &macros.ctrl_alt_f6,
                    Key::CtrlAltF7 => &macros.ctrl_alt_f7,
                    Key::CtrlAltF8 => &macros.ctrl_alt_f8,
                    Key::CtrlAltF9 => &macros.ctrl_alt_f9,
                    Key::CtrlAltF10 => &macros.ctrl_alt_f10,
                    Key::CtrlAltF11 => &macros.ctrl_alt_f11,
                    Key::CtrlAltF12 => &macros.ctrl_alt_f12,
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
                let mode = st
                    .radios
                    .get(&st.focused_radio)
                    .map(|r| normalize_mode(r.mode.as_str()))
                    .unwrap_or("CW");
                let entry = st.focused_entry_mut();
                entry.clear_values();
                entry.esm_step = EsmStep::Idle;
                entry.scp_matches.clear();
                entry.scp_n1_matches.clear();
                entry.scp_cycle_index = None;
                // `clear_values` wipes the RST field along with everything
                // else. Without this reapply, a later bandmap-nav autopopulate
                // (which seeds the call but not RST) would leave the entry
                // ready to log a QSO with blank RST.
                crate::entry::esm::apply_default_rst(entry, contest, mode);
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
                apply_history_only(st, contest, call_history, contest_history);
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
        AppEvent::QuickLog => quick_log(st, contest, macros),
        AppEvent::EsmTrigger => handle_esm(st, contest, macros),
        AppEvent::BandmapUp { radio: target } | AppEvent::BandmapDown { radio: target } => {
            let raw_is_down = matches!(ev, AppEvent::BandmapDown { .. });
            // When the bandmap renders high-freq-at-top, the operator's
            // "visually down" is a lower frequency. The cursor-step math
            // (Some / Between branches and the skip-worked loop) needs the
            // flipped direction so the step lands visually below/above the
            // anchor. The no-cursor case stays on `raw_is_down`: the
            // existing convention (Down→lowest, Up→highest) already aligns
            // visually in reversed mode (lowest is at the bottom, highest
            // at the top) and matches operator muscle memory.
            let is_down = if st.bandmap_high_at_top { !raw_is_down } else { raw_is_down };

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
            // Re-resolve the anchor every step so list churn (new spots,
            // withdrawn spots, re-sorting) doesn't drift us off the call
            // we last navigated to. `On.call` anchors by call; `Between`
            // seeds the step as if we were at the insertion point.
            let prev_idx: Option<usize> = match st.bandmap_cursors.get(&target) {
                Some(BandmapCursor::On { call, .. }) => {
                    spots.iter().position(|s| &s.call == call)
                }
                Some(BandmapCursor::Between { freq_hz }) => {
                    let pos = spots.partition_point(|s| s.freq_hz < *freq_hz);
                    if is_down {
                        Some((pos + len - 1) % len)
                    } else {
                        Some(pos % len)
                    }
                }
                None => None,
            };
            let first_idx = match (raw_is_down, prev_idx) {
                // No cursor: use the raw key direction so muscle memory
                // ("Down → lowest", "Up → highest") is preserved across
                // both display orientations.
                (true, None) => 0,
                (false, None) => len - 1,
                // Stepping from a known cursor: use `is_down`, which is
                // visually-flipped in reversed mode so the step lands on
                // the correct neighbor.
                (_, Some(i)) => {
                    if is_down {
                        (i + 1) % len
                    } else {
                        (i + len - 1) % len
                    }
                }
            };

            // Optional skip-worked mode: step past consecutive dupes
            // until we land on an un-worked spot. Stops if we'd wrap
            // all the way back to `first_idx` without finding one —
            // in that case every visible spot is worked, so the nav
            // key becomes a no-op (no cursor/rig change).
            let idx = if st.bandmap_skip_worked {
                let mut i = first_idx;
                let mut steps = 0usize;
                loop {
                    let s = &spots[i];
                    let call_up = s.call.to_ascii_uppercase();
                    if !dupe_checker.is_dupe(&call_up, band, mode) {
                        break Some(i);
                    }
                    steps += 1;
                    if steps >= len {
                        break None;
                    }
                    i = if is_down {
                        (i + 1) % len
                    } else {
                        (i + len - 1) % len
                    };
                }
            } else {
                Some(first_idx)
            };

            let Some(idx) = idx else {
                // Every spot on this band+mode is a dupe. Leave the
                // cursor, rig, and entry untouched so the operator
                // sees that nothing moved.
                return Vec::new();
            };

            let spot = &spots[idx];
            let freq_hz = spot.freq_hz;
            let call = spot.call.clone();

            select_bandmap_spot(
                st, target, call, freq_hz, contest, dupe_checker, mult_checker,
                call_history, contest_history,
            )
        }
        AppEvent::BandmapSelect { radio: target, call, freq_hz } => {
            select_bandmap_spot(
                st, target, call, freq_hz, contest, dupe_checker, mult_checker,
                call_history, contest_history,
            )
        }
        AppEvent::CwSpeedAdjust { delta } => {
            // Range is the conservative contest-CW window. Widen if a user
            // legitimately runs outside it; WK3 itself goes 5–99 WPM.
            const MIN_WPM: u8 = 5;
            const MAX_WPM: u8 = 60;

            let radio = st.focused_radio;
            let current = st
                .radios
                .get(&radio)
                .map(|r| r.cw_speed)
                .unwrap_or(st.default_cw_speed);
            let next = ((current as i16) + delta as i16)
                .clamp(MIN_WPM as i16, MAX_WPM as i16) as u8;
            if next == current {
                return Vec::new();
            }

            if let Some(r) = st.radios.get_mut(&radio) {
                r.cw_speed = next;
            } else {
                // No rig connected yet — stash the new speed as the session
                // default so the value sticks once RigStatus seeds RadioState.
                st.default_cw_speed = next;
            }

            vec![Effect::KeyerSetSpeed { wpm: next }]
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

/// Select a specific bandmap spot for `target` radio: pin the cursor to it,
/// populate the CALL field, switch to S&P, autofill from history, and emit
/// a `RigSet` for the spot's frequency. Shared by `BandmapUp/Down` (which
/// pick a spot relative to the current cursor) and `BandmapSelect` (which
/// is given the call+freq directly — e.g. from a GUI bandmap click).
#[allow(clippy::too_many_arguments)]
fn select_bandmap_spot(
    st: &mut AppState,
    target: RadioId,
    call: String,
    freq_hz: u64,
    contest: &dyn ContestEntry,
    dupe_checker: &dyn DupeChecker,
    mult_checker: &dyn MultChecker,
    call_history: &dyn CallHistoryLookup,
    contest_history: &dyn ContestHistoryLookup,
) -> Vec<Effect> {
    st.bandmap_cursors.insert(
        target,
        BandmapCursor::On {
            call: call.clone(),
            freq_hz,
        },
    );

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

        let mode = st
            .radios
            .get(&target)
            .map(|r| normalize_mode(r.mode.as_str()))
            .unwrap_or("CW");
        let entry = st.focused_entry_mut();
        crate::entry::esm::apply_default_rst(entry, contest, mode);
    }

    recompute_feedback(st, dupe_checker, mult_checker);
    apply_history_only(st, contest, call_history, contest_history);
    revalidate_after_edit(st, contest);
    st.focused_radio = original_focus;

    vec![Effect::RigSet { radio: target, freq_hz }]
}

fn revalidate_after_edit(st: &mut AppState, contest: &dyn ContestEntry) {
    let ctx = EntryContext {
        my_call: st.my_call.clone(),
        my_zone: st.my_zone,
        rst_sent: st.rst_sent.clone(),
        rig: st.radios.get(&st.focused_radio).cloned(),
        serial: st.focused_entry().assigned_serial,
        station_config: st.station_config.clone(),
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
                        dupe_checker.is_dupe(call_norm, band, mode),
                        mult_checker.is_new_mult(call_norm, band, mode),
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

    // Preserve the caller's call-anchored cursor when the rig's new freq
    // already matches the anchor and the anchored call is still present.
    // Without this short-circuit, `partition_point` below collapses a
    // cluster of co-located spots to its first member — breaking Ctrl+Up
    // navigation through spots at the same frequency (and click-to-select
    // when multiple spots share a freq).
    if let Some(BandmapCursor::On { call, freq_hz }) = st.bandmap_cursors.get(&radio)
        && *freq_hz == target && spots.iter().any(|s| &s.call == call) {
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
        BandmapCursor::On {
            call: spots[nearest].call.clone(),
            freq_hz: spots[nearest].freq_hz,
        }
    } else {
        // Drop `pos` — we now anchor on the rig's actual freq instead of
        // a list insertion point, so render recomputes `pos` every time.
        let _ = pos;
        BandmapCursor::Between { freq_hz: target }
    };
    st.bandmap_cursors.insert(radio, cursor);
}

/// Called when the bandmap itself changes (spot added/withdrawn). The
/// user's navigation cursor is call-anchored, so list churn alone
/// doesn't move it: a stable `On { call }` stays put. Two cases need
/// action:
///   - anchored call was just withdrawn → demote to `Between` at its
///     last known freq so the UI keeps a meaningful position.
///   - cursor is `Between` and a new spot now falls inside the rig's
///     passband → upgrade to `On` so the operator sees that a spot
///     just appeared on their current freq (rig-follow feedback).
fn snap_all_bandmap_cursors(st: &mut AppState) {
    let radios: Vec<RadioId> = st.radios.keys().copied().collect();
    for radio in radios {
        let Some(cursor) = st.bandmap_cursors.get(&radio).cloned() else {
            continue;
        };
        let rs = match st.radios.get(&radio) {
            Some(r) if r.freq_hz > 0 => r,
            _ => continue,
        };
        let band = freq_to_band_label(rs.freq_hz);
        let mode = normalize_mode(&rs.mode);
        let target = rs.freq_hz;
        let half_width = u64::from(
            rs.filter_width_hz
                .unwrap_or_else(|| default_filter_width_hz(mode)),
        ) / 2;
        let spots = filtered_bandmap_spots(&st.bandmap, band, mode);

        match cursor {
            BandmapCursor::On { call, freq_hz } => {
                if !spots.iter().any(|s| s.call == call) {
                    st.bandmap_cursors
                        .insert(radio, BandmapCursor::Between { freq_hz });
                }
            }
            BandmapCursor::Between { freq_hz } => {
                // If a spot now sits within the rig's passband, adopt it.
                if let Some(spot) = nearest_spot_within(&spots, target, half_width) {
                    st.bandmap_cursors.insert(
                        radio,
                        BandmapCursor::On {
                            call: spot.call.clone(),
                            freq_hz: spot.freq_hz,
                        },
                    );
                } else {
                    // Nothing nearby — keep the Between freq as-is (render
                    // computes the insertion point from freq_hz directly).
                    let _ = freq_hz;
                }
            }
        }
    }
}

fn nearest_spot_within(
    spots: &[Spot],
    target: u64,
    half_width: u64,
) -> Option<&Spot> {
    if spots.is_empty() {
        return None;
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
    if spots[nearest].freq_hz.abs_diff(target) <= half_width {
        Some(&spots[nearest])
    } else {
        None
    }
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
    contest_history: &dyn ContestHistoryLookup,
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

    apply_history_lookup(st, contest, call_history, contest_history);
}

/// History lookup only — no SCP search. Used by bandmap navigation
/// and SCP cycle where the callsign is already known/complete.
fn apply_history_only(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    call_history: &dyn CallHistoryLookup,
    contest_history: &dyn ContestHistoryLookup,
) {
    if st.current_call().is_empty() {
        clear_history_fields(st);
        return;
    }

    apply_history_lookup(st, contest, call_history, contest_history);
}

fn apply_history_lookup(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    call_history: &dyn CallHistoryLookup,
    contest_history: &dyn ContestHistoryLookup,
) {
    // Own the callsign — the lookup returns owned data, so we don't need
    // the borrow past these two calls and can mutate entry fields below.
    let current_call = st.current_call().to_string();
    let contest_pairs = contest_history.lookup(&current_call);
    let call_pairs = call_history.lookup(&current_call);

    // Neither source has anything for this call — clear any fields still
    // tagged `from_history` from a prior lookup.
    if contest_pairs.is_none() && call_pairs.is_none() {
        for field in &mut st.focused_entry_mut().fields {
            if field.from_history {
                field.value.clear();
                field.cursor = 0;
                field.from_history = false;
            }
        }
        return;
    }

    // Contest-history pass first. Keys are contest-engine spec field ids
    // (e.g. `"name"`, `"loc"`); form fields carry the uppercased spec id
    // as their `label` (see `SpecDrivenContest::form_spec`), so we match
    // there. Track which field_ids this pass filled so the call-history
    // pass below doesn't clobber them.
    let mut filled_by_contest: std::collections::HashSet<u16> = std::collections::HashSet::new();
    if let Some(pairs) = contest_pairs {
        for (spec_id, value) in &pairs {
            let target = spec_id.to_ascii_uppercase();
            if let Some(field) = st
                .focused_entry_mut()
                .fields
                .iter_mut()
                .find(|f| f.label == target)
                && (field.value.is_empty() || field.from_history) {
                    field.value = normalize_history_value(value);
                    field.cursor = field.value.len();
                    field.from_history = true;
                    filled_by_contest.insert(field.field_id);
                }
        }
    }

    // Call-history pass fills remaining gaps. Same field-overwrite gate
    // as before (`is_empty || from_history`), but we also skip any field
    // just filled by contest-history — contest-history is authoritative
    // for the current contest.
    if let Some(pairs) = call_pairs {
        let mapping = contest.history_field_mapping();
        let pairs_map: std::collections::HashMap<&str, &str> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        for (col_name, field_id) in &mapping {
            if filled_by_contest.contains(field_id) {
                continue;
            }
            if let Some(value) = pairs_map.get(col_name)
                && let Some(field) = st
                    .focused_entry_mut()
                    .fields
                    .iter_mut()
                    .find(|f| f.field_id == *field_id)
                    && (field.value.is_empty() || field.from_history) {
                        field.value = normalize_history_value(value);
                        field.cursor = field.value.len();
                        field.from_history = true;
                    }
        }
    }
}

fn normalize_history_value(v: &str) -> String {
    if v.chars().all(|c| c.is_ascii_digit()) {
        v.to_string()
    } else {
        v.to_ascii_uppercase()
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
        state::BandmapCursor,
        reducer::{
            DupeChecker, MultChecker, NoCallHistory, NoContestHistory, NoDupeChecker,
            NoMultChecker, NoScp,
        },
        state::{AppState, Macros, Spot},
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
            &NoContestHistory,
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
        let mut entry1 = EntryState::from_spec(&contest.form_spec());
        let mut entry2 = EntryState::from_spec(&contest.form_spec());
        crate::entry::esm::apply_default_rst(&mut entry1, contest.as_ref(), "CW");
        crate::entry::esm::apply_default_rst(&mut entry2, contest.as_ref(), "CW");
        entries.insert(1, entry1);
        entries.insert(2, entry2);
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
            station_config: HashMap::new(),
            bandmap_cursors: HashMap::new(),
            default_cw_speed: 28,
            serial_counter: None,
            show_passband_qrm: false,
            bandmap_skip_worked: false,
            bandmap_high_at_top: false,
            bandmap_version: 0,
        }
    }

    #[test]
    fn block_dupes_gates_enter_with_beep_only() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().block_dupes = true;
        st.focused_entry_mut().mode = OpMode::Run;

        // RigStatus into a band MatchDupeChecker recognizes.
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::RigStatus {
                radio: 1, freq_hz: 14_025_000, mode: "CW".to_string(),
                is_ptt: false, filter_width_hz: None,
            },
        );
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::TextInput { s: "K5ZD".to_string() },
        );
        assert!(st.focused_entry().is_dupe);

        let effects = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::KeyPress { key: Key::Enter },
        );

        // Only a Beep — no CW, no log.
        assert_eq!(effects.len(), 1);
        assert!(matches!(
            effects[0],
            Effect::Beep { kind: crate::BeepKind::Error }
        ));
        assert!(!effects.iter().any(|e| matches!(e, Effect::CwSend { .. })));
        assert!(!effects.iter().any(|e| matches!(e, Effect::LogInsert { .. })));
    }

    #[test]
    fn block_dupes_does_not_affect_f_keys() {
        // F2 (and other F-keys) bypass handle_esm and remain functional even
        // when block_dupes refuses Enter.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().block_dupes = true;

        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::RigStatus {
                radio: 1, freq_hz: 14_025_000, mode: "CW".to_string(),
                is_ptt: false, filter_width_hz: None,
            },
        );
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::TextInput { s: "K5ZD".to_string() },
        );
        assert!(st.focused_entry().is_dupe);

        let effects = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::KeyPress { key: Key::F2 },
        );

        assert!(effects.iter().any(|e| matches!(e, Effect::CwSend { .. })));
    }

    #[test]
    fn block_dupes_off_logs_dupe_normally() {
        // Default behavior (block_dupes=false): Enter still works on dupes.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;
        // Don't set block_dupes — leave it at the default `false`.

        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::RigStatus {
                radio: 1, freq_hz: 14_025_000, mode: "CW".to_string(),
                is_ptt: false, filter_width_hz: None,
            },
        );
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::TextInput { s: "K5ZD".to_string() },
        );
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::KeyPress { key: Key::Space },
        );
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::TextInput { s: "5".to_string() },
        );

        // Enter 1: send call+exch.
        let e1 = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(e1.iter().any(|e| matches!(e, Effect::CwSend { .. })));
        // Enter 2: log.
        let e2 = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(e2.iter().any(|e| matches!(e, Effect::LogInsert { .. })));
    }

    #[test]
    fn rst_field_id_set_for_cqww() {
        let contest = contest_from_id("cqww").unwrap();
        assert_eq!(contest.rst_field_id(), Some(2));
    }

    #[test]
    fn rst_field_id_none_for_cwt() {
        // CWT exchange is CALL/NAME/XCHG — no RST field.
        let contest = contest_from_id("cwt").unwrap();
        assert_eq!(contest.rst_field_id(), None);
    }

    #[test]
    fn default_rst_uses_spec_variant_per_mode() {
        let contest = contest_from_id("cqww").unwrap();
        assert_eq!(contest.default_rst("CW").as_deref(), Some("599"));
        assert_eq!(contest.default_rst("SSB").as_deref(), Some("59"));
    }

    #[test]
    fn default_rst_falls_back_to_const_for_state_qp() {
        // MIQP has no `variants` block; the value comes from sent_variants.
        let contest = contest_from_id("miqp").unwrap();
        assert_eq!(contest.default_rst("CW").as_deref(), Some("599"));
    }

    #[test]
    fn space_behaves_like_tab_for_non_rst_contest() {
        // CWT has no RST field, so Space should advance one field, no skip.
        let contest = contest_from_id("cwt").unwrap();
        let mut entries = HashMap::new();
        entries.insert(1, EntryState::from_spec(&contest.form_spec()));
        entries.insert(2, EntryState::from_spec(&contest.form_spec()));
        let mut st = mk_state();
        st.entries = entries;
        let macros = Macros::default();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        assert_eq!(st.focused_entry_mut().focus, 1);
    }

    #[test]
    fn f12_wipes_entry_but_keeps_rst_populated() {
        // Regression: before this was fixed, F12 cleared RST and never
        // reapplied the default, so a subsequent bandmap-nav autopopulate
        // could log a QSO with blank RST.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().fields[0].value = "K1ABC".to_string();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::F12 },
        );
        assert_eq!(st.focused_entry().fields[0].value, "");
        assert_eq!(
            st.focused_entry().fields[1].value, "599",
            "F12 must reapply default RST so the next QSO isn't logged blank"
        );
    }

    #[test]
    fn bandmap_nav_fills_rst_when_entry_was_wiped() {
        // Regression: F12 + bandmap-nav used to autopopulate the call
        // field but leave RST empty, letting the operator log a QSO with
        // no RST. BandmapUp/Down now reapplies the default on empty.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.radios.insert(1, crate::state::RadioState {
            freq_hz: 14_025_000,
            mode: "CW".to_string(),
            is_ptt: false,
            cw_speed: 28,
            filter_width_hz: None,
        });
        st.bandmap.push(crate::state::Spot {
            call: "W1AW".to_string(),
            freq_hz: 14_030_000,
            mode: "CW".to_string(),
        });

        // Simulate the post-F12 state: RST blank.
        st.focused_entry_mut().fields[1].value.clear();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::BandmapDown { radio: 1 },
        );

        assert_eq!(st.focused_entry().fields[0].value, "W1AW");
        assert_eq!(
            st.focused_entry().fields[1].value, "599",
            "BandmapDown must reapply default RST when the field is blank"
        );
    }

    #[test]
    fn rst_repopulates_after_log() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        // RST is "599" out of mk_state.
        assert_eq!(st.focused_entry().fields[1].value, "599");

        // Log K1ABC zone 5. After clear_values, RST should be "599" again.
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::TextInput { s: "K1ABC".to_string() },
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
        reduce(&mut st, contest.as_ref(), &macros, AppEvent::KeyPress { key: Key::Enter });
        reduce(&mut st, contest.as_ref(), &macros, AppEvent::KeyPress { key: Key::Enter });

        assert_eq!(st.focused_entry().fields[0].value, "");
        assert_eq!(st.focused_entry().fields[1].value, "599");
        assert_eq!(st.focused_entry().fields[2].value, "");
    }

    #[test]
    fn rst_repopulates_to_59_when_rig_is_ssb() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        st.focused_entry_mut().mode = OpMode::Run;

        // Switch the rig to SSB before any log.
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::RigStatus {
                radio: 1,
                freq_hz: 14_200_000,
                mode: "SSB".to_string(),
                is_ptt: false,
                filter_width_hz: None,
            },
        );

        reduce(&mut st, contest.as_ref(), &macros, AppEvent::TextInput { s: "K1ABC".to_string() });
        reduce(&mut st, contest.as_ref(), &macros, AppEvent::KeyPress { key: Key::Space });
        reduce(&mut st, contest.as_ref(), &macros, AppEvent::TextInput { s: "5".to_string() });
        reduce(&mut st, contest.as_ref(), &macros, AppEvent::KeyPress { key: Key::Enter });
        reduce(&mut st, contest.as_ref(), &macros, AppEvent::KeyPress { key: Key::Enter });

        assert_eq!(st.focused_entry().fields[1].value, "59");
    }

    #[test]
    fn tab_focus_wraps() {
        // Tab walks every field including RST, so 3 presses on a 3-field
        // entry (CALL/RST/Zone) wrap back to CALL.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        for _ in 0..3 {
            reduce(
                &mut st,
                contest.as_ref(),
                &macros,
                AppEvent::KeyPress { key: Key::Tab },
            );
        }

        assert_eq!(st.focused_entry_mut().focus, 0);
    }

    #[test]
    fn space_skips_rst_field() {
        // Space skips the auto-populated RST field. From CALL (idx 0) it
        // should land on Zone (idx 2), not RST (idx 1).
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );

        assert_eq!(st.focused_entry_mut().focus, 2);
        assert_eq!(st.focused_entry_mut().fields[2].field_id, 3);
    }

    #[test]
    fn tab_lands_on_rst_field() {
        // Tab is the override path that lets the operator edit RST.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::KeyPress { key: Key::Tab },
        );

        assert_eq!(st.focused_entry_mut().focus, 1);
        assert_eq!(st.focused_entry_mut().fields[1].field_id, 2);
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
    fn sp_two_step_esm() {
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
            AppEvent::TextInput { s: "5".to_string() },
        );

        // Enter 2: send exchange (sp_exch, no callsign) AND log atomically
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
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text == "599 4"))
        );
        assert!(
            effects2
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );
        // Exchange CW must precede the log insert in the effects vector
        let exch_idx = effects2
            .iter()
            .position(|e| matches!(e, Effect::CwSend { .. }))
            .unwrap();
        let log_idx = effects2
            .iter()
            .position(|e| matches!(e, Effect::LogInsert { .. }))
            .unwrap();
        assert!(exch_idx < log_idx);
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

        // Focus moved off CALL (exact landing spot depends on RST-skip rules).
        assert!(st.focused_entry_mut().focus > 0);
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
            &NoContestHistory,
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
            &NoContestHistory,
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
            &NoContestHistory,
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
            &NoContestHistory,
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
            &NoContestHistory,
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
            &NoContestHistory,
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
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    #[test]
    fn bandmap_snap_outside_passband_shows_between() {
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_000, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::Between { freq_hz: 14_025_000 })
        ));
    }

    #[test]
    fn bandmap_snap_tie_prefers_lower_freq() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_026_000, "CW");
        // With a wide 2400 Hz filter both spots are inside the passband;
        // the tie-break prefers the lower-frequency spot.
        rig_status(&mut st, 1, 14_025_500, "CW", Some(2400));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
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
        let manual = st.bandmap_cursors.get(&1).cloned();
        assert!(matches!(manual, Some(BandmapCursor::On { .. })));
        // Another RigStatus at the *same* freq — BandmapUp's RigSet effect
        // isn't applied in unit tests, so the rig freq is unchanged. The
        // "freq didn't change" guard must preserve the manual cursor.
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        assert_eq!(st.bandmap_cursors.get(&1).cloned(), manual);
    }

    #[test]
    fn bandmap_snap_band_change() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 7_025_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
        // Move to 40m — filtered list becomes just the 40m spot.
        rig_status(&mut st, 1, 7_025_100, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "W1AW"
        ));
    }

    #[test]
    fn bandmap_snap_empty_filter_preserves_cursor() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        let before = st.bandmap_cursors.get(&1).cloned();
        assert!(before.is_some());
        // 15m has no spots; snap should leave the cursor alone.
        rig_status(&mut st, 1, 21_025_000, "CW", Some(500));
        assert_eq!(st.bandmap_cursors.get(&1).cloned(), before);
    }

    #[test]
    fn bandmap_snap_per_radio_independent() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        rig_status(&mut st, 2, 14_027_050, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
        assert!(matches!(
            st.bandmap_cursors.get(&2),
            Some(BandmapCursor::On { call, .. }) if call == "W1AW"
        ));
    }

    #[test]
    fn bandmap_snap_falls_back_to_mode_default_when_no_filter() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        // No filter_width_hz reported. CW fallback is 500 Hz, so half-width
        // is 250. A 200 Hz delta should still be "On".
        rig_status(&mut st, 1, 14_025_200, "CW", None);
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    #[test]
    fn bandmap_snap_normalizes_mode_usb_to_ssb() {
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_200_000, "SSB");
        // Rig reports "USB" — must normalize to "SSB" before filtering or
        // the spot list is empty and the snap is a no-op.
        rig_status(&mut st, 1, 14_200_500, "USB", Some(2400));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    #[test]
    fn self_spot_is_dropped_before_bandmap() {
        // Regression: if the operator gets spotted on their own freq,
        // the spot must not reach the bandmap. Otherwise nav steps
        // through the self-spot and the cursor can re-anchor on it.
        let mut st = mk_state(); // my_call = "N0CALL"
        let bandmap_version_before = st.bandmap_version;
        add_spot(&mut st, "N0CALL", 14_025_000, "CW");
        assert!(st.bandmap.is_empty(), "self-spot must not enter bandmap");
        assert_eq!(
            st.bandmap_version, bandmap_version_before,
            "self-spot must not bump bandmap_version"
        );
    }

    #[test]
    fn self_spot_match_is_case_insensitive() {
        let mut st = mk_state(); // my_call = "N0CALL"
        add_spot(&mut st, "n0call", 14_025_000, "CW");
        assert!(st.bandmap.is_empty(), "lowercase match must still drop");
    }

    #[test]
    fn non_self_spots_still_reach_bandmap() {
        let mut st = mk_state(); // my_call = "N0CALL"
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        assert_eq!(st.bandmap.len(), 1, "non-self spot must be recorded");
    }

    #[test]
    fn bandmap_cursor_updates_on_spot_received() {
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        // Rig parked 1 kHz above DL1ABC, outside its 500 Hz filter — so
        // initially the cursor is Between the two spots.
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::Between { freq_hz: 14_024_000 })
        ));
        // A new spot arrives at 14.024.000 — now inside the rig's filter.
        // The cursor must upgrade to On K5ZD on the SpotReceived event.
        add_spot(&mut st, "K5ZD", 14_024_000, "CW");
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    #[test]
    fn bandmap_cursor_updates_on_spot_withdrawn() {
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
        // Remove DL1ABC — the cursor is anchored to K5ZD, so it stays on
        // K5ZD. Previously (index-anchored), this event would have
        // spuriously shifted the cursor to W1AW.
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::SpotWithdrawn {
                call: "DL1ABC".to_string(),
            },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    #[test]
    fn bandmap_cursor_stays_between_when_new_spot_is_far() {
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_000, "CW", Some(500));
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::Between { freq_hz: 14_025_000 })
        ));
        // A new spot below both — cursor stays Between at the same freq.
        // Under the old index-based model the insertion index would have
        // shifted; now Between carries freq_hz directly so it's stable.
        add_spot(&mut st, "K0XX", 14_022_000, "CW");
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::Between { freq_hz: 14_025_000 })
        ));
    }

    #[test]
    fn bandmap_nav_cursor_survives_new_spot_near_rig_freq() {
        // User reports: after navigating to a spot with Ctrl-Alt-Down,
        // a new spot arriving near the rig freq sometimes shifts the
        // cursor off the spot they're trying to work. Next Ctrl-Alt-Down
        // then lands somewhere unexpected (potentially right back on
        // the just-worked call). Regression guard: once the cursor is
        // anchored on `call`, list churn must not move it.
        let mut st = mk_state();
        add_spot(&mut st, "DL1ABC", 14_023_000, "CW");
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_025_100, "CW", Some(500));
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();

        // Navigate: lands on K5ZD (next Down from the auto-snapped cursor).
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::BandmapDown { radio: 1 },
        );
        let before = st.bandmap_cursors.get(&1).cloned();
        assert!(
            matches!(&before, Some(BandmapCursor::On { call, .. }) if call == "W1AW"),
            "seeding assumption: after Down from K5ZD the cursor should land on W1AW, got {before:?}"
        );

        // A noisy new spot arrives right at the rig's freq. Pre-fix,
        // snap_all_bandmap_cursors would yank the cursor from W1AW to
        // the new spot (nearest to rig freq).
        add_spot(&mut st, "NEW1", 14_025_100, "CW");
        assert!(
            matches!(
                st.bandmap_cursors.get(&1),
                Some(BandmapCursor::On { call, .. }) if call == "W1AW"
            ),
            "cursor must remain anchored to the call the operator navigated to"
        );

        // And another BandmapDown keeps stepping from W1AW, not the
        // new spot. New sorted list: DL1ABC, K5ZD, NEW1, W1AW. Down
        // from W1AW (last) wraps to DL1ABC.
        reduce(
            &mut st,
            contest.as_ref(),
            &macros,
            AppEvent::BandmapDown { radio: 1 },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "DL1ABC"
        ));
    }

    #[test]
    fn bandmap_nav_without_skip_worked_lands_on_dupe_spots() {
        // Control test: with the flag off (default), BandmapDown must
        // still land on a dupe like it always has.
        let mut st = mk_state();
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        // Clear the auto-snap Between cursor so the step starts from None.
        st.bandmap_cursors.clear();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        // MatchDupeChecker reports K5ZD as a dupe on 20m CW.
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::BandmapDown { radio: 1 },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    #[test]
    fn bandmap_nav_skip_worked_jumps_past_dupe() {
        // Flag on: BandmapDown from nothing skips past K5ZD (dupe) and
        // lands on W1AW (unworked).
        let mut st = mk_state();
        st.bandmap_skip_worked = true;
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        st.bandmap_cursors.clear();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &MatchDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::BandmapDown { radio: 1 },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "W1AW"
        ));
    }

    #[test]
    fn bandmap_nav_skip_worked_is_noop_when_every_spot_is_dupe() {
        // All spots are dupes: nav key must not change the cursor and
        // must emit no RigSet.
        struct AllDupe;
        impl DupeChecker for AllDupe {
            fn is_dupe(&self, _: &str, _: &str, _: &str) -> bool { true }
        }
        let mut st = mk_state();
        st.bandmap_skip_worked = true;
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        let before_cursor = st.bandmap_cursors.get(&1).cloned();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        let effects = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &AllDupe, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::BandmapDown { radio: 1 },
        );
        assert!(effects.is_empty(), "all-dupes nav must not emit RigSet");
        assert_eq!(
            st.bandmap_cursors.get(&1).cloned(),
            before_cursor,
            "all-dupes nav must leave the cursor untouched"
        );
    }

    #[test]
    fn bandmap_nav_skip_worked_up_direction_also_skips() {
        // BandmapUp from nothing wraps to len-1 = W1AW in sorted order
        // (K5ZD, W1AW). W1AW is unworked → cursor lands there on the
        // first check. To exercise the skip in Up direction, make W1AW
        // the dupe instead.
        struct W1awDupe;
        impl DupeChecker for W1awDupe {
            fn is_dupe(&self, call: &str, _: &str, _: &str) -> bool { call == "W1AW" }
        }
        let mut st = mk_state();
        st.bandmap_skip_worked = true;
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        st.bandmap_cursors.clear();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &W1awDupe, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::BandmapUp { radio: 1 },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
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
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
        ));
    }

    /// Three spots at the exact same frequency, plus one bracketing spot
    /// above. Walking Up from the bracketing spot should visit each
    /// co-located call in turn, not collapse to the first-at-freq.
    /// Regression test for a bug where `snap_bandmap_cursor_to_freq`
    /// ran after every cross-freq BandmapUp and overwrote the anchored
    /// cursor with `spots[partition_point]` (always the lowest-indexed
    /// spot at target freq).
    #[test]
    fn bandmap_up_walks_through_co_located_spots() {
        let mut st = mk_state();
        add_spot(&mut st, "A1A", 14_000_000, "CW");
        add_spot(&mut st, "B2B", 14_000_000, "CW");
        add_spot(&mut st, "C3C", 14_000_000, "CW");
        add_spot(&mut st, "R9R", 14_010_000, "CW");
        rig_status(&mut st, 1, 14_010_000, "CW", Some(500));
        // Cursor now On(R9R).
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "R9R"
        ));

        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();

        // First Up: cursor moves to C3C; rig follows to 14_000_000.
        // The reducer's BandmapUp sets the cursor On(C3C), and the
        // RigStatus fire-back at the new freq must not re-snap to A1A.
        reduce(
            &mut st, contest.as_ref(), &macros,
            AppEvent::BandmapUp { radio: 1 },
        );
        rig_status(&mut st, 1, 14_000_000, "CW", Some(500));
        assert!(
            matches!(
                st.bandmap_cursors.get(&1),
                Some(BandmapCursor::On { call, .. }) if call == "C3C"
            ),
            "first Up should land on C3C (the highest-indexed co-located spot), not snap to A1A; got {:?}",
            st.bandmap_cursors.get(&1)
        );

        // Second Up: C3C → B2B. Rig freq unchanged, so no snap.
        reduce(
            &mut st, contest.as_ref(), &macros,
            AppEvent::BandmapUp { radio: 1 },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "B2B"
        ));

        // Third Up: B2B → A1A.
        reduce(
            &mut st, contest.as_ref(), &macros,
            AppEvent::BandmapUp { radio: 1 },
        );
        assert!(matches!(
            st.bandmap_cursors.get(&1),
            Some(BandmapCursor::On { call, .. }) if call == "A1A"
        ));
    }

    /// Direct check on the snap short-circuit: with the cursor already
    /// pinned to the middle spot of a co-located cluster, a `RigStatus`
    /// event that moves the rig freq *to* that cluster's frequency must
    /// leave the cursor alone rather than snapping to the first-at-freq.
    #[test]
    fn bandmap_snap_preserves_middle_of_cluster() {
        let mut st = mk_state();
        add_spot(&mut st, "A1A", 14_000_000, "CW");
        add_spot(&mut st, "B2B", 14_000_000, "CW");
        add_spot(&mut st, "C3C", 14_000_000, "CW");
        // Start the rig away from the cluster so the subsequent
        // rig_status is a genuine freq change (snap will run).
        rig_status(&mut st, 1, 14_010_000, "CW", Some(500));
        // Pin the cursor on B2B, the middle of the cluster.
        st.bandmap_cursors.insert(
            1,
            BandmapCursor::On { call: "B2B".to_string(), freq_hz: 14_000_000 },
        );
        // Rig moves to the cluster freq. Without the short-circuit,
        // snap would pick A1A (partition_point's first-at-freq).
        rig_status(&mut st, 1, 14_000_000, "CW", Some(500));
        assert!(
            matches!(
                st.bandmap_cursors.get(&1),
                Some(BandmapCursor::On { call, .. }) if call == "B2B"
            ),
            "snap must preserve a valid call-anchor at target freq; got {:?}",
            st.bandmap_cursors.get(&1)
        );
    }

    /// With `bandmap_high_at_top = true`, the bandmap displays highest
    /// freq at the top. Pressing BandmapDown means "visually down" =
    /// toward lower freq, so from a cleared cursor it should land on
    /// the lower-freq spot (K5ZD), not the higher one (W1AW).
    #[test]
    fn bandmap_nav_reversed_down_steps_to_lower_freq() {
        let mut st = mk_state();
        st.bandmap_high_at_top = true;
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        st.bandmap_cursors.clear();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            &mut st, contest.as_ref(), &macros,
            AppEvent::BandmapDown { radio: 1 },
        );
        assert!(
            matches!(
                st.bandmap_cursors.get(&1),
                Some(BandmapCursor::On { call, .. }) if call == "K5ZD"
            ),
            "reversed BandmapDown should land on the lower-freq spot; got {:?}",
            st.bandmap_cursors.get(&1)
        );
    }

    /// Symmetric to the above: with the flag on, BandmapUp should land
    /// on the higher-freq spot (visually up = toward the top of the
    /// reversed display).
    #[test]
    fn bandmap_nav_reversed_up_steps_to_higher_freq() {
        let mut st = mk_state();
        st.bandmap_high_at_top = true;
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        rig_status(&mut st, 1, 14_024_000, "CW", Some(500));
        st.bandmap_cursors.clear();
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        reduce(
            &mut st, contest.as_ref(), &macros,
            AppEvent::BandmapUp { radio: 1 },
        );
        assert!(
            matches!(
                st.bandmap_cursors.get(&1),
                Some(BandmapCursor::On { call, .. }) if call == "W1AW"
            ),
            "reversed BandmapUp should land on the higher-freq spot; got {:?}",
            st.bandmap_cursors.get(&1)
        );
    }

    /// Reversed display + skip-worked: the skip loop must walk in the
    /// visually-correct direction (visually down = lower freq). Anchor
    /// the cursor on W1AW (visually at the top in reversed mode), then
    /// press Down. With K5ZD a dupe, the step should land on K1ABC,
    /// having skipped past K5ZD.
    #[test]
    fn bandmap_nav_reversed_with_skip_worked() {
        struct K5ZDDupe;
        impl DupeChecker for K5ZDDupe {
            fn is_dupe(&self, call: &str, _: &str, _: &str) -> bool { call == "K5ZD" }
        }
        let mut st = mk_state();
        st.bandmap_high_at_top = true;
        st.bandmap_skip_worked = true;
        add_spot(&mut st, "K1ABC", 14_023_000, "CW");
        add_spot(&mut st, "K5ZD", 14_025_000, "CW");
        add_spot(&mut st, "W1AW", 14_027_000, "CW");
        // Park the rig on W1AW so the cursor anchors there.
        rig_status(&mut st, 1, 14_027_000, "CW", Some(500));
        let contest = contest_from_id("cqww").unwrap();
        let macros = Macros::default();
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &K5ZDDupe, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::BandmapDown { radio: 1 },
        );
        assert!(
            matches!(
                st.bandmap_cursors.get(&1),
                Some(BandmapCursor::On { call, .. }) if call == "K1ABC"
            ),
            "reversed+skip BandmapDown from W1AW should skip K5ZD and land on K1ABC; got {:?}",
            st.bandmap_cursors.get(&1)
        );
    }

    // ---- Contest-history autopopulate tests --------------------------

    /// Stubbed ContestHistoryLookup for reducer-level tests. Pre-wired
    /// rows return spec-id keyed pairs on exact-match lookup.
    struct StubContestHistory {
        rows: HashMap<String, Vec<(String, String)>>,
    }

    impl StubContestHistory {
        fn new(entries: &[(&str, &[(&str, &str)])]) -> Self {
            let rows = entries
                .iter()
                .map(|(call, pairs)| {
                    (
                        call.to_ascii_uppercase(),
                        pairs
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                    )
                })
                .collect();
            Self { rows }
        }
    }

    impl crate::reducer::ContestHistoryLookup for StubContestHistory {
        fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
            self.rows.get(&call_norm.to_ascii_uppercase()).cloned()
        }
    }

    /// Stubbed CallHistoryLookup (.ch stand-in) — column names, not spec ids.
    struct StubCallHistory {
        rows: HashMap<String, Vec<(String, String)>>,
    }

    impl StubCallHistory {
        fn new(entries: &[(&str, &[(&str, &str)])]) -> Self {
            let rows = entries
                .iter()
                .map(|(call, pairs)| {
                    (
                        call.to_ascii_uppercase(),
                        pairs
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                    )
                })
                .collect();
            Self { rows }
        }
    }

    impl crate::reducer::CallHistoryLookup for StubCallHistory {
        fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
            self.rows.get(&call_norm.to_ascii_uppercase()).cloned()
        }
    }

    fn reduce_with_history(
        st: &mut AppState,
        contest: &dyn ContestEntry,
        macros: &Macros,
        call_history: &dyn crate::reducer::CallHistoryLookup,
        contest_history: &dyn crate::reducer::ContestHistoryLookup,
        ev: AppEvent,
    ) -> Vec<Effect> {
        crate::reducer::reduce(
            st,
            contest,
            macros,
            &NoDupeChecker,
            &NoMultChecker,
            call_history,
            contest_history,
            &NoScp,
            ev,
        )
    }

    /// Contest-history wins over call-history for the same field: if the
    /// station sent a different name in this contest than `.ch` claims,
    /// the contest value is what auto-populates.
    #[test]
    fn contest_history_beats_call_history() {
        // CWT form: CALL(1), NAME(2), EXCH1(3).
        let contest = contest_from_id("cwt").unwrap();
        let mut st = mk_state_for(contest.as_ref());
        let macros = Macros::default();

        let ch = StubCallHistory::new(&[("N3QE", &[("Name", "TINA"), ("Exch1", "TN")])]);
        let th = StubContestHistory::new(&[(
            "N3QE",
            &[("name", "TIM"), ("xchg", "TN")],
        )]);

        reduce_with_history(
            &mut st,
            contest.as_ref(),
            &macros,
            &ch,
            &th,
            AppEvent::TextInput { s: "N3QE".to_string() },
        );

        let name_field = st
            .focused_entry()
            .fields
            .iter()
            .find(|f| f.label == "NAME")
            .expect("NAME field");
        assert_eq!(name_field.value, "TIM", "contest history wins over .ch");
        assert!(name_field.from_history);
    }

    /// When contest-history has no entry for the call, `.ch` data fills
    /// in the form unchanged — the new code path is additive.
    #[test]
    fn call_history_fills_when_contest_history_empty() {
        let contest = contest_from_id("cwt").unwrap();
        let mut st = mk_state_for(contest.as_ref());
        let macros = Macros::default();

        let ch = StubCallHistory::new(&[("N3QE", &[("Name", "TINA"), ("Exch1", "TN")])]);
        let th = StubContestHistory::new(&[]);

        reduce_with_history(
            &mut st,
            contest.as_ref(),
            &macros,
            &ch,
            &th,
            AppEvent::TextInput { s: "N3QE".to_string() },
        );

        let name_field = st
            .focused_entry()
            .fields
            .iter()
            .find(|f| f.label == "NAME")
            .expect("NAME field");
        assert_eq!(name_field.value, "TINA");
    }

    /// Contest-history fills NAME (partial row); `.ch` still fills the
    /// gap (EXCH1). Contest-history is authoritative where it has data;
    /// call-history covers the rest.
    #[test]
    fn contest_and_call_history_merge_on_disjoint_fields() {
        let contest = contest_from_id("cwt").unwrap();
        let mut st = mk_state_for(contest.as_ref());
        let macros = Macros::default();

        let ch = StubCallHistory::new(&[("N3QE", &[("Name", "TINA"), ("Exch1", "TN")])]);
        let th = StubContestHistory::new(&[("N3QE", &[("name", "TIM")])]);

        reduce_with_history(
            &mut st,
            contest.as_ref(),
            &macros,
            &ch,
            &th,
            AppEvent::TextInput { s: "N3QE".to_string() },
        );

        let fields = &st.focused_entry().fields;
        let name = fields.iter().find(|f| f.label == "NAME").unwrap();
        let exch = fields.iter().find(|f| f.label == "XCHG").unwrap();
        assert_eq!(name.value, "TIM", "contest history supplied NAME");
        assert_eq!(exch.value, "TN", "call history filled the gap");
    }

    /// Manually typed (non-`from_history`) values are never overwritten
    /// by a subsequent history lookup.
    #[test]
    fn manual_edit_survives_history_refresh() {
        let contest = contest_from_id("cwt").unwrap();
        let mut st = mk_state_for(contest.as_ref());
        let macros = Macros::default();

        let ch = StubCallHistory::new(&[]);
        let th = StubContestHistory::new(&[(
            "N3QE",
            &[("name", "TIM"), ("xchg", "TN")],
        )]);

        // Simulate: operator types call, then tabs into NAME and types
        // manually, then edits the call (which re-fires autopopulate).
        reduce_with_history(
            &mut st, contest.as_ref(), &macros, &ch, &th,
            AppEvent::TextInput { s: "N3QE".to_string() },
        );
        // NAME got auto-populated to "TIM"; operator tabs to NAME and
        // overwrites with "TOM" (first keystroke clears the autopop value).
        reduce_with_history(
            &mut st, contest.as_ref(), &macros, &ch, &th,
            AppEvent::KeyPress { key: Key::Tab },
        );
        reduce_with_history(
            &mut st, contest.as_ref(), &macros, &ch, &th,
            AppEvent::TextInput { s: "TOM".to_string() },
        );

        let name_before = st
            .focused_entry()
            .fields
            .iter()
            .find(|f| f.label == "NAME")
            .unwrap()
            .clone();
        assert_eq!(name_before.value, "TOM");
        assert!(!name_before.from_history);

        // Now bump the call field — this re-triggers apply_call_history
        // with the same contest-history row. The manually-edited NAME
        // must NOT be clobbered. Tab back to CALL, backspace off the
        // trailing character, then retype so the TextInput handler on
        // field_id=1 triggers a fresh history lookup.
        while st.focused_entry().fields[st.focused_entry().focus].field_id != 1 {
            reduce_with_history(
                &mut st,
                contest.as_ref(),
                &macros,
                &ch,
                &th,
                AppEvent::KeyPress { key: Key::Tab },
            );
        }
        reduce_with_history(
            &mut st,
            contest.as_ref(),
            &macros,
            &ch,
            &th,
            AppEvent::KeyPress { key: Key::Backspace },
        );
        reduce_with_history(
            &mut st,
            contest.as_ref(),
            &macros,
            &ch,
            &th,
            AppEvent::TextInput { s: "E".to_string() },
        );

        let name_after = st
            .focused_entry()
            .fields
            .iter()
            .find(|f| f.label == "NAME")
            .unwrap();
        assert_eq!(name_after.value, "TOM", "manual edit must survive");
        assert!(!name_after.from_history);
    }

    fn mk_state_for(contest: &dyn ContestEntry) -> AppState {
        let mut entries = HashMap::new();
        let mut entry1 = EntryState::from_spec(&contest.form_spec());
        let mut entry2 = EntryState::from_spec(&contest.form_spec());
        crate::entry::esm::apply_default_rst(&mut entry1, contest, "CW");
        crate::entry::esm::apply_default_rst(&mut entry2, contest, "CW");
        entries.insert(1, entry1);
        entries.insert(2, entry2);
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
            station_config: HashMap::new(),
            bandmap_cursors: HashMap::new(),
            default_cw_speed: 25,
            serial_counter: None,
            show_passband_qrm: false,
            bandmap_skip_worked: false,
            bandmap_high_at_top: false,
            bandmap_version: 0,
        }
    }

    #[test]
    fn cw_speed_adjust_updates_focused_radio_and_emits_effect() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        // Seed R1 so cw_speed starts at default_cw_speed (28 in mk_state).
        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &NoDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::RigStatus {
                radio: 1, freq_hz: 14_025_000, mode: "CW".to_string(),
                is_ptt: false, filter_width_hz: None,
            },
        );
        assert_eq!(st.radios.get(&1).unwrap().cw_speed, 28);

        let effects = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &NoDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::CwSpeedAdjust { delta: 2 },
        );
        assert_eq!(st.radios.get(&1).unwrap().cw_speed, 30);
        assert_eq!(effects, vec![Effect::KeyerSetSpeed { wpm: 30 }]);
    }

    #[test]
    fn cw_speed_adjust_clamps_and_suppresses_noop() {
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();

        crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &NoDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::RigStatus {
                radio: 1, freq_hz: 14_025_000, mode: "CW".to_string(),
                is_ptt: false, filter_width_hz: None,
            },
        );
        // Drive up against the 60 WPM ceiling in two saturating steps.
        for _ in 0..20 {
            crate::reducer::reduce(
                &mut st, contest.as_ref(), &macros,
                &NoDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
                AppEvent::CwSpeedAdjust { delta: 2 },
            );
        }
        assert_eq!(st.radios.get(&1).unwrap().cw_speed, 60);

        // Another PageUp at the ceiling is a no-op — no effect emitted.
        let effects = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &NoDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::CwSpeedAdjust { delta: 2 },
        );
        assert!(effects.is_empty());
    }

    #[test]
    fn cw_speed_adjust_without_rig_updates_default() {
        // Before any RigStatus, there's no RadioState — the adjust should
        // still take hold by bumping default_cw_speed so the value sticks.
        let contest = contest_from_id("cqww").unwrap();
        let mut st = mk_state();
        let macros = Macros::default();
        assert_eq!(st.default_cw_speed, 28);
        assert!(st.radios.is_empty());

        let effects = crate::reducer::reduce(
            &mut st, contest.as_ref(), &macros,
            &NoDupeChecker, &NoMultChecker, &NoCallHistory, &NoContestHistory, &NoScp,
            AppEvent::CwSpeedAdjust { delta: -2 },
        );
        assert_eq!(st.default_cw_speed, 26);
        assert_eq!(effects, vec![Effect::KeyerSetSpeed { wpm: 26 }]);
    }
}
