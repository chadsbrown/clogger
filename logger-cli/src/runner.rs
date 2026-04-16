use std::{
    collections::{BTreeMap, HashMap},
    fs,
};

use anyhow::{Context, Result, bail};
use logger_core::{
    AppEvent, AppState, BeepKind, CallHistoryLookup, ContestEntry, Effect, EntryState, Key, Macros,
    NoCallHistory, NoScp, OpMode, ScpLookup, Spot, contest_from_id, reduce,
};
use serde::Serialize;
use serde_json::Value;

use logger_runtime::{ConfigValue, LogAdapter, decode_exchange_pairs};

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
    score: logger_runtime::ScoreSummary,
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
    RigSet {
        radio: u8,
        freq_hz: u64,
    },
    CwAbort,
    UiClearEntry,
    So2rFocusChanged {
        radio: u8,
    },
}

#[derive(Debug, Clone, Serialize)]
struct TraceState {
    focused_radio: u8,
    entry_focus_index: usize,
    entry_focus_field_id: u16,
    esm_step: String,
    is_dupe: bool,
    is_new_mult: bool,
    is_passband_qrm: bool,
    overall: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scp_matches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    scp_n1_matches: Vec<String>,
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
        Self { records }
    }
}

impl CallHistoryLookup for ScriptCallHistory {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>> {
        self.records
            .get(call_norm)
            .map(|rec| rec.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}

struct ScriptScp {
    sorted_calls: Vec<String>,
}

impl ScpLookup for ScriptScp {
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

