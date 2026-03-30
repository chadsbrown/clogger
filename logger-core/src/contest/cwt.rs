use std::sync::OnceLock;

use serde::Deserialize;

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
const XCHG_ID: u16 = 3;

#[derive(Debug, Clone)]
pub struct CwtContest {
    spec: ParsedCwtSpec,
}

impl Default for CwtContest {
    fn default() -> Self {
        Self {
            spec: parsed_spec().clone(),
        }
    }
}

impl ContestEntry for CwtContest {
    fn contest_id(&self) -> &str {
        "cwt"
    }

    fn contest_instance_id(&self) -> u64 {
        3
    }

    fn default_macros(&self) -> Macros {
        Macros {
            f1: "CQ CWT {MYCALL}".to_string(),
            f2: "{CALL} {NAME} {XCHG}".to_string(),
            f3: "TU {CALL}".to_string(),
        }
    }

    fn form_spec(&self) -> EntryFormSpec {
        let mut fields = vec![EntryFieldSpec {
            field_id: CALL_ID,
            label: "CALL".to_string(),
            required: true,
        }];

        for (idx, field) in self.spec.received_fields.iter().enumerate() {
            fields.push(EntryFieldSpec {
                field_id: (idx as u16) + 2,
                label: field.id.to_ascii_uppercase(),
                required: field.required,
            });
        }

        EntryFormSpec { fields }
    }

    fn validate_entry(&self, input: &EntryState, _ctx: &EntryContext) -> EntryValidation {
        let mut fields = Vec::with_capacity(input.fields.len());

        for field in &input.fields {
            let val = field.value.trim();
            let status = if field.field_id == CALL_ID {
                if val.is_empty() {
                    Validation::Invalid("CALL required".to_string())
                } else {
                    Validation::Valid
                }
            } else {
                let idx = (field.field_id - 2) as usize;
                if let Some(spec_field) = self.spec.received_fields.get(idx) {
                    if spec_field.required && val.is_empty() {
                        Validation::Invalid(format!(
                            "{} required",
                            spec_field.id.to_ascii_uppercase()
                        ))
                    } else {
                        Validation::Valid
                    }
                } else {
                    Validation::Valid
                }
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
        let xchg = input
            .get_field_value_by_id(XCHG_ID)
            .unwrap_or_default()
            .trim()
            .to_uppercase();

        let rig = ctx.rig.clone();
        Ok(QsoDraft {
            contest_id: "cwt".to_string(),
            callsign: call,
            band: super::freq_to_band_label(rig.as_ref().map(|r| r.freq_hz).unwrap_or(0)),
            mode: rig
                .as_ref()
                .map(|r| r.mode.to_ascii_uppercase())
                .unwrap_or_else(|| "CW".to_string()),
            freq_hz: rig.as_ref().map(|r| r.freq_hz).unwrap_or(0),
            exchange_schema_id: 3,
            exchange_pairs: vec![
                ("name".to_string(), name),
                ("xchg".to_string(), xchg),
            ],
        })
    }
}

fn parsed_spec() -> &'static ParsedCwtSpec {
    static SPEC: OnceLock<ParsedCwtSpec> = OnceLock::new();
    SPEC.get_or_init(|| {
        let raw: CwtSpecRoot = serde_json::from_str(include_str!("../../specs/cwt.json"))
            .expect("embedded cwt.json must parse");
        let received_fields = raw
            .exchange
            .received_variants
            .first()
            .map(|v| {
                v.fields
                    .iter()
                    .map(|f| ReceivedField {
                        id: f.id.clone(),
                        required: f.required,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        ParsedCwtSpec { received_fields }
    })
}

#[derive(Debug, Clone)]
struct ParsedCwtSpec {
    received_fields: Vec<ReceivedField>,
}

#[derive(Debug, Clone)]
struct ReceivedField {
    id: String,
    required: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct CwtSpecRoot {
    exchange: Exchange,
}

#[derive(Debug, Clone, Deserialize)]
struct Exchange {
    received_variants: Vec<ReceivedVariant>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReceivedVariant {
    fields: Vec<ReceivedFieldRaw>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReceivedFieldRaw {
    id: String,
    required: bool,
}

#[cfg(test)]
mod tests {
    use crate::{
        contest::traits::ContestEntry,
        entry::state::EntryState,
        EntryContext,
    };

    use super::CwtContest;

    #[test]
    fn valid_cwt_entry() {
        let contest = CwtContest::default();
        let mut entry = EntryState::from_spec(&contest.form_spec());
        entry.fields[0].value = "K1ABC".to_string();
        entry.fields[1].value = "CHAD".to_string();
        entry.fields[2].value = "2187".to_string();
        let out = contest.validate_entry(
            &entry,
            &EntryContext {
                my_call: "N0CALL".to_string(),
                my_zone: 4,
                rst_sent: "599".to_string(),
                rig: None,
            },
        );
        assert!(out.overall.is_valid());
    }

    #[test]
    fn missing_name_is_invalid() {
        let contest = CwtContest::default();
        let mut entry = EntryState::from_spec(&contest.form_spec());
        entry.fields[0].value = "K1ABC".to_string();
        entry.fields[1].value = "".to_string();
        entry.fields[2].value = "2187".to_string();
        let out = contest.validate_entry(
            &entry,
            &EntryContext {
                my_call: "N0CALL".to_string(),
                my_zone: 4,
                rst_sent: "599".to_string(),
                rig: None,
            },
        );
        assert!(out.overall.is_invalid());
    }
}
