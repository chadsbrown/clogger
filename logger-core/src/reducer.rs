use crate::{
    contest::{
        filtered_bandmap_spots, freq_to_band_label,
        traits::{ContestEntry, EntryContext},
    },
    effects::Effect,
    entry::{
        esm::handle_esm,
        state::{EsmStep, OpMode},
    },
    events::{AppEvent, Key},
    macro_expand::expand_macro,
    state::{AppState, Macros, RadioState},
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
        } => {
            st.radios.insert(
                radio,
                RadioState {
                    freq_hz,
                    mode,
                    is_ptt,
                },
            );
            if radio == st.focused_radio {
                recompute_feedback(st, dupe_checker, mult_checker);
            }
            Vec::new()
        }
        AppEvent::SpotReceived { spot } => {
            st.bandmap.push(spot);
            Vec::new()
        }
        AppEvent::SpotWithdrawn { call } => {
            st.bandmap.retain(|s| s.call != call);
            Vec::new()
        }
        AppEvent::SetOpMode { mode } => {
            st.entry.mode = mode;
            Vec::new()
        }
        AppEvent::ToggleOpMode => {
            st.entry.mode = match st.entry.mode {
                OpMode::Run => OpMode::Sp,
                OpMode::Sp => OpMode::Run,
            };
            Vec::new()
        }
        AppEvent::FocusRadio { radio } => {
            st.focused_radio = radio;
            recompute_feedback(st, dupe_checker, mult_checker);
            Vec::new()
        }
        AppEvent::SetOperator { operator } => {
            st.active_operator = operator;
            Vec::new()
        }
        AppEvent::TextInput { s } => {
            let mut touched_call = false;
            if let Some(field) = st.entry.focused_mut() {
                touched_call = field.field_id == 1;
                if !touched_call {
                    field.from_history = false;
                }
                field.value.push_str(&s);
            }
            revalidate_after_edit(st, contest);
            if touched_call {
                st.entry.scp_cycle_index = None;
                recompute_feedback(st, dupe_checker, mult_checker);
                apply_call_history(st, contest, call_history, scp);
                revalidate_after_edit(st, contest);
            }
            Vec::new()
        }
        AppEvent::KeyPress { key } => match key {
            Key::Space | Key::Tab => {
                if !st.entry.fields.is_empty() {
                    st.entry.focus = (st.entry.focus + 1) % st.entry.fields.len();
                }
                Vec::new()
            }
            Key::Backspace => {
                let mut touched_call = false;
                if let Some(field) = st.entry.focused_mut() {
                    touched_call = field.field_id == 1;
                    if !touched_call {
                        field.from_history = false;
                    }
                    field.value.pop();
                }
                revalidate_after_edit(st, contest);
                if touched_call {
                    st.entry.scp_cycle_index = None;
                    recompute_feedback(st, dupe_checker, mult_checker);
                    apply_call_history(st, contest, call_history, scp);
                    revalidate_after_edit(st, contest);
                }
                Vec::new()
            }
            Key::Esc => {
                let mut touched_call = false;
                if let Some(field) = st.entry.focused_mut() {
                    touched_call = field.field_id == 1;
                    field.value.clear();
                }
                if touched_call {
                    st.entry.scp_cycle_index = None;
                    clear_history_fields(st);
                }
                revalidate_after_edit(st, contest);
                if touched_call {
                    recompute_feedback(st, dupe_checker, mult_checker);
                }
                Vec::new()
            }
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
            Key::Equal => {
                if st.entry.scp_matches.is_empty() {
                    return Vec::new();
                }
                let len = st.entry.scp_matches.len();
                let idx = match st.entry.scp_cycle_index {
                    None => 0,
                    Some(i) => (i + 1) % len,
                };
                st.entry.scp_cycle_index = Some(idx);
                let new_call = st.entry.scp_matches[idx].clone();
                if let Some(field) = st.entry.fields.iter_mut().find(|f| f.field_id == 1) {
                    field.value = new_call;
                }
                let saved_matches = st.entry.scp_matches.clone();
                let saved_n1 = st.entry.scp_n1_matches.clone();
                let saved_index = st.entry.scp_cycle_index;
                revalidate_after_edit(st, contest);
                recompute_feedback(st, dupe_checker, mult_checker);
                apply_call_history(st, contest, call_history, scp);
                revalidate_after_edit(st, contest);
                st.entry.scp_matches = saved_matches;
                st.entry.scp_n1_matches = saved_n1;
                st.entry.scp_cycle_index = saved_index;
                Vec::new()
            }
            Key::Enter => handle_esm(st, contest, macros),
        },
        AppEvent::EsmTrigger => handle_esm(st, contest, macros),
        AppEvent::BandmapUp | AppEvent::BandmapDown => {
            let radio = st.radios.get(&st.focused_radio).filter(|r| r.freq_hz > 0);
            let band = radio
                .map(|r| freq_to_band_label(r.freq_hz))
                .unwrap_or_else(|| "40m".to_string());
            let mode = radio.map(|r| r.mode.as_str()).unwrap_or("CW");

            let spots = filtered_bandmap_spots(&st.bandmap, &band, mode);
            if spots.is_empty() {
                return Vec::new();
            }

            let len = spots.len();
            let idx = match (ev, st.bandmap_cursor) {
                (AppEvent::BandmapDown, None) => 0,
                (AppEvent::BandmapDown, Some(i)) => (i + 1) % len,
                (AppEvent::BandmapUp, None) => len - 1,
                (AppEvent::BandmapUp, Some(i)) => (i + len - 1) % len,
                _ => unreachable!(),
            };

            let spot = &spots[idx];
            let radio = st.focused_radio;
            let freq_hz = spot.freq_hz;

            st.bandmap_cursor = Some(idx);
            st.entry.mode = OpMode::Sp;

            // Set call field to selected spot's callsign
            if let Some(field) = st.entry.fields.iter_mut().find(|f| f.field_id == 1) {
                field.value = spot.call.clone();
            }

            // Reset focus to call field
            st.entry.focus = 0;
            st.entry.scp_cycle_index = None;

            // Revalidate + feedback + call history (same pattern as Key::Equal)
            revalidate_after_edit(st, contest);
            recompute_feedback(st, dupe_checker, mult_checker);
            apply_call_history(st, contest, call_history, scp);
            revalidate_after_edit(st, contest);

            vec![Effect::RigSet { radio, freq_hz }]
        }
    }
}