    fn n_plus_one_matches(&self, call: &str, limit: usize) -> Vec<String> {
        if call.is_empty() {
            return Vec::new();
        }
        self.sorted_calls
            .iter()
            .filter(|c| is_edit_distance_one(call, c))
            .take(limit)
            .cloned()
            .collect()
    }
}

fn is_edit_distance_one(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return false;
    }
    if la == lb {
        // substitution or transposition
        let diffs: Vec<usize> = (0..la).filter(|&i| a[i] != b[i]).collect();
        match diffs.len() {
            1 => true,
            2 => {
                // transposition
                a[diffs[0]] == b[diffs[1]] && a[diffs[1]] == b[diffs[0]]
            }
            _ => false,
        }
    } else {
        // insert/delete: the longer string has one extra char
        let (short, long) = if la < lb { (a, b) } else { (b, a) };
        let mut i = 0;
        let mut j = 0;
        let mut edits = 0;
        while i < short.len() && j < long.len() {
            if short[i] != long[j] {
                edits += 1;
                if edits > 1 {
                    return false;
                }
                j += 1;
            } else {
                i += 1;
                j += 1;
            }
        }
        true
    }
}

/// Wrapper that overrides `uses_serial()` on an inner contest.
struct SerialContest(Box<dyn ContestEntry>);

fn json_to_config_value(v: &serde_json::Value) -> Option<ConfigValue> {
    match v {
        serde_json::Value::Bool(b) => Some(ConfigValue::Bool(*b)),
        serde_json::Value::Number(n) => n.as_i64().map(ConfigValue::Int),
        serde_json::Value::String(s) => Some(ConfigValue::Text(s.clone())),
        _ => None,
    }
}

impl ContestEntry for SerialContest {
    fn contest_id(&self) -> &str { self.0.contest_id() }
    fn contest_instance_id(&self) -> u64 { self.0.contest_instance_id() }
    fn default_macros(&self) -> Macros { self.0.default_macros() }
    fn form_spec(&self) -> logger_core::entry::spec::EntryFormSpec { self.0.form_spec() }
    fn validate_entry(
        &self,
        input: &EntryState,
        ctx: &logger_core::EntryContext,
    ) -> logger_core::entry::validation::EntryValidation {
        self.0.validate_entry(input, ctx)
    }
    fn build_qso_draft(
        &self,
        input: &EntryState,
        ctx: &logger_core::EntryContext,
    ) -> Result<logger_core::QsoDraft, logger_core::EntryError> {
        self.0.build_qso_draft(input, ctx)
    }
    fn history_field_mapping(&self) -> Vec<(&str, u16)> { self.0.history_field_mapping() }
    fn uses_serial(&self) -> bool { true }
    fn auto_toggle_mode(&self) -> bool { self.0.auto_toggle_mode() }
    fn rst_field_id(&self) -> Option<u16> { self.0.rst_field_id() }
    fn default_rst(&self, mode: &str) -> Option<String> { self.0.default_rst(mode) }
}

fn execute_script(script: &Script, record_trace: bool) -> Result<RunArtifacts> {
    let contest_id = script
        .contest
        .as_deref()
        .unwrap_or("cqww")
        .to_ascii_lowercase();
    let contest: Box<dyn ContestEntry> = {
        let base = contest_from_id(&contest_id)
            .ok_or_else(|| anyhow::anyhow!("unknown contest: {contest_id}"))?;
        if script.uses_serial {
            Box::new(SerialContest(base))
        } else {
            base
        }
    };
    let mut macros = contest.default_macros();
    let ov = &script.macro_overrides;
    if let Some(ref v) = ov.f1 { macros.f1 = v.clone(); }
    if let Some(ref v) = ov.f2 { macros.f2 = v.clone(); }
    if let Some(ref v) = ov.f3 { macros.f3 = v.clone(); }
    if let Some(ref v) = ov.f5 { macros.f5 = v.clone(); }
    if let Some(ref v) = ov.f7 { macros.f7 = v.clone(); }
    if let Some(ref v) = ov.f8 { macros.f8 = v.clone(); }
    if let Some(ref v) = ov.f9 { macros.f9 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f1 { macros.ctrl_alt_f1 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f2 { macros.ctrl_alt_f2 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f3 { macros.ctrl_alt_f3 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f4 { macros.ctrl_alt_f4 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f5 { macros.ctrl_alt_f5 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f6 { macros.ctrl_alt_f6 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f7 { macros.ctrl_alt_f7 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f8 { macros.ctrl_alt_f8 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f9 { macros.ctrl_alt_f9 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f10 { macros.ctrl_alt_f10 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f11 { macros.ctrl_alt_f11 = v.clone(); }
    if let Some(ref v) = ov.ctrl_alt_f12 { macros.ctrl_alt_f12 = v.clone(); }

    let mut entries = HashMap::new();
    let mut entry1 = EntryState::from_spec(&contest.form_spec());
    let mut entry2 = EntryState::from_spec(&contest.form_spec());
    entry1.block_dupes = script.block_dupes;
    entry2.block_dupes = script.block_dupes;
    logger_core::apply_default_rst(&mut entry1, contest.as_ref(), "CW");
    logger_core::apply_default_rst(&mut entry2, contest.as_ref(), "CW");
    entries.insert(1, entry1);
    entries.insert(2, entry2);
    let mut st = AppState {
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
        my_exchange: script.my_exchange.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        bandmap_cursors: HashMap::new(),
        default_cw_speed: 28,
        serial_counter: None,
        show_passband_qrm: script.show_passband_qrm,
        bandmap_version: 0,
    };
    if contest.uses_serial() {
        st.serial_counter = Some(1);
    }

    let mut keyer = FakeKeyer::default();

    // Build typed config map for contest-engine: my_cq_zone + back-compat
    // my_name/my_xchg/my_loc + station_config typed passthrough.
    let mut scorer_config: HashMap<String, ConfigValue> = HashMap::new();
    scorer_config.insert(
        "my_cq_zone".to_string(),
        ConfigValue::Int(i64::from(st.my_zone)),
    );
    for (k, v) in &st.my_exchange {
        scorer_config.insert(
            format!("my_{}", k.to_ascii_lowercase()),
            ConfigValue::Text(v.clone()),
        );
    }
    for (k, v) in &script.station_config {
        let Some(cv) = json_to_config_value(v) else {
            bail!("station_config[{}]: unsupported JSON type (must be bool, integer, or string)", k);
        };
        scorer_config.insert(k.clone(), cv);
    }

    let scorer = logger_runtime::scorer_for_contest(contest.as_ref(), scorer_config)?;
    let mut log = LogAdapter::new(scorer, contest.contest_instance_id());
    let mut rig = FakeRig::default();
    let mut beep_error_count = 0usize;
    let mut trace = Vec::new();

    let call_history: Box<dyn CallHistoryLookup> = if script.call_history.is_empty() {
        Box::new(NoCallHistory)
    } else {
        Box::new(ScriptCallHistory::from_entries(&script.call_history))
    };

    let scp: Box<dyn ScpLookup> = if script.scp_calls.is_empty() {
        Box::new(NoScp)
    } else {
        let mut sorted = script.scp_calls.iter().map(|c| c.to_ascii_uppercase()).collect::<Vec<_>>();
        sorted.sort();
        sorted.dedup();
        Box::new(ScriptScp { sorted_calls: sorted })
    };

    for script_event in script.events.iter().cloned() {
        let app_event = match script_event.clone() {
            ScriptEvent::RigStatus {
                radio,
                freq_hz,
                mode,
                is_ptt,
                filter_width_hz,
            } => Some(AppEvent::RigStatus {
                radio,
                freq_hz,
                mode,
                is_ptt,
                filter_width_hz,
            }),
            ScriptEvent::SetMode { mode } => Some(AppEvent::SetOpMode {
                mode: match mode {
                    crate::script::ModeValue::Run => OpMode::Run,
                    crate::script::ModeValue::Sp => OpMode::Sp,
                },
            }),
            ScriptEvent::FocusRadio { radio } => Some(AppEvent::FocusRadio { radio }),
            ScriptEvent::SwapRadios => Some(AppEvent::SwapRadios),
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
                    KeyValue::F4 => Key::F4,
                    KeyValue::Enter => Key::Enter,
                    KeyValue::Equal => Key::Equal,
                },
            }),
            ScriptEvent::Esm => Some(AppEvent::EsmTrigger),
            ScriptEvent::QuickLog => Some(AppEvent::QuickLog),
            ScriptEvent::Spot { call, freq_hz, mode } => Some(AppEvent::SpotReceived {
                spot: Spot { call, freq_hz, mode },
            }),
            ScriptEvent::SpotWithdraw { call } => Some(AppEvent::SpotWithdrawn { call }),
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
                scp.as_ref(),
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
                            st.focused_entry().fields.iter().position(|f| f.field_id == field_id)
                        {
                            st.focused_entry_mut().focus = idx;
                        }
                    }
                    Effect::CwAbort => {}
                    Effect::RigSet { .. } => {}
                    Effect::UiClearEntry => {
                        // state already reflects clear behavior in reducer
                    }
                    Effect::So2rFocusChanged { .. } => {
                        // SO2R hardware switching is a no-op in script tests
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
                    entry_focus_index: st.focused_entry().focus,
                    entry_focus_field_id: st
                        .focused_entry()
                        .fields
                        .get(st.focused_entry().focus)
                        .map(|f| f.field_id)
                        .unwrap_or(0),
                    esm_step: format!("{:?}", st.focused_entry().esm_step),
                    is_dupe: st.focused_entry().is_dupe,
                    is_new_mult: st.focused_entry().is_new_mult,
                    is_passband_qrm: st.focused_entry().is_passband_qrm,
                    overall: normalize_overall(&st.focused_entry().overall),
                    scp_matches: st.focused_entry().scp_matches.clone(),
                    scp_n1_matches: st.focused_entry().scp_n1_matches.clone(),
                },
            });
        }

        for (radio_id, state) in &st.radios {
            rig.set(*radio_id, state.clone());
        }
    }

    let full_cw = keyer.joined_text();
    let cw_sent = keyer.sent.iter().map(|(_, t)| t.clone()).collect();

    let score = log.score_summary();
    Ok(RunArtifacts {
        st,
        records: log.ordered_records(),
        full_cw,
        cw_sent,
        beep_error_count,
        trace,
        score,
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
        let entry = artifacts.st.focused_entry();
        let got = entry
            .fields
            .get(entry.focus)
            .map(|f| f.field_id)
            .unwrap_or(0);
        if got != expected_field_id {
            bail!("expected focus field id {}, got {}", expected_field_id, got);
        }
    }
    if let Some(expected) = script.expectations.final_is_dupe
        && artifacts.st.focused_entry().is_dupe != expected
    {
        bail!(
            "expected final is_dupe {}, got {}",
            expected,
            artifacts.st.focused_entry().is_dupe
        );
    }
    if let Some(expected) = script.expectations.final_is_new_mult
        && artifacts.st.focused_entry().is_new_mult != expected
    {
        bail!(
            "expected final is_new_mult {}, got {}",
            expected,
            artifacts.st.focused_entry().is_new_mult
        );
    }
    if let Some(expected) = script.expectations.final_is_passband_qrm
        && artifacts.st.focused_entry().is_passband_qrm != expected
    {
        bail!(
            "expected final is_passband_qrm {}, got {}",
            expected,
            artifacts.st.focused_entry().is_passband_qrm
        );
    }

    if let Some(expected_fields) = &script.expectations.final_field_values {
        for (field_id, expected_val) in expected_fields {
            let got = artifacts
                .st
                .focused_entry()
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

    if let Some(expected) = script.expectations.final_serial_counter
        && artifacts.st.serial_counter != Some(expected)
    {
        bail!(
            "expected serial_counter {:?}, got {:?}",
            expected,
            artifacts.st.serial_counter
        );
    }

    if let Some(expected) = script.expectations.final_focused_radio
        && artifacts.st.focused_radio != expected
    {
        bail!(
            "expected focused_radio {}, got {}",
            expected,
            artifacts.st.focused_radio
        );
    }

    for (radio_id, exp) in &script.expectations.final_radio_entries {
        let entry = artifacts.st.entry_for(*radio_id).ok_or_else(|| {
            anyhow::anyhow!("no entry for radio {radio_id}")
        })?;
        if let Some(expected_mode) = &exp.op_mode {
            let got = format!("{:?}", entry.mode);
            if got != *expected_mode {
                bail!(
                    "radio {} expected op_mode {:?}, got {:?}",
                    radio_id,
                    expected_mode,
                    got
                );
            }
        }
        if let Some(expected_step) = &exp.esm_step {
            let got = format!("{:?}", entry.esm_step);
            if got != *expected_step {
                bail!(
                    "radio {} expected esm_step {:?}, got {:?}",
                    radio_id,
                    expected_step,
                    got
                );
            }
        }
        if let Some(expected_fields) = &exp.field_values {
            for (field_id, expected_val) in expected_fields {
                let got = entry.get_field_value_by_id(*field_id).unwrap_or("");
                if got != expected_val {
                    bail!(
                        "radio {} field {} expected {:?}, got {:?}",
                        radio_id,
                        field_id,
                        expected_val,
                        got
                    );
                }
            }
        }
        if let Some(expected_field_id) = exp.focus_field_id {
            let got = entry
                .fields
                .get(entry.focus)
                .map(|f| f.field_id)
                .unwrap_or(0);
            if got != expected_field_id {
                bail!(
                    "radio {} expected focus field {}, got {}",
                    radio_id,
                    expected_field_id,
                    got
                );
            }
        }
    }

    if let Some(expected_cursors) = &script.expectations.final_bandmap_cursors {
        for (radio_id, expected) in expected_cursors {
            let got = artifacts.st.bandmap_cursors.get(radio_id).copied();
            if got != Some(*expected) {
                bail!(
                    "radio {} expected bandmap_cursor {:?}, got {:?}",
                    radio_id,
                    expected,
                    got
                );
            }
        }
    }

    if let Some(expected) = script.expectations.final_claimed_score
        && artifacts.score.claimed_score != expected
    {
        bail!(
            "expected final_claimed_score {}, got {}",
            expected,
            artifacts.score.claimed_score
        );
    }
    if let Some(expected) = script.expectations.final_total_qsos
        && artifacts.score.total_qsos != expected
    {
        bail!(
            "expected final_total_qsos {}, got {}",
            expected,
            artifacts.score.total_qsos
        );
    }
    if let Some(expected) = script.expectations.final_total_mults
        && artifacts.score.total_mults != expected
    {
        bail!(
            "expected final_total_mults {}, got {}",
            expected,
            artifacts.score.total_mults
        );
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
        Effect::RigSet { radio, freq_hz } => TraceEffect::RigSet {
            radio: *radio,
            freq_hz: *freq_hz,
        },
        Effect::CwAbort => TraceEffect::CwAbort,
        Effect::UiClearEntry => TraceEffect::UiClearEntry,
        Effect::So2rFocusChanged { radio } => TraceEffect::So2rFocusChanged { radio: *radio },
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
            "cqww_rst_autopop.json",
            "cqww_sp_one_step.json",
            "cqww_sp_send_tu.json",
            "cqww_dupe_indicator.json",
            "cqww_block_dupes.json",
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
            "cqww_serial_number.json",
            "mst_run_two_step.json",
            "so2r_preserve_entry_across_swap.json",
            "so2r_independent_op_mode.json",
            "so2r_concurrent_inflight_qsos.json",
            "so2r_serial_shared_counter.json",
            "cqww_passband_qrm.json",
            "bandmap_snap_to_freq.json",
            "miqp_out_of_state_log.json",
            "flqp_in_state_log.json",
            "nmqp_power_class_out_of_state.json",
            "flqp_rover_county_log.json",
            "moqp_rover_county_log.json",
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
