use std::{
    collections::{BTreeMap, HashMap},
    fs,
};

use anyhow::{Context, Result, bail};
use logger_core::{
    AppEvent, AppState, BeepKind, CallHistoryLookup, Effect, EntryState, EsmPolicy, Key,
    NoCallHistory, OpMode, Spot, contest_from_id, reduce,
};
use serde::Serialize;
use serde_json::Value;

use logger_runtime::{LogAdapter, decode_exchange_pairs};

use crate::{
    fakes::{fake_keyer::FakeKeyer, fake_rig::FakeRig},
    script::{KeyValue, Script, ScriptEvent},
};

#[derive(Debug)]
struct RunArtifacts {
    st: AppState,
    records: Vec<qsolog::qso::QsoRecord>,
    full_cw: String,
    cw_sent: Vec<String>,
    beep_error_count: usize,
    trace: Vec<TraceStep>,
}

#[derive(Debug, Clone, Serialize)]
struct TraceStep {
    event: Value,
    effects: Vec<TraceEffect>,
    state_after: TraceState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum TraceEffect {
    CwSend {
        radio: u8,
        text: String,
    },
    LogInsert {
        callsign: String,
        exchange_pairs: Vec<(String, String)>,
    },
    Beep {
        kind: String,
    },
    UiSetFocus {
        field_id: u16,
    },
    UiClearEntry,
}

#[derive(Debug, Clone, Serialize)]
struct TraceState {
    focused_radio: u8,
    entry_focus_index: usize,
    entry_focus_field_id: u16,
    esm_step: String,
    is_dupe: bool,
    is_new_mult: bool,
    overall: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scp_matches: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TraceSnapshot {
    script: String,
    steps: Vec<TraceStep>,
}

pub fn run_script_file(path: &str) -> Result<()> {
    let data = fs::read_to_string(path).with_context(|| format!("read script: {path}"))?;
    let script: Script =
        serde_json::from_str(&data).with_context(|| format!("parse script: {path}"))?;
    run_script(script)
}

pub fn run_script(script: Script) -> Result<()> {
    let artifacts = execute_script(&script, false)?;
    validate_expectations(&artifacts, &script)
}

struct ScriptCallHistory {
    records: HashMap<String, HashMap<String, String>>,
    sorted_calls: Vec<String>,
}

impl ScriptCallHistory {
    fn from_entries(entries: &[crate::script::CallHistoryEntry]) -> Self {
        let mut records: HashMap<String, HashMap<String, String>> = HashMap::new();
        for entry in entries {
            let call = entry.call.to_ascii_uppercase();
            let fields: HashMap<String, String> = entry
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            records.insert(call, fields);
        }
        let mut sorted_calls: Vec<String> = records.keys().cloned().collect();
        sorted_calls.sort();
        Self {
            records,
            sorted_calls,
        }
    }
}

impl CallHistoryLookup for ScriptCallHistory {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
        self.records
            .get(call_norm)
            .map(|rec| rec.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    fn partial_matches(&self, prefix: &str, limit: usize) -> Vec<String> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let start = self.sorted_calls.partition_point(|c| c.as_str() < prefix);
        self.sorted_calls[start..]
            .iter()
            .take_while(|c| c.starts_with(prefix))
            .take(limit)
            .cloned()
            .collect()
    }
}

fn execute_script(script: &Script, record_trace: bool) -> Result<RunArtifacts> {
    let contest_id = script
        .contest
        .as_deref()
        .unwrap_or("cqww")
        .to_ascii_lowercase();
    let contest = contest_from_id(&contest_id)
        .ok_or_else(|| anyhow::anyhow!("unknown contest: {contest_id}"))?;
    let macros = contest.default_macros();

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
    let mut log = LogAdapter::new(contest.contest_id(), st.my_zone);
    let mut rig = FakeRig::default();
    let mut beep_error_count = 0usize;
    let mut trace = Vec::new();

    let call_history: Box<dyn CallHistoryLookup> = if script.call_history.is_empty() {
        Box::new(NoCallHistory)
    } else {
        Box::new(ScriptCallHistory::from_entries(&script.call_history))
    };

    for script_event in script.events.iter().cloned() {
        let app_event = match script_event.clone() {
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
                    crate::script::ModeValue::Run => OpMode::Run,
                    crate::script::ModeValue::Sp => OpMode::Sp,
                },
            }),
            ScriptEvent::FocusRadio { radio } => Some(AppEvent::FocusRadio { radio }),
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

