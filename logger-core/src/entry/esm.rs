use crate::{
    contest::{normalize_mode, traits::{ContestEntry, EntryContext}},
    effects::{BeepKind, Effect},
    entry::state::{EntryState, EsmStep, OpMode},
    macro_expand::expand_macro,
    state::{AppState, LastLoggedContext, Macros},
};

/// Pre-fill the RST field with the contest's mode-appropriate default
/// (typically "599" on CW, "59" on SSB), but only if the field is currently
/// empty. Safe to call on contests without an RST field.
pub fn apply_default_rst(entry: &mut EntryState, contest: &dyn ContestEntry, mode: &str) {
    let Some(field_id) = contest.rst_field_id() else { return };
    let Some(value) = contest.default_rst(mode) else { return };
    if let Some(field) = entry.fields.iter_mut().find(|f| f.field_id == field_id)
        && field.value.is_empty() {
            field.cursor = value.len();
            field.value = value;
        }
}

fn focused_radio_mode(st: &AppState) -> &'static str {
    st.radios
        .get(&st.focused_radio)
        .map(|r| normalize_mode(&r.mode))
        .unwrap_or("CW")
}

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

    // Block dupes from being worked accidentally via Enter. F-key macros
    // remain available as the explicit override path. Empty calls are safe:
    // recompute_feedback forces is_dupe=false when the call field is empty.
    if st.focused_entry().block_dupes && st.focused_entry().is_dupe {
        return vec![Effect::Beep { kind: BeepKind::Error }];
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

    // Run is always two-step: Enter 1 sends call+exchange (so the operator can
    // copy the runner's reply); Enter 2 logs and sends TU. Collapsing these
    // into one would require logging before the received exchange is typed.
    if st.focused_entry().esm_step == EsmStep::Idle {
        st.focused_entry_mut().esm_step = EsmStep::ExchSent;
        claim_serial(st, contest);
        let mut effects = vec![Effect::CwSend {
            radio: st.focused_radio,
            text: compose_call_exchange(st, macros),
        }];
        // Auto-advance cursor past CALL field so operator can enter received exchange
        if focused_field_id(st) == Some(1)
            && let Some(next_id) = next_field_id(st) {
                st.focused_entry_mut().focus += 1;
                effects.push(Effect::UiSetFocus { field_id: next_id });
            }
        return effects;
    }

    if st.focused_entry().overall.is_invalid() {
        return invalid_focus_effects(st);
    }

    log_and_clear(st, contest, macros, true, false)
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

    // Cursor past CALL field: send my exchange and log atomically.
    // S&P doesn't send TU, so there's nothing to do after sending the exchange.
    if st.focused_entry().overall.is_invalid() {
        return invalid_focus_effects(st);
    }
    claim_serial(st, contest);
    let template = macros.sp_f2.as_deref().unwrap_or(&macros.f2);
    let mut effects = vec![Effect::CwSend {
        radio: st.focused_radio,
        text: expand_macro(template, st),
    }];
    effects.extend(log_and_clear(st, contest, macros, false, false));
    effects
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
            // Append every sent-exchange field resolved against the live
            // station config, so the log record preserves what we actually
            // sent (needed for Cabrillo log checking and RTC posting).
            // Emits keys like `sent_zone`, `sent_name`, `sent_prec`, etc.,
            // per the contest-engine spec's sent_variants field ids.
            let sent_pairs = contest.sent_exchange_pairs(&ctx);
            if !sent_pairs.is_empty() {
                for draft in &mut drafts {
                    draft.exchange_pairs.extend(sent_pairs.iter().cloned());
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
                serial: st.focused_entry().assigned_serial,
            };

            let mode = focused_radio_mode(st);
            let entry = st.focused_entry_mut();
            entry.clear_values();
            apply_default_rst(entry, contest, mode);
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
        station_config: st.station_config.clone(),
    }
}