fn revalidate_after_edit(st: &mut AppState, contest: &dyn ContestEntry) {
    let validation = contest.validate_entry(
        &st.entry,
        &EntryContext {
            my_call: st.my_call.clone(),
            my_zone: st.my_zone,
            rst_sent: st.rst_sent.clone(),
            rig: st.radios.get(&st.focused_radio).cloned(),
        },
    );

    for (idx, status) in validation.fields.into_iter().enumerate() {
        if let Some(field) = st.entry.fields.get_mut(idx) {
            field.status = status;
        }
    }
    st.entry.overall = validation.overall;
    // Reset ExchSent→Idle on edit (forces re-send in Run mode).
    // Keep CallSent so S&P users can fill exchange after sending their call.
    if st.entry.esm_step == EsmStep::ExchSent {
        st.entry.esm_step = EsmStep::Idle;
    }
}

fn recompute_feedback(
    st: &mut AppState,
    dupe_checker: &dyn DupeChecker,
    mult_checker: &dyn MultChecker,
) {
    let call_norm = st.current_call();
    if call_norm.is_empty() {
        st.entry.is_dupe = false;
        st.entry.is_new_mult = false;
        return;
    }
    let Some(rig) = st.radios.get(&st.focused_radio) else {
        st.entry.is_dupe = false;
        st.entry.is_new_mult = false;
        return;
    };

    let band = crate::contest::freq_to_band_label(rig.freq_hz);
    let mode = normalize_mode(&rig.mode);
    st.entry.is_dupe = dupe_checker.is_dupe(&call_norm, &band, &mode);
    st.entry.is_new_mult = mult_checker.is_new_mult(&call_norm, &band, &mode);
}