        let mut trace_effects = Vec::new();
        if let Some(ev) = app_event {
            let effects = reduce(
                &mut st,
                contest.as_ref(),
                &macros,
                &log,
                &log,
                call_history.as_ref(),
                ev,
            );
            if record_trace {
                trace_effects = effects.iter().map(normalize_effect).collect();
            }
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
                        if let Some(idx) =
                            st.entry.fields.iter().position(|f| f.field_id == field_id)
                        {
                            st.entry.focus = idx;
                        }
                    }
                    Effect::UiClearEntry => {
                        // state already reflects clear behavior in reducer
                    }
                }
            }
        }

        if record_trace {
            trace.push(TraceStep {
                event: serde_json::to_value(&script_event).unwrap_or(Value::Null),
                effects: trace_effects,
                state_after: TraceState {
                    focused_radio: st.focused_radio,
                    entry_focus_index: st.entry.focus,
                    entry_focus_field_id: st
                        .entry
                        .fields
                        .get(st.entry.focus)
                        .map(|f| f.field_id)
                        .unwrap_or(0),
                    esm_step: format!("{:?}", st.entry.esm_step),
                    is_dupe: st.entry.is_dupe,
                    is_new_mult: st.entry.is_new_mult,
                    overall: normalize_overall(&st.entry.overall),
                    scp_matches: st.entry.scp_matches.clone(),
                },
            });
        }

        for (radio_id, state) in &st.radios {
            rig.set(*radio_id, state.clone());
        }
    }

    let full_cw = keyer.joined_text();
    let cw_sent = keyer.sent.iter().map(|(_, t)| t.clone()).collect();

    Ok(RunArtifacts {
        st,
        records: log.ordered_records(),
        full_cw,
        cw_sent,
        beep_error_count,
        trace,
    })
}

fn validate_expectations(artifacts: &RunArtifacts, script: &Script) -> Result<()> {
    if script.expectations.qsos.len() != artifacts.records.len() {
        bail!(
            "expected {} qsos, got {}",
            script.expectations.qsos.len(),
            artifacts.records.len()
        );
    }

    for (exp, got) in script
        .expectations
        .qsos
        .iter()
        .zip(artifacts.records.iter())
    {
        if exp.call.to_uppercase() != got.callsign_norm {
            bail!(
                "qso mismatch expected call {} got {}",
                exp.call,
                got.callsign_norm
            );
        }
        if let Some(exp_band) = &exp.band {
            let got_band = match got.band {
                qsolog::types::Band::B160m => "160m",
                qsolog::types::Band::B80m => "80m",
                qsolog::types::Band::B40m => "40m",
                qsolog::types::Band::B20m => "20m",
                qsolog::types::Band::B15m => "15m",
                qsolog::types::Band::B10m => "10m",
                qsolog::types::Band::Other => "other",
            };
            if exp_band.to_ascii_lowercase() != got_band {
                bail!("qso mismatch expected band {} got {}", exp_band, got_band);
            }
        }

        let pairs = decode_exchange_pairs(&got.exchange)?;
        let got_map: BTreeMap<String, String> = pairs.into_iter().collect();
        if let Some(rst) = &exp.rst
            && got_map.get("rst") != Some(rst)
        {
            bail!(
                "qso mismatch expected rst {} got {:?}",
                rst,
                got_map.get("rst")
            );
        }
        if let Some(zone) = exp.zone {
            let got_zone = got_map.get("zone").and_then(|z| z.parse::<u8>().ok());
            if got_zone != Some(zone) {
                bail!(
                    "qso mismatch expected zone {} got {:?}",
                    zone,
                    got_map.get("zone")
                );
            }
        }
        if let Some(exchange) = &exp.exchange {
            for (k, v) in exchange {
                if got_map.get(k) != Some(v) {
                    bail!(
                        "qso mismatch exchange {} expected {} got {:?}",
                        k,
                        v,
                        got_map.get(k)
                    );
                }
            }
        }
    }

    let mut cursor = 0usize;
    for needle in &script.expectations.cw_sent_contains {
        if let Some(pos) = artifacts.full_cw[cursor..].find(needle) {
            cursor += pos + needle.len();
        } else {
            bail!("expected CW output to contain in order: {needle}");
        }
    }
    if !script.expectations.cw_sent_exact.is_empty()
        && artifacts.cw_sent != script.expectations.cw_sent_exact
    {
        bail!(
            "expected exact CW {:?}, got {:?}",
            script.expectations.cw_sent_exact,
            artifacts.cw_sent
        );
    }

    if let Some(expected) = script.expectations.beep_error_count
        && expected != artifacts.beep_error_count
    {
        bail!(
            "expected {} error beeps, got {}",
            expected,
            artifacts.beep_error_count
        );
    }

    if let Some(expected_field_id) = script.expectations.focus_field_id {
        let got = artifacts
            .st
            .entry
            .fields
            .get(artifacts.st.entry.focus)
            .map(|f| f.field_id)
            .unwrap_or(0);
        if got != expected_field_id {
            bail!("expected focus field id {}, got {}", expected_field_id, got);
        }
    }
    if let Some(expected) = script.expectations.final_is_dupe
        && artifacts.st.entry.is_dupe != expected
    {
        bail!(
            "expected final is_dupe {}, got {}",
            expected,
            artifacts.st.entry.is_dupe
        );
    }
    if let Some(expected) = script.expectations.final_is_new_mult
        && artifacts.st.entry.is_new_mult != expected
    {
        bail!(
            "expected final is_new_mult {}, got {}",
            expected,
            artifacts.st.entry.is_new_mult
        );
    }

    if let Some(expected_fields) = &script.expectations.final_field_values {
        for (field_id, expected_val) in expected_fields {
            let got = artifacts
                .st
                .entry
                .get_field_value_by_id(*field_id)
                .unwrap_or("");
            if got != expected_val {
                bail!(
                    "expected field {} value {:?}, got {:?}",
                    field_id,
                    expected_val,
                    got
                );
            }
        }
    }

    Ok(())
}

