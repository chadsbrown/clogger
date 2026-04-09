use contest_engine::spec::{ContestSpec, DomainRef, ExchangeField, FieldType, embedded};

use crate::{
    entry::{
        spec::{EntryFieldSpec, EntryFormSpec},
        state::{EntryState, Validation},
        validation::EntryValidation,
    },
    state::{Macros, QsoDraft},
};

use super::registry::SpecContestMeta;
use super::traits::{CategoryMode, ContestEntry, EntryContext, EntryError};

const CALL_ID: u16 = 1;

pub struct SpecDrivenContest {
    meta: &'static SpecContestMeta,
    spec: ContestSpec,
}

impl SpecDrivenContest {
    /// Create a new spec-driven contest from a registry entry. Returns None
    /// if the corresponding contest-engine spec cannot be found.
    pub fn new(meta: &'static SpecContestMeta) -> Option<Self> {
        let spec = embedded::spec_by_id(meta.contest_id)?;
        Some(Self { meta, spec })
    }

    fn received_fields(&self) -> &[ExchangeField] {
        self.spec
            .exchange
            .received_variants
            .first()
            .map(|v| v.fields.as_slice())
            .unwrap_or(&[])
    }

    fn field_width(&self, field_id: u16) -> u16 {
        // Check registry override first
        if let Some((_, w)) = self.meta.field_widths.iter().find(|(id, _)| *id == field_id) {
            return *w;
        }
        // Fall back to width derived from field type (exchange fields only)
        if field_id >= 2 {
            let idx = (field_id - 2) as usize;
            if let Some(f) = self.received_fields().get(idx) {
                return default_width_for_field_type(Some(&f.field_type), f.domain.as_ref());
            }
        }
        12 // default for CALL or unknown
    }
}

impl ContestEntry for SpecDrivenContest {
    fn contest_id(&self) -> &str {
        &self.spec.id
    }

    fn contest_name(&self) -> &str {
        &self.spec.name
    }

    fn contest_instance_id(&self) -> u64 {
        self.meta.contest_instance_id
    }

    fn default_macros(&self) -> Macros {
        (self.meta.default_macros_fn)()
    }

    fn form_spec(&self) -> EntryFormSpec {
        let mut fields = vec![EntryFieldSpec {
            field_id: CALL_ID,
            label: "CALL".to_string(),
            required: true,
            width: self.field_width(CALL_ID),
        }];

        for (idx, field) in self.received_fields().iter().enumerate() {
            let field_id = (idx as u16) + 2;
            fields.push(EntryFieldSpec {
                field_id,
                label: field.id.to_ascii_uppercase(),
                required: field.required,
                width: self.field_width(field_id),
            });
        }

        EntryFormSpec { fields }
    }

    fn validate_entry(&self, input: &EntryState, _ctx: &EntryContext) -> EntryValidation {
        let received = self.received_fields();
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
                if let Some(spec_field) = received.get(idx) {
                    validate_field(spec_field, val)
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

        let received = self.received_fields();
        let exchange_pairs: Vec<(String, String)> = received
            .iter()
            .enumerate()
            .map(|(idx, spec_field)| {
                let field_id = (idx as u16) + 2;
                let value = input
                    .get_field_value_by_id(field_id)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let value = if spec_field.normalize_upper_trim {
                    value.to_ascii_uppercase()
                } else {
                    value
                };
                (spec_field.id.clone(), value)
            })
            .collect();

        let rig = ctx.rig.clone();
        Ok(QsoDraft {
            contest_id: self.spec.id.clone(),
            callsign: call,
            band: super::freq_to_band_label(rig.as_ref().map(|r| r.freq_hz).unwrap_or(0)),
            mode: rig
                .as_ref()
                .map(|r| r.mode.to_ascii_uppercase())
                .unwrap_or_else(|| "CW".to_string()),
            freq_hz: rig.as_ref().map(|r| r.freq_hz).unwrap_or(0),
            exchange_schema_id: self.meta.exchange_schema_id,
            exchange_pairs,
        })
    }

    fn history_field_mapping(&self) -> Vec<(&str, u16)> {
        self.meta.history_mapping.iter().copied().collect()
    }

    fn uses_serial(&self) -> bool {
        self.meta.uses_serial
    }

    fn cabrillo_id(&self, mode: CategoryMode) -> Option<&'static str> {
        (self.meta.cabrillo_id_fn)(mode)
    }
}

fn validate_field(spec_field: &ExchangeField, value: &str) -> Validation {
    if spec_field.required && value.is_empty() {
        return Validation::Invalid(format!(
            "{} required",
            spec_field.id.to_ascii_uppercase()
        ));
    }
    if value.is_empty() {
        return Validation::Valid;
    }

    match spec_field.field_type {
        FieldType::Rst => {
            if value.len() >= 2
                && value.len() <= 3
                && value.chars().all(|c| c.is_ascii_digit())
            {
                Validation::Valid
            } else {
                Validation::Invalid("RST must be 2-3 digits".to_string())
            }
        }
        FieldType::Int => {
            let parsed = value.parse::<i64>().ok();
            match (parsed, &spec_field.domain) {
                (Some(n), Some(DomainRef::Range { min, max }))
                    if n >= *min && n <= *max =>
                {
                    Validation::Valid
                }
                (Some(_), Some(DomainRef::Range { min, max })) => {
                    Validation::Invalid(format!(
                        "{} must be {}-{}",
                        spec_field.id.to_ascii_uppercase(),
                        min,
                        max
                    ))
                }
                (Some(_), _) => Validation::Valid,
                (None, _) => Validation::Invalid(format!(
                    "{} must be numeric",
                    spec_field.id.to_ascii_uppercase()
                )),
            }
        }
        _ => Validation::Valid,
    }
}

fn default_width_for_field_type(
    field_type: Option<&FieldType>,
    domain: Option<&DomainRef>,
) -> u16 {
    match field_type {
        Some(FieldType::Rst) => 3,
        Some(FieldType::Int) => match domain {
            Some(DomainRef::Range { min, max }) => {
                let min_len = min.to_string().len();
                let max_len = max.to_string().len();
                min_len.max(max_len) as u16
            }
            _ => 5,
        },
        _ => 8,
    }
}
