use crate::state::AppState;

pub fn expand_macro(template: &str, st: &AppState) -> String {
    let call = st.current_call();
    let entry = st.focused_entry();

    // When call is empty, fall back to last logged context for repeats
    let (effective_call, prev_fields) = if call.is_empty() {
        if let Some(ctx) = &entry.last_logged_context {
            (ctx.call.as_str(), Some(&ctx.fields))
        } else {
            (call.as_str(), None)
        }
    } else {
        (call.as_str(), None)
    };

    let my_zone = st.my_zone.to_string();
    let serial_str = entry
        .assigned_serial
        .map(|n| n.to_string())
        .unwrap_or_default();
    let base = [
        ("{MYCALL}", st.my_call.as_str()),
        ("{MYZONE}", my_zone.as_str()),
        ("{RST_SENT}", st.rst_sent.as_str()),
        ("{CALL}", effective_call),
        ("{SERIAL}", serial_str.as_str()),
    ];

    let mut out = base
        .into_iter()
        .fold(template.to_string(), |acc, (k, v)| acc.replace(k, v));

    // Expand {MY<KEY>} from my_exchange map (e.g. {MYNAME}, {MYXCHG})
    for (key, val) in &st.my_exchange {
        let token = format!("{{MY{}}}", key.to_ascii_uppercase());
        out = out.replace(&token, val);
    }

    // Expand field labels — use previous context if available (repeat mode)
    let out = if let Some(fields) = prev_fields {
        fields.iter().fold(out, |acc, (label, value)| {
            let token = format!("{{{}}}", label);
            acc.replace(&token, value.trim())
        })
    } else {
        entry.fields.iter().fold(out, |acc, f| {
            let token = format!("{{{}}}", f.label.to_ascii_uppercase());
            acc.replace(&token, f.value.trim())
        })
    };

    // Convert < and > speed markers to absolute {WPM} commands.
    // Known prosigns (<AR>, <SK>, <BT>, <KN>, <AS>) are preserved.
    let cw_speed = st
        .radios
        .get(&st.focused_radio)
        .map(|r| r.cw_speed)
        .unwrap_or(st.default_cw_speed);
    expand_speed_markers(&out, cw_speed)
}

const SPEED_STEP: u8 = 2;
const PROSIGNS: &[&str] = &["AR", "SK", "BT", "KN", "AS"];

fn expand_speed_markers(input: &str, cw_speed: u8) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut current = cw_speed as i16;

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Check if this is a prosign: <XX> where XX is a known prosign
            let rest: String = chars.clone().take_while(|&c| c != '>').collect();
            if PROSIGNS.contains(&rest.to_ascii_uppercase().as_str()) {
                // Consume through '>'
                for _ in 0..rest.len() {
                    chars.next();
                }
                chars.next(); // consume '>'
                out.push('<');
                out.push_str(&rest);
                out.push('>');
            } else {
                // Speed down
                current = (current - SPEED_STEP as i16).max(0);
                out.push_str(&format!("{{{current}}}"));
            }
        } else if ch == '>' {
            // Speed up
            current = (current + SPEED_STEP as i16).min(99);
            out.push_str(&format!("{{{current}}}"));
        } else {
            out.push(ch);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        CqwwContest,
        contest::traits::ContestEntry,
        entry::state::EntryState,
        state::{AppState, EsmPolicy},
    };

    use super::expand_macro;

    use crate::state::RadioState;

    #[test]
    fn speed_markers_converted() {
        let contest = CqwwContest::default();
        let mut st = AppState {
            now_ms: 0,
            focused_radio: 1,
            active_operator: 1,
            radios: HashMap::new(),
            entries: HashMap::from([(1, EntryState::from_spec(&contest.form_spec()))]),
            bandmap: Vec::new(),
            last_logged: None,
            my_call: "N0CALL".to_string(),
            my_zone: 4,
            rst_sent: "599".to_string(),
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursor: None,
            default_cw_speed: 28,
            serial_counter: None,
        };
        st.radios.insert(1, RadioState {
            freq_hz: 14_025_000,
            mode: "CW".to_string(),
            is_ptt: false,
            cw_speed: 36,
        });

        // < slows by 2, > speeds by 2 (tracking running speed)
        // {0} restores original speed
        let out = expand_macro("<5NN{0}", &st);
        assert_eq!(out, "{34}5NN{0}");

        let out = expand_macro(">TU {MYCALL}{0}", &st);
        assert_eq!(out, "{38}TU N0CALL{0}");

        // < and > as wrapper: <5NN> = slow down, send, restore
        let out = expand_macro("<5NN>", &st);
        assert_eq!(out, "{34}5NN{36}");

        // Stacking: << = -4
        let out = expand_macro("<<5NN>>", &st);
        assert_eq!(out, "{34}{32}5NN{34}{36}");
    }

    #[test]
    fn prosigns_preserved() {
        let contest = CqwwContest::default();
        let st = AppState {
            now_ms: 0,
            focused_radio: 1,
            active_operator: 1,
            radios: HashMap::new(),
            entries: HashMap::from([(1, EntryState::from_spec(&contest.form_spec()))]),
            bandmap: Vec::new(),
            last_logged: None,
            my_call: "N0CALL".to_string(),
            my_zone: 4,
            rst_sent: "599".to_string(),
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursor: None,
            default_cw_speed: 28,
            serial_counter: None,
        };

        let out = expand_macro("CQ TEST <AR>", &st);
        assert_eq!(out, "CQ TEST <AR>");
    }

    #[test]
    fn speed_down_saturates() {
        let contest = CqwwContest::default();
        let st = AppState {
            now_ms: 0,
            focused_radio: 1,
            active_operator: 1,
            radios: HashMap::new(),
            entries: HashMap::from([(1, EntryState::from_spec(&contest.form_spec()))]),
            bandmap: Vec::new(),
            last_logged: None,
            my_call: "N0CALL".to_string(),
            my_zone: 4,
            rst_sent: "599".to_string(),
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursor: None,
            default_cw_speed: 1,
            serial_counter: None,
        };

        let out = expand_macro("<5NN>", &st);
        assert_eq!(out, "{0}5NN{2}");
    }

    #[test]
    fn expands_placeholders() {
        let contest = CqwwContest::default();
        let mut st = AppState {
            now_ms: 0,
            focused_radio: 1,
            active_operator: 1,
            radios: HashMap::new(),
            entries: HashMap::from([(1, EntryState::from_spec(&contest.form_spec()))]),
            bandmap: Vec::new(),
            last_logged: None,
            my_call: "N0CALL".to_string(),
            my_zone: 4,
            rst_sent: "599".to_string(),
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursor: None,
            default_cw_speed: 28,
            serial_counter: None,
        };
        st.focused_entry_mut().fields[0].value = "K1ABC".to_string();

        let out = expand_macro("{MYCALL} {MYZONE} {RST_SENT} {CALL}", &st);
        assert_eq!(out, "N0CALL 4 599 K1ABC");
    }
}
