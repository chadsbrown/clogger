use crate::{
    contest::traits::{ContestEntry, EntryContext, EntryError},
    entry::{
        spec::{EntryFieldSpec, EntryFormSpec},
        state::{EntryState, Validation},
        validation::EntryValidation,
    },
    state::{Macros, QsoDraft},
};

const CALL_ID: u16 = 1;
const NAME_ID: u16 = 2;

#[derive(Debug, Clone, Default)]
pub struct MstContest;

impl ContestEntry for MstContest {
    fn contest_id(&self) -> &str {
        "mst"
    }

    fn contest_instance_id(&self) -> u64 {
        4
    }

    fn default_macros(&self) -> Macros {
        Macros {
            f1: "CQ MST {MYCALL}".to_string(),
            f2: "{MYNAME} {SERIAL}".to_string(),
            f3: "TU {MYCALL}".to_string(),
            ..Macros::default()
        }
    }

    fn form_spec(&self) -> EntryFormSpec {
        EntryFormSpec {
            fields: vec![
                EntryFieldSpec {
                    field_id: CALL_ID,
                    label: "CALL".to_string(),
                    required: true,
                    width: 12,
                },
                EntryFieldSpec {
                    field_id: NAME_ID,
                    label: "NAME".to_string(),
                    required: true,
                    width: 10,
                },
            ],
        }
    }

    fn validate_entry(&self, input: &EntryState, _ctx: &EntryContext) -> EntryValidation {
        let mut fields = Vec::with_capacity(input.fields.len());

        for field in &input.fields {
            let val = field.value.trim();
            let status = if field.required && val.is_empty() {
                Validation::Invalid(format!("{} required", field.label))
            } else {
                Validation::Valid
            };
            fields.push(status);
        }

        let first_invalid = fields.iter().position(|s| s.is_invalid());
        let overall = if first_invalid.is_some() {
            Validation::Invalid("entry invalid".to_string())
        } else {
            Validation::Valid
        };

        EntryValidation {
            fields,
            overall,
            first_invalid,
        }
    }

    fn build_qso_draft(
        &self,
        input: &EntryState,
        ctx: &EntryContext,
    ) -> Result<QsoDraft, EntryError> {
        let call = input
            .get_field_value_by_id(CALL_ID)
            .unwrap_or_default()
            .trim()
            .to_uppercase();
        if call.is_empty() {
            return Err(EntryError {
                message: "empty callsign".to_string(),
            });
        }

        let name = input
            .get_field_value_by_id(NAME_ID)
            .unwrap_or_default()
            .trim()
            .to_uppercase();

        let rig = ctx.rig.clone();
        Ok(QsoDraft {
            contest_id: "mst".to_string(),
            callsign: call,
            band: super::freq_to_band_label(rig.as_ref().map(|r| r.freq_hz).unwrap_or(0)),
            mode: rig
                .as_ref()
                .map(|r| r.mode.to_ascii_uppercase())
                .unwrap_or_else(|| "CW".to_string()),
            freq_hz: rig.as_ref().map(|r| r.freq_hz).unwrap_or(0),
            exchange_schema_id: 4,
            exchange_pairs: vec![("name".to_string(), name)],
        })
    }

    fn history_field_mapping(&self) -> Vec<(&str, u16)> {
        vec![("Name", NAME_ID)]
    }

    fn uses_serial(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::{EntryContext, contest::traits::ContestEntry, entry::state::EntryState};

    use super::MstContest;

    #[test]
    fn valid_mst_entry() {
        let contest = MstContest;
        let mut entry = EntryState::from_spec(&contest.form_spec());
        entry.fields[0].value = "K1ABC".to_string();
        entry.fields[1].value = "CHAD".to_string();
        let out = contest.validate_entry(
            &entry,
            &EntryContext {
                my_call: "N0CALL".to_string(),
                my_zone: 4,
                rst_sent: "599".to_string(),
                rig: None,
                serial: None,
            },
        );
        assert!(out.overall.is_valid());
    }

    #[test]
    fn missing_name_is_invalid() {
        let contest = MstContest;
        let mut entry = EntryState::from_spec(&contest.form_spec());
        entry.fields[0].value = "K1ABC".to_string();
        entry.fields[1].value = "".to_string();
        let out = contest.validate_entry(
            &entry,
            &EntryContext {
                my_call: "N0CALL".to_string(),
                my_zone: 4,
                rst_sent: "599".to_string(),
                rig: None,
                serial: None,
            },
        );
        assert!(out.overall.is_invalid());
    }
}
