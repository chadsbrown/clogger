use crate::{
    contest::traits::{ContestEntry, EntryContext},
    effects::{BeepKind, Effect},
    entry::state::{EsmStep, OpMode},
    macro_expand::expand_macro,
    state::{AppState, Macros},
};

pub fn handle_esm(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    if !st.entry.esm_enabled {
        return Vec::new();
    }

    if st.entry.mode == OpMode::Run {
        handle_run(st, contest, macros)
    } else {
        handle_sp(st, contest, macros)
    }
}

fn handle_run(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    if st.current_call().is_empty() {
        return vec![Effect::CwSend {
            radio: st.focused_radio,
            text: expand_macro(&macros.f1, st),
        }];
    }

    if st.entry.overall.is_invalid() {
        return invalid_focus_effects(st);
    }

    if st.entry.esm_step == EsmStep::Idle && st.esm_policy.run_two_step {
        st.entry.esm_step = EsmStep::ExchSent;
        return vec![Effect::CwSend {
            radio: st.focused_radio,
            text: expand_macro(&macros.f2, st),
        }];
    }

    log_and_clear(st, contest, macros, true)
}

fn handle_sp(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    if st.entry.overall.is_invalid() {
        return invalid_focus_effects(st);
    }

    if st.esm_policy.sp_log_on_first_enter {
        return log_and_clear(st, contest, macros, st.esm_policy.sp_send_tu);
    }

    if st.entry.esm_step == EsmStep::Idle {
        st.entry.esm_step = EsmStep::ExchSent;
        return vec![Effect::CwSend {
            radio: st.focused_radio,
            text: expand_macro(&macros.f2, st),
        }];
    }

    log_and_clear(st, contest, macros, st.esm_policy.sp_send_tu)
}

fn log_and_clear(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    macros: &Macros,
    send_tu: bool,
) -> Vec<Effect> {
    match contest.build_qso_draft(&st.entry, &entry_ctx(st)) {
        Ok(draft) => {
            let exch_text = expand_macro(&macros.f2, st);
            let tu_text = if send_tu {
                Some(expand_macro(&macros.f3, st))
            } else {
                None
            };

            st.entry.clear_values();
            st.entry.esm_step = EsmStep::Idle;

            let mut effects = vec![
                Effect::CwSend {
                    radio: st.focused_radio,
                    text: exch_text,
                },
                Effect::LogInsert { draft },
            ];
            if let Some(text) = tu_text {
                effects.push(Effect::CwSend {
                    radio: st.focused_radio,
                    text,
                });
            }
            effects.push(Effect::UiClearEntry);
            effects.push(Effect::UiSetFocus { field_id: 1 });
            effects
        }
        Err(_) => vec![Effect::Beep {
            kind: BeepKind::Error,
        }],
    }
}

fn invalid_focus_effects(st: &mut AppState) -> Vec<Effect> {
    let mut effects = vec![Effect::Beep {
        kind: BeepKind::Error,
    }];
    if let Some(idx) = st.entry.first_invalid_index() {
        st.entry.focus = idx;
        let field_id = st.entry.fields[idx].field_id;
        effects.push(Effect::UiSetFocus { field_id });
    }
    effects
}

fn entry_ctx(st: &AppState) -> EntryContext {
    EntryContext {
        my_call: st.my_call.clone(),
        my_zone: st.my_zone,
        rst_sent: st.rst_sent.clone(),
        rig: st.radios.get(&st.focused_radio).cloned(),
    }
}
