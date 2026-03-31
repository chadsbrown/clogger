use crate::state::AppState;

pub fn expand_macro(template: &str, st: &AppState) -> String {
    let call = st.current_call();
    let my_zone = st.my_zone.to_string();
    let base = [
        ("{MYCALL}", st.my_call.as_str()),
        ("{MYZONE}", my_zone.as_str()),
        ("{RST_SENT}", st.rst_sent.as_str()),
        ("{CALL}", call.as_str()),
    ];

    let mut out = base
        .into_iter()
        .fold(template.to_string(), |acc, (k, v)| acc.replace(k, v));

    // Expand {MY<KEY>} from my_exchange map (e.g. {MYNAME}, {MYXCHG})
    for (key, val) in &st.my_exchange {
        let token = format!("{{MY{}}}", key.to_ascii_uppercase());
        out = out.replace(&token, val);
    }

    st.entry.fields.iter().fold(out, |acc, f| {
        let token = format!("{{{}}}", f.label.to_ascii_uppercase());
        acc.replace(&token, f.value.trim())
    })
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

    #[test]
    fn expands_placeholders() {
        let contest = CqwwContest::default();
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
            my_exchange: HashMap::new(),
            esm_policy: EsmPolicy::default(),
            bandmap_cursor: None,
        };
        st.entry.fields[0].value = "K1ABC".to_string();

        let out = expand_macro("{MYCALL} {MYZONE} {RST_SENT} {CALL}", &st);
        assert_eq!(out, "N0CALL 4 599 K1ABC");
    }
}
