use std::{collections::HashMap, sync::OnceLock};

use serde::Deserialize;

use crate::{
    contest::traits::{ContestEntry, EntryContext, EntryError},
    entry::{
        spec::{EntryFieldSpec, EntryFormSpec},
        state::{EntryState, Validation},
        validation::EntryValidation,
    },
    state::QsoDraft,
};

const CALL_ID: u16 = 1;

#[derive(Debug, Clone)]
pub struct CqwwContest {
    spec: ParsedCqwwSpec,
}

impl Default for CqwwContest {
    fn default() -> Self {
        Self {
            spec: parsed_spec().clone(),
        }
    }
}

impl ContestEntry for CqwwContest {
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
                    validate_value(spec_field, val)
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

    fn build_qso_draft(&self, input: &EntryState, _ctx: &EntryContext) -> Result<QsoDraft, EntryError> {
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

        let mut values: HashMap<&str, String> = HashMap::new();
        for (idx, spec_field) in self.spec.received_fields.iter().enumerate() {
            let field_id = (idx as u16) + 2;
            let value = input
                .get_field_value_by_id(field_id)
                .unwrap_or_default()
                .trim()
                .to_string();
            values.insert(spec_field.id.as_str(), value);
        }

        let _rst = values
            .get("rst")
            .cloned()
            .ok_or_else(|| EntryError {
                message: "missing rst".to_string(),
            })?;
        let _zone = values
            .get("zone")
            .ok_or_else(|| EntryError {
                message: "missing zone".to_string(),
            })?
            .parse::<u8>()
            .map_err(|_| EntryError {
                message: "invalid zone".to_string(),
            })?;
        let rig = _ctx.rig.clone();

        let exchange_pairs = self
            .spec
            .received_fields
            .iter()
            .map(|f| {
                (
                    f.id.clone(),
                    values.get(f.id.as_str()).cloned().unwrap_or_default(),
                )
            })
            .collect();

        Ok(QsoDraft {
            contest_id: "cqww".to_string(),
            callsign: call,
            band: freq_to_band_label(rig.as_ref().map(|r| r.freq_hz).unwrap_or(0)),
            mode: rig
                .as_ref()
                .map(|r| r.mode.to_ascii_uppercase())
                .unwrap_or_else(|| "CW".to_string()),
            freq_hz: rig.as_ref().map(|r| r.freq_hz).unwrap_or(0),
            exchange_schema_id: 1,
            exchange_pairs,
        })
    }
}

fn freq_to_band_label(freq_hz: u64) -> String {
    match freq_hz {
        1_800_000..=2_000_000 => "160m",
        3_500_000..=4_000_000 => "80m",
        7_000_000..=7_300_000 => "40m",
        14_000_000..=14_350_000 => "20m",
        21_000_000..=21_450_000 => "15m",
        28_000_000..=29_700_000 => "10m",
        _ => "other",
    }
    .to_string()
}

fn validate_value(spec_field: &ReceivedField, value: &str) -> Validation {
    if spec_field.required && value.is_empty() {
        return Validation::Invalid(format!("{} required", spec_field.id.to_ascii_uppercase()));
    }

    match spec_field.field_type.as_str() {
        "Rst" => {
            if value.len() >= 2 && value.len() <= 3 && value.chars().all(|c| c.is_ascii_digit()) {
                Validation::Valid
            } else {
                Validation::Invalid("RST must be 2-3 digits".to_string())
            }
        }
        "Int" => {
            let parsed = value.parse::<i64>().ok();
            match (parsed, &spec_field.domain) {
                (Some(n), Some(Range { min, max })) if n >= *min && n <= *max => Validation::Valid,
                (Some(_), Some(Range { min, max })) => {
                    Validation::Invalid(format!("{} must be {}-{}", spec_field.id.to_ascii_uppercase(), min, max))
                }
                (Some(_), None) => Validation::Valid,
                _ => Validation::Invalid(format!("{} must be numeric", spec_field.id.to_ascii_uppercase())),
            }
        }
        _ => Validation::Valid,
    }
}

fn parsed_spec() -> &'static ParsedCqwwSpec {
    static SPEC: OnceLock<ParsedCqwwSpec> = OnceLock::new();
    SPEC.get_or_init(|| {
        let raw: CqwwSpecRoot = serde_json::from_str(include_str!("../../specs/cqww.json"))
            .expect("embedded cqww.json must parse");
        let received_fields = raw
            .exchange
            .received_variants
            .first()
            .map(|v| {
                v.fields
                    .iter()
                    .map(|f| ReceivedField {
                        id: f.id.clone(),
                        field_type: f.field_type.clone(),
                        required: f.required,
                        domain: f.domain.as_ref().and_then(|d| d.range.clone()),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        ParsedCqwwSpec { received_fields }
    })
}

#[derive(Debug, Clone)]
struct ParsedCqwwSpec {
    received_fields: Vec<ReceivedField>,
}

#[derive(Debug, Clone)]
struct ReceivedField {
    id: String,
    field_type: String,
    required: bool,
    domain: Option<Range>,
}

#[derive(Debug, Clone, Deserialize)]
struct CqwwSpecRoot {
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
    field_type: String,
    required: bool,
    domain: Option<Domain>,
}

#[derive(Debug, Clone, Deserialize)]
struct Domain {
    #[serde(rename = "Range")]
    range: Option<Range>,
}

#[derive(Debug, Clone, Deserialize)]
struct Range {
    min: i64,
    max: i64,
}
