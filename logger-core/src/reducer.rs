use crate::{
    contest::traits::{ContestEntry, EntryContext},
    effects::Effect,
    entry::{esm::handle_esm, state::EsmStep},
    events::{AppEvent, Key},
    macro_expand::expand_macro,
    state::{AppState, Macros, RadioState},
};

pub fn reduce(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    macros: &Macros,
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
            Vec::new()
        }
        AppEvent::SetOperator { operator } => {
            st.active_operator = operator;
            Vec::new()
        }
        AppEvent::TextInput { s } => {
            if let Some(field) = st.entry.focused_mut() {
                field.value.push_str(&s);
            }
            revalidate_after_edit(st, contest);
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
                if let Some(field) = st.entry.focused_mut() {
                    field.value.pop();
                }
                revalidate_after_edit(st, contest);
                Vec::new()
            }
            Key::Esc => {
                if let Some(field) = st.entry.focused_mut() {
                    field.value.clear();
                }
                revalidate_after_edit(st, contest);
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        contest::traits::ContestEntry,
        CqwwContest,
        effects::Effect,
        entry::state::{EntryState, EsmStep, OpMode, Validation},
        events::{AppEvent, Key},
        reducer::reduce,
        state::{AppState, EsmPolicy, Macros},
    };

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
}
