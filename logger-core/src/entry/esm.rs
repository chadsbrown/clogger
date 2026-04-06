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
        let mut effects = vec![Effect::CwSend {
            radio: st.focused_radio,
            text: compose_call_exchange(st, macros),
        }];
        // Auto-advance cursor past CALL field so operator can enter received exchange
        if focused_field_id(st) == Some(1) {
            if let Some(next_id) = next_field_id(st) {
                st.entry.focus += 1;
                effects.push(Effect::UiSetFocus { field_id: next_id });
            }
        }
        return effects;
    }

    log_and_clear(st, contest, macros, true, !st.esm_policy.run_two_step)
}

fn handle_sp(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    // Cursor in CALL field: send MYCALL (repeatable)
    if focused_field_id(st) == Some(1) {
        if st.current_call().is_empty() {
            return Vec::new();
        }
        st.entry.esm_step = EsmStep::CallSent;
        return vec![Effect::CwSend {
            radio: st.focused_radio,
            text: expand_macro("{MYCALL}", st),
        }];
    }

    // Cursor past CALL field: exchange or log based on EsmStep
    match st.entry.esm_step {
        EsmStep::Idle | EsmStep::CallSent => {
            if st.entry.overall.is_invalid() {
                return invalid_focus_effects(st);
            }
            st.entry.esm_step = EsmStep::ExchSent;
            vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro(&macros.f2, st),
            }]
        }
        EsmStep::ExchSent => log_and_clear(st, contest, macros, false, false),
    }
}

fn log_and_clear(
    st: &mut AppState,
    contest: &dyn ContestEntry,
    macros: &Macros,
    send_tu: bool,
    send_exch: bool,
) -> Vec<Effect> {
    match contest.build_qso_draft(&st.entry, &entry_ctx(st)) {
        Ok(draft) => {
            let exch_text = compose_call_exchange(st, macros);
            let tu_text = if send_tu {
                Some(expand_macro(&macros.f3, st))
            } else {
                None
            };

            st.entry.clear_values();
            st.entry.esm_step = EsmStep::Idle;

            let mut effects = Vec::new();
            if send_exch {
                effects.push(Effect::CwSend {
                    radio: st.focused_radio,
                    text: exch_text,
                });
            }
            effects.push(Effect::LogInsert { draft });
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

fn focused_field_id(st: &AppState) -> Option<u16> {
    st.entry.fields.get(st.entry.focus).map(|f| f.field_id)
}

fn next_field_id(st: &AppState) -> Option<u16> {
    st.entry.fields.get(st.entry.focus + 1).map(|f| f.field_id)
}

fn compose_call_exchange(st: &AppState, macros: &Macros) -> String {
    let call = expand_macro(&macros.f5, st);
    let exch = expand_macro(&macros.f2, st);
    format!("{call} {exch}")
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
