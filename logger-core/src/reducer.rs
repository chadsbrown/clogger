use crate::{
    contest::traits::{ContestEntry, EntryContext},
    effects::Effect,
    entry::{esm::handle_esm, state::EsmStep},
    events::{AppEvent, Key},
    macro_expand::expand_macro,
    state::{AppState, Macros, RadioState},
};

pub trait DupeChecker {
    fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool;
}

pub struct NoDupeChecker;

impl DupeChecker for NoDupeChecker {
    fn is_dupe(&self, _call_norm: &str, _band: &str, _mode: &str) -> bool {
        false
    }
}

pub fn reduce(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    macros: &Macros,
    dupe_checker: &dyn DupeChecker,
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
                recompute_dupe(st, dupe_checker);
            }
            Vec::new()
        }
        AppEvent::SpotReceived { spot } => {
            st.bandmap.push(spot);
            Vec::new()
        }
        AppEvent::SetOpMode { mode } => {
            st.entry.mode = mode;
            Vec::new()
        }
        AppEvent::FocusRadio { radio } => {
            st.focused_radio = radio;
            recompute_dupe(st, dupe_checker);
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
                field.value.push_str(&s);
            }
            revalidate_after_edit(st, contest);
            if touched_call {
                recompute_dupe(st, dupe_checker);
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
                    field.value.pop();
                }
                revalidate_after_edit(st, contest);
                if touched_call {
                    recompute_dupe(st, dupe_checker);
                }
                Vec::new()
            }
            Key::Esc => {
                let mut touched_call = false;
                if let Some(field) = st.entry.focused_mut() {
                    touched_call = field.field_id == 1;
                    field.value.clear();
                }
                revalidate_after_edit(st, contest);
                if touched_call {
                    recompute_dupe(st, dupe_checker);
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
            Key::Enter => handle_esm(st, contest, macros),
        },
        AppEvent::EsmTrigger => handle_esm(st, contest, macros),
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
    st.entry.esm_step = EsmStep::Idle;
}

fn recompute_dupe(st: &mut AppState, dupe_checker: &dyn DupeChecker) {
    let call_norm = st.current_call();
    if call_norm.is_empty() {
        st.entry.is_dupe = false;
        return;
    }
    let Some(rig) = st.radios.get(&st.focused_radio) else {
        st.entry.is_dupe = false;
        return;
    };

    let band = freq_to_band_label(rig.freq_hz);
    let mode = normalize_mode(&rig.mode);
    st.entry.is_dupe = dupe_checker.is_dupe(&call_norm, &band, &mode);
}

fn freq_to_band_label(freq_hz: u64) -> String {
    match freq_hz {
        1_800_000..=2_000_000 => "160m",
        3_500_000..=4_000_000 => "80m",
        7_000_000..=7_300_000 => "40m",
        14_000_000..=14_350_000 => "20m",
        21_000_000..=21_450_000 => "15m",
        28_000_000..=29_700_000 => "10m",
        _ => "other",
    }
    .to_string()
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
        contest::traits::ContestEntry,
        CqwwContest,
        effects::Effect,
        entry::state::{EntryState, EsmStep, OpMode, Validation},
        events::{AppEvent, Key},
        reducer::{DupeChecker, NoDupeChecker},
        state::{AppState, EsmPolicy, Macros},
    };

    fn reduce(
        st: &mut AppState,
        contest: &dyn ContestEntry,
        macros: &Macros,
        ev: AppEvent,
    ) -> Vec<Effect> {
        crate::reducer::reduce(st, contest, macros, &NoDupeChecker, ev)
    }

    struct MatchDupeChecker;

    impl DupeChecker for MatchDupeChecker {
        fn is_dupe(&self, call_norm: &str, band: &str, mode: &str) -> bool {
            call_norm == "K5ZD" && band == "20m" && mode == "CW"
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
            esm_policy: EsmPolicy::default(),
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
        assert!(effects2.iter().any(|e| matches!(e, Effect::LogInsert { .. })));
    }

    #[test]
    fn sp_one_step_logs_immediately_when_valid() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Sp;

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

        let effects = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::LogInsert { .. })));
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
    fn sp_send_tu_policy_emits_tu() {
        let contest = CqwwContest::default();
        let mut st = mk_state();
        let macros = Macros::default();
        st.entry.mode = OpMode::Sp;
        st.esm_policy.sp_send_tu = true;

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

        let effects = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::CwSend { text, .. } if text.contains("TU")))
        );
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
            AppEvent::FocusRadio { radio: 2 },
        );
        assert!(!st.entry.is_dupe);
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
        assert!(effects.iter().any(|e| matches!(e, Effect::LogInsert { .. })));
        assert!(effects.iter().any(|e| matches!(e, Effect::CwSend { text, .. } if text.starts_with("TU "))));
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
            AppEvent::TextInput { s: "05".to_string() },
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
        assert!(effects_resend.iter().any(|e| {
            matches!(e, Effect::CwSend { text, .. } if text.contains("K1ABC 599 4"))
        }));
        assert!(!effects_resend
            .iter()
            .any(|e| matches!(e, Effect::LogInsert { .. })));

        let effects_log = reduce(
            &mut st,
            &contest,
            &macros,
            AppEvent::KeyPress { key: Key::Enter },
        );
        assert!(effects_log
            .iter()
            .any(|e| matches!(e, Effect::LogInsert { .. })));
        assert!(effects_log
            .iter()
            .any(|e| matches!(e, Effect::CwSend { text, .. } if text.starts_with("TU "))));
    }
}