fn normalize_effect(effect: &Effect) -> TraceEffect {
    match effect {
        Effect::CwSend { radio, text } => TraceEffect::CwSend {
            radio: *radio,
            text: text.clone(),
        },
        Effect::LogInsert { draft } => TraceEffect::LogInsert {
            callsign: draft.callsign.clone(),
            exchange_pairs: draft.exchange_pairs.clone(),
        },
        Effect::Beep { kind } => TraceEffect::Beep {
            kind: format!("{:?}", kind),
        },
        Effect::UiSetFocus { field_id } => TraceEffect::UiSetFocus {
            field_id: *field_id,
        },
        Effect::UiClearEntry => TraceEffect::UiClearEntry,
    }
}

fn normalize_overall(v: &logger_core::Validation) -> String {
    match v {
        logger_core::Validation::Unknown => "Unknown".to_string(),
        logger_core::Validation::Valid => "Valid".to_string(),
        logger_core::Validation::Invalid(_) => "Invalid".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{TraceSnapshot, execute_script, run_script_file};
    use crate::script::Script;

    #[test]
    fn run_all_golden_scripts() {
        let base = format!("{}/../scripts", env!("CARGO_MANIFEST_DIR"));
        let scripts = [
            "cqww_run_two_step.json",
            "cqww_run_invalid.json",
            "cqww_sp_one_step.json",
            "cqww_sp_send_tu.json",
            "cqww_dupe_indicator.json",
            "cqww_new_mult_indicator.json",
            "cqww_run_exch_sent_edit_resets.json",
            "sweeps_run_two_step.json",
            "sweeps_invalid_focus.json",
            "sweeps_dupe_indicator.json",
            "sweeps_run_exch_sent_edit_resets.json",
            "so2r_focus_dupe_band_separation.json",
            "so2r_focus_mult_per_band.json",
            "cwt_call_history_prepopulate.json",
            "cqww_call_history_operator_override.json",
        ];

        for script in scripts {
            let path = format!("{base}/{script}");
            run_script_file(&path).unwrap_or_else(|e| panic!("{script} failed: {e}"));
        }
    }

    #[test]
    fn snapshot_regressions() {
        let base = format!("{}/../scripts", env!("CARGO_MANIFEST_DIR"));
        let scripts = [
            "cqww_run_two_step.json",
            "cqww_run_exch_sent_edit_resets.json",
            "cqww_dupe_indicator.json",
            "cqww_new_mult_indicator.json",
            "so2r_focus_dupe_band_separation.json",
        ];
        let update = std::env::var("UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");

        for script_name in scripts {
            let script_path = format!("{base}/{script_name}");
            let data = std::fs::read_to_string(&script_path).expect("read script");
            let script: Script = serde_json::from_str(&data).expect("parse script");
            let artifacts = execute_script(&script, true).expect("run script with trace");

            let snapshot = TraceSnapshot {
                script: script_name.to_string(),
                steps: artifacts.trace,
            };
            let snapshot_json = serde_json::to_string_pretty(&snapshot).expect("serialize trace");

            let stem = script_name.strip_suffix(".json").unwrap_or(script_name);
            let snapshot_path = format!("{base}/snapshots/{stem}.trace.json");
            if update {
                if let Some(parent) = std::path::Path::new(&snapshot_path).parent() {
                    std::fs::create_dir_all(parent).expect("create snapshot dir");
                }
                std::fs::write(&snapshot_path, snapshot_json.as_bytes()).expect("write snapshot");
                continue;
            }

            let expected_raw = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "missing snapshot {} (set UPDATE_SNAPSHOTS=1)",
                    snapshot_path
                )
            });
            let expected_val: serde_json::Value =
                serde_json::from_str(&expected_raw).expect("parse expected snapshot");
            let actual_val: serde_json::Value =
                serde_json::from_str(&snapshot_json).expect("parse actual snapshot");
            assert_eq!(
                expected_val, actual_val,
                "snapshot mismatch for {} (set UPDATE_SNAPSHOTS=1)",
                script_name
            );
        }
    }
}
