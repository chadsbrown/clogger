use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result, bail};
use logger_core::{
    AppEvent, AppState, BeepKind, ContestEntry, CqwwContest, Effect, EntryState, EsmPolicy, Key,
    Macros, OpMode, Spot, SweepsContest, reduce,
};

use crate::{
    fakes::{
        fake_keyer::FakeKeyer,
        fake_rig::FakeRig,
        qso_log_adapter::{QsoLogAdapter, decode_exchange_pairs},
    },
    script::{ContestValue, KeyValue, ModeValue, Script, ScriptEvent},
};

pub fn run_script_file(path: &str) -> Result<()> {
    let data = std::fs::read_to_string(path).with_context(|| format!("read script: {path}"))?;
    let script: Script = serde_json::from_str(&data).with_context(|| format!("parse script: {path}"))?;
    run_script(script)
}

pub fn run_script(script: Script) -> Result<()> {
    let contest_kind = script.contest.unwrap_or(ContestValue::Cqww);
    let (contest, macros): (Box<dyn ContestEntry>, Macros) = match contest_kind {
        ContestValue::Cqww => (Box::new(CqwwContest::default()), Macros::default()),
        ContestValue::Sweeps => (
            Box::new(SweepsContest),
            Macros {
                f1: "CQ SS {MYCALL}".to_string(),
                f2: "{CALL} {NR} {PREC} {CHECK} {SECTION}".to_string(),
                f3: "TU {CALL}".to_string(),
            },
        ),
    };

    let mut st = AppState {
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
    };
    if let Some(v) = script.esm_policy.run_two_step {
        st.esm_policy.run_two_step = v;
    }
    if let Some(v) = script.esm_policy.sp_log_on_first_enter {
        st.esm_policy.sp_log_on_first_enter = v;
    }
    if let Some(v) = script.esm_policy.sp_send_tu {
        st.esm_policy.sp_send_tu = v;
    }

    let mut keyer = FakeKeyer::default();
    let mut log = QsoLogAdapter::new();
    let mut rig = FakeRig::default();
    let mut beep_error_count = 0usize;

    for ev in script.events {
        let app_event = match ev {
            ScriptEvent::RigStatus {
                radio,
                freq_hz,
                mode,
                is_ptt,
            } => Some(AppEvent::RigStatus {
                radio,
                freq_hz,
                mode,
                is_ptt,
            }),
            ScriptEvent::SetMode { mode } => Some(AppEvent::SetOpMode {
                mode: match mode {
                    ModeValue::Run => OpMode::Run,
                    ModeValue::Sp => OpMode::Sp,
                },
            }),
            ScriptEvent::Text { s } => Some(AppEvent::TextInput { s }),
            ScriptEvent::Key { key } => Some(AppEvent::KeyPress {
                key: match key {
                    KeyValue::Space => Key::Space,
                    KeyValue::Tab => Key::Tab,
                    KeyValue::Backspace => Key::Backspace,
                    KeyValue::Esc => Key::Esc,
                    KeyValue::F1 => Key::F1,
                    KeyValue::F2 => Key::F2,
                    KeyValue::F3 => Key::F3,
                    KeyValue::Enter => Key::Enter,
                },
            }),
            ScriptEvent::Esm => Some(AppEvent::EsmTrigger),
            ScriptEvent::Spot { call, freq_hz } => Some(AppEvent::SpotReceived {
                spot: Spot { call, freq_hz },
            }),
        };

        if let Some(ev) = app_event {
            let effects = reduce(&mut st, contest.as_ref(), &macros, &log, ev);
            for effect in effects {
                match effect {
                    Effect::CwSend { radio, text } => keyer.send(radio, text),
                    Effect::LogInsert { draft } => {
                        let id = log.insert(
                            draft,
                            st.now_ms.max(0) as u64,
                            st.focused_radio as u32,
                            st.active_operator as u32,
                        )?;
                        st.last_logged = Some(id);
                    }
                    Effect::Beep { kind } => {
                        if kind == BeepKind::Error {
                            beep_error_count += 1;
                        }
                    }
                    Effect::UiSetFocus { field_id } => {
                        if let Some(idx) = st.entry.fields.iter().position(|f| f.field_id == field_id) {
                            st.entry.focus = idx;
                        }
                    }
                    Effect::UiClearEntry => {
                        // state already reflects clear behavior in reducer
                    }
                }
            }
        }

        for (radio_id, state) in &st.radios {
            rig.set(*radio_id, state.clone());
        }
    }

    let records = log.ordered_records();
    if script.expectations.qsos.len() != records.len() {
        bail!(
            "expected {} qsos, got {}",
            script.expectations.qsos.len(),
            records.len()
        );
    }

    for (exp, got) in script.expectations.qsos.iter().zip(records.iter()) {
        if exp.call.to_uppercase() != got.callsign_norm {
            bail!(
                "qso mismatch expected call {} got {}",
                exp.call,
                got.callsign_norm
            );
        }

        let pairs = decode_exchange_pairs(&got.exchange)?;
        let got_map: BTreeMap<String, String> = pairs.into_iter().collect();
        if let Some(rst) = &exp.rst
            && got_map.get("rst") != Some(rst)
        {
            bail!("qso mismatch expected rst {} got {:?}", rst, got_map.get("rst"));
        }
        if let Some(zone) = exp.zone {
            let z = zone.to_string();
            if got_map.get("zone") != Some(&z) {
                bail!("qso mismatch expected zone {} got {:?}", zone, got_map.get("zone"));
            }
        }
        if let Some(exchange) = &exp.exchange {
            for (k, v) in exchange {
                if got_map.get(k) != Some(v) {
                    bail!("qso mismatch exchange {} expected {} got {:?}", k, v, got_map.get(k));
                }
            }
        }
    }

    let full_cw = keyer.joined_text();
    let mut cursor = 0usize;
    for needle in &script.expectations.cw_sent_contains {
        if let Some(pos) = full_cw[cursor..].find(needle) {
            cursor += pos + needle.len();
        } else {
            bail!("expected CW output to contain in order: {needle}");
        }
    }

    if let Some(expected) = script.expectations.beep_error_count
        && expected != beep_error_count
    {
        bail!("expected {} error beeps, got {}", expected, beep_error_count);
    }

    if let Some(expected_field_id) = script.expectations.focus_field_id {
        let got = st.entry.fields.get(st.entry.focus).map(|f| f.field_id).unwrap_or(0);
        if got != expected_field_id {
            bail!("expected focus field id {}, got {}", expected_field_id, got);
        }
    }
    if let Some(expected) = script.expectations.final_is_dupe
        && st.entry.is_dupe != expected
    {
        bail!(
            "expected final is_dupe {}, got {}",
            expected,
            st.entry.is_dupe
        );
    }

    Ok(())
}