fn apply_call_history(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    call_history: &dyn CallHistoryLookup,
    scp: &dyn ScpLookup,
) {
    let call_norm = st.current_call();
    if call_norm.is_empty() {
        // Clear any previous history-populated fields and SCP matches
        clear_history_fields(st);
        return;
    }

    // Update SCP matches
    st.entry.scp_matches = scp.partial_matches(&call_norm, 10);
    st.entry.scp_n1_matches = scp.n_plus_one_matches(&call_norm, 10);

    // Exact lookup
    let Some(pairs) = call_history.lookup(&call_norm) else {
        // No exact match — clear history-populated fields but keep SCP
        for field in &mut st.entry.fields {
            if field.from_history {
                field.value.clear();
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
            if let Some(field) = st.entry.fields.iter_mut().find(|f| f.field_id == *field_id) {
                if field.value.is_empty() || field.from_history {
                    field.value = if value.chars().all(|c| c.is_ascii_digit()) {
                        value.to_string()
                    } else {
                        value.to_ascii_uppercase()
                    };
                    field.from_history = true;
                }
            }
        }
    }
}

fn clear_history_fields(st: &mut AppState) {
    for field in &mut st.entry.fields {
        if field.from_history {
            field.value.clear();
            field.from_history = false;
        }
    }
    st.entry.scp_matches.clear();
    st.entry.scp_n1_matches.clear();
}

fn normalize_mode(mode: &str) -> String {
    match mode.trim().to_ascii_uppercase().as_str() {
        "CW" => "CW",
        "SSB" => "SSB",
        "DIGITAL" => "DIGITAL",
        _ => "OTHER",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        CqwwContest,
        contest::traits::ContestEntry,
        effects::Effect,
        entry::state::{EntryState, EsmStep, OpMode, Validation},
        events::{AppEvent, Key},
        reducer::{
            DupeChecker, MultChecker, NoCallHistory, NoDupeChecker, NoMultChecker, NoScp,
        },
        state::{AppState, EsmPolicy, Macros},
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
        let contest = CqwwContest::default();
        AppState {
            now_ms: 0,
            focused_radio: 1,
            active_operator: 1,
            radios: HashMap::new(),
            entry: EntryState::from_spec(&contest.form_spec()),
            bandmap: Vec::new(),
            last_logged: None,
            my_call: "N0CALL".to_string(),
            my_zone: 4,
            rst_sent: "599".to_string(),
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursor: None,
        }
    }

    #[test]
    fn space_focus_wraps() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );

        assert_eq!(st.entry.focus, 0);
    }

    #[test]
    fn validation_updates_per_field_status() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "59".to_string(),
            },
        );

        assert_eq!(st.entry.fields[0].status, Validation::Valid);
        assert_eq!(st.entry.fields[1].status, Validation::Valid);
        assert!(st.entry.fields[2].status.is_invalid());
    }

    #[test]
    fn editing_resets_esm_step() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();

        st.entry.esm_step = EsmStep::ExchSent;
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput { s: "K".to_string() },
        );

        assert_eq!(st.entry.esm_step, EsmStep::Idle);
    }

    #[test]
    fn run_two_step_state_transition() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Run;

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput { s: "5".to_string() },
        );

        let effects1 = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.entry.esm_step, EsmStep::ExchSent);
        assert!(effects1.iter().any(|e| matches!(e, Effect::CwSend { .. })));

        let effects2 = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.entry.esm_step, EsmStep::Idle);
        assert!(
            effects2
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );
    }

    #[test]
    fn sp_three_step_esm() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Sp;

        // Type call
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );

        // Enter 1: send MYCALL, step → CallSent
        let effects1 = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.entry.esm_step, EsmStep::CallSent);
        assert!(
            effects1
                .iter()
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text == "N0CALL"))
        );

        // Fill exchange
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput { s: "5".to_string() },
        );

        // Enter 2: send exchange (sp_exch, no callsign), step → ExchSent
        let effects2 = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.entry.esm_step, EsmStep::ExchSent);
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
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.entry.esm_step, EsmStep::Idle);
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
    fn invalid_enter_beeps_and_focuses_first_invalid() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Run;

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        let effects = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );

        assert!(effects.iter().any(|e| matches!(e, Effect::Beep { .. })));
        assert_eq!(st.entry.fields[st.entry.focus].field_id, 2);
    }

    #[test]
    fn space_advances_without_inserting_literal_space() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );

        assert_eq!(st.entry.focus, 1);
        assert_eq!(st.entry.fields[0].value, "K1ABC");
        assert!(st.entry.fields[0].value.chars().all(|c| c != ' '));
    }

    #[test]
    fn dupe_recomputes_on_call_edit_and_focused_rig_changes() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();

        crate::reducer::reduce(
            &mut st,
            &contest,
            &macros,
            &MatchDupeChecker,
            &NoMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::TextInput {
                s: "K5ZD".to_string(),
            },
        );
        assert!(!st.entry.is_dupe);

        crate::reducer::reduce(
            &mut st,
            &contest,
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
            },
        );
        assert!(st.entry.is_dupe);

        crate::reducer::reduce(
            &mut st,
            &contest,
            &macros,
            &MatchDupeChecker,
            &NoMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::FocusRadio { radio: 2 },
        );
        assert!(!st.entry.is_dupe);
    }

    #[test]
    fn mult_recomputes_on_call_and_focus_context_changes() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();

        crate::reducer::reduce(
            &mut st,
            &contest,
            &macros,
            &NoDupeChecker,
            &MatchMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::TextInput {
                s: "DL1ABC".to_string(),
            },
        );
        assert!(!st.entry.is_new_mult);

        crate::reducer::reduce(
            &mut st,
            &contest,
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
            },
        );
        assert!(st.entry.is_new_mult);

        crate::reducer::reduce(
            &mut st,
            &contest,
            &macros,
            &NoDupeChecker,
            &MatchMultChecker,
            &NoCallHistory,
            &NoScp,
            AppEvent::FocusRadio { radio: 2 },
        );
        assert!(!st.entry.is_new_mult);
    }

    #[test]
    fn run_exchsent_logs_without_resending_exch() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Run;

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput { s: "5".to_string() },
        );

        let _ = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        let effects = reduce(
            &mut st,
            &contest,
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
    fn run_edit_after_exch_sent_requires_resend_then_log() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Run;

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "K1ABC".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "599".to_string(),
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Space },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput {
                s: "05".to_string(),
            },
        );

        let _ = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert_eq!(st.entry.esm_step, EsmStep::ExchSent);

        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress {
                key: Key::Backspace,
            },
        );
        reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::TextInput { s: "4".to_string() },
        );
        assert_eq!(st.entry.esm_step, EsmStep::Idle);

        let effects_resend = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(
            effects_resend.iter().any(|e| {
                matches!(e, Effect::CwSend { text, .. } if text.contains("K1ABC 599 4"))
            })
        );
        assert!(
            !effects_resend
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );

        let effects_log = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(
            effects_log
                .iter()
                .any(|e| matches!(e, Effect::LogInsert { .. }))
        );
        assert!(
            effects_log
                .iter()
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text.starts_with("TU ")))
        );
    }
}
