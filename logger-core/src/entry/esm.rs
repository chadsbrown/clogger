use crate::{
    contest::traits::{ContestEntry, EntryContext},
    effects::{BeepKind, Effect},
    entry::state::{EsmStep, OpMode},
    macro_expand::expand_macro,
    state::{AppState, LastLoggedContext, Macros},
};

pub fn quick_log(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    if st.focused_entry().overall.is_invalid() {
        return invalid_focus_effects(st);
    }
    log_and_clear(st, contest, macros, false, false)
}

pub fn handle_esm(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    if !st.focused_entry().esm_enabled {
        return Vec::new();
    }

    if st.focused_entry().mode == OpMode::Run {
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

    if st.focused_entry().esm_step == EsmStep::Idle && st.esm_policy.run_two_step {
        st.focused_entry_mut().esm_step = EsmStep::ExchSent;
        claim_serial(st, contest);
        let mut effects = vec![Effect::CwSend {
            radio: st.focused_radio,
            text: compose_call_exchange(st, macros),
        }];
        // Auto-advance cursor past CALL field so operator can enter received exchange
        if focused_field_id(st) == Some(1) {
            if let Some(next_id) = next_field_id(st) {
                st.focused_entry_mut().focus += 1;
                effects.push(Effect::UiSetFocus { field_id: next_id });
            }
        }
        return effects;
    }

    if st.focused_entry().overall.is_invalid() {
        return invalid_focus_effects(st);
    }

    log_and_clear(st, contest, macros, true, !st.esm_policy.run_two_step)
}

fn handle_sp(st: &mut AppState, contest: &dyn ContestEntry, macros: &Macros) -> Vec<Effect> {
    // Cursor in CALL field: send MYCALL (repeatable)
    if focused_field_id(st) == Some(1) {
        if st.current_call().is_empty() {
            return Vec::new();
        }
        st.focused_entry_mut().esm_step = EsmStep::CallSent;
        return vec![Effect::CwSend {
            radio: st.focused_radio,
            text: expand_macro("{MYCALL}", st),
        }];
    }

    // Cursor past CALL field: exchange or log based on EsmStep
    match st.focused_entry().esm_step {
        EsmStep::Idle | EsmStep::CallSent => {
            if st.focused_entry().overall.is_invalid() {
                return invalid_focus_effects(st);
            }
            st.focused_entry_mut().esm_step = EsmStep::ExchSent;
            claim_serial(st, contest);
            let template = macros.sp_f2.as_deref().unwrap_or(&macros.f2);
            vec![Effect::CwSend {
                radio: st.focused_radio,
                text: expand_macro(template, st),
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
    claim_serial(st, contest);
    let ctx = entry_ctx(st);
    match contest.build_qso_drafts(st.focused_entry(), &ctx) {
        Ok(mut drafts) => {
            // Append serial to each draft's exchange if assigned. For county-
            // line rover splits, every generated draft shares the same serial
            // because they all come from one transmission.
            if let Some(serial) = st.focused_entry().assigned_serial {
                for draft in &mut drafts {
                    draft
                        .exchange_pairs
                        .push(("serial".to_string(), serial.to_string()));
                }
            }
            let exch_text = compose_call_exchange(st, macros);
            let tu_text = if send_tu {
                Some(expand_macro(&macros.f3, st))
            } else {
                None
            };

            // Snapshot context before clearing for repeat-to-previous.
            // `current_call` returns `&str`; this is the one place we allocate
            // an owned copy, and it only fires once per QSO log.
            let last_ctx = LastLoggedContext {
                call: st.current_call().to_owned(),
                fields: st.focused_entry().fields.iter()
                    .map(|f| (f.label.to_ascii_uppercase(), f.value.clone()))
                    .collect(),
            };

            let entry = st.focused_entry_mut();
            entry.clear_values();
            entry.esm_step = EsmStep::Idle;
            entry.last_logged_context = Some(last_ctx);
            if contest.auto_toggle_mode() {
                entry.mode = match entry.mode {
                    OpMode::Run => OpMode::Sp,
                    OpMode::Sp => OpMode::Run,
                };
            }

            let mut effects = Vec::new();
            if send_exch {
                effects.push(Effect::CwSend {
                    radio: st.focused_radio,
                    text: exch_text,
                });
            }
            // Emit one LogInsert per draft. For single-county QSOs this is
            // one effect; for rover splits it's N effects applied in order.
            for draft in drafts {
                effects.push(Effect::LogInsert { draft });
            }
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
    let entry = st.focused_entry();
    entry.fields.get(entry.focus).map(|f| f.field_id)
}

fn next_field_id(st: &AppState) -> Option<u16> {
    let entry = st.focused_entry();
    entry.fields.get(entry.focus + 1).map(|f| f.field_id)
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
    let entry = st.focused_entry_mut();
    if let Some(idx) = entry.first_invalid_index() {
        entry.focus = idx;
        let field_id = entry.fields[idx].field_id;
        effects.push(Effect::UiSetFocus { field_id });
    }
    effects
}

/// Claim the next serial number from the counter, storing it in entry state.
/// No-op if the contest doesn't use serials or a serial is already claimed.
fn claim_serial(st: &mut AppState, contest: &dyn ContestEntry) {
    if !contest.uses_serial() {
        return;
    }
    if st.focused_entry().assigned_serial.is_some() {
        return;
    }
    if let Some(counter) = st.serial_counter {
        st.focused_entry_mut().assigned_serial = Some(counter);
        st.serial_counter = Some(counter + 1);
    }
}

fn entry_ctx(st: &AppState) -> EntryContext {
    EntryContext {
        my_call: st.my_call.clone(),
        my_zone: st.my_zone,
        rst_sent: st.rst_sent.clone(),
        rig: st.radios.get(&st.focused_radio).cloned(),
        serial: st.focused_entry().assigned_serial,
    }
}
