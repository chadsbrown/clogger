use std::collections::HashMap;

use contest_engine::spec::{
    ContestSpec, DomainRef, ExchangeField, FieldType, Mode, Operand, Predicate, Scope, SentValue,
    Value, embedded,
};

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
            band: super::freq_to_band_label(rig.as_ref().map(|r| r.freq_hz).unwrap_or(0)).to_string(),
            mode: rig
                .as_ref()
                .map(|r| r.mode.to_ascii_uppercase())
                .unwrap_or_else(|| "CW".to_string()),
            freq_hz: rig.as_ref().map(|r| r.freq_hz).unwrap_or(0),
            exchange_schema_id: self.meta.exchange_schema_id,
            exchange_pairs,
        })
    }

    /// Override: split into N drafts when any received field has a
    /// `multi_value_sep` declared in the spec and its typed value contains
    /// the separator. Handles county-line rovers in state QSO parties — e.g.
    /// MOQP receiving "DAD/GRN/POL" yields three drafts, each with one
    /// county, so contest-engine scores them as N independent QSOs (matching
    /// N1MM / sponsor convention). At most one field is treated as
    /// splittable per entry; if multiple fields declare a separator, only
    /// the first one found is split to avoid combinatorial blow-up.
    fn build_qso_drafts(
        &self,
        input: &EntryState,
        ctx: &EntryContext,
    ) -> Result<Vec<QsoDraft>, EntryError> {
        let base = self.build_qso_draft(input, ctx)?;

        for (idx, spec_field) in self.received_fields().iter().enumerate() {
            let Some(sep) = spec_field.multi_value_sep.as_deref() else {
                continue;
            };
            if sep.is_empty() {
                continue;
            }
            let Some((_, raw_value)) = base.exchange_pairs.get(idx) else {
                continue;
            };
            if !raw_value.contains(sep) {
                continue;
            }
            let pieces: Vec<String> = raw_value
                .split(sep)
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            if pieces.len() <= 1 {
                continue;
            }
            let drafts = pieces
                .into_iter()
                .map(|piece| {
                    let mut draft = base.clone();
                    draft.exchange_pairs[idx].1 = piece;
                    draft
                })
                .collect();
            return Ok(drafts);
        }

        Ok(vec![base])
    }

    fn history_field_mapping(&self) -> Vec<(&str, u16)> {
        self.meta.history_mapping.iter().copied().collect()
    }

    fn uses_serial(&self) -> bool {
        self.meta.uses_serial
    }

    fn cabrillo_id(&self, mode: CategoryMode) -> Option<&str> {
        // Look up directly from the contest-engine spec. For contests
        // with mode-specific sponsor names (CQWW, ARRL-DX, SS, NAQP,
        // ARRL-SS), find the variant whose `allowed_modes` includes
        // the target mode and return its `cabrillo_contest`. For
        // single-mode contests with no variants (CWT, MST, state QPs,
        // NS Sprint), fall back to the top-level `cabrillo_contest`.
        if let Some(target) = category_mode_to_engine(mode) {
            for variant in self.spec.variants.values() {
                let Some(allowed) = variant.allowed_modes.as_ref() else {
                    continue;
                };
                if allowed.contains(&target) {
                    if let Some(name) = variant.cabrillo_contest.as_deref() {
                        return Some(name);
                    }
                }
            }
        }
        // No matching variant. If the contest has no variants at all,
        // the top-level name is the answer regardless of mode.
        if self.spec.variants.is_empty() {
            return Some(self.spec.cabrillo_contest.as_str());
        }
        // Contest has variants but none matched — e.g. Mixed against
        // a CW/SSB split. Preserve the old behavior of returning None
        // so multi-mode contests don't silently export with the wrong
        // (top-level) name.
        None
    }

    fn auto_toggle_mode(&self) -> bool {
        self.meta.auto_toggle_mode
    }

    fn rst_field_id(&self) -> Option<u16> {
        match self.received_fields().first()?.field_type {
            FieldType::Rst => Some(2),
            _ => None,
        }
    }

    fn default_rst(&self, mode: &str) -> Option<String> {
        // Only RST contests get an auto-populate value.
        self.rst_field_id()?;

        // Mode-specific override declared in `variants` (e.g. CQWW: cw → 599, ssb → 59).
        if let Some(target) = normalized_mode_to_engine(mode) {
            for variant in self.spec.variants.values() {
                let Some(allowed) = variant.allowed_modes.as_ref() else { continue };
                if !allowed.contains(&target) {
                    continue;
                }
                if let Some(rst) = variant.exchange.as_ref().and_then(|e| e.sent_rst_value.clone()) {
                    return Some(rst);
                }
            }
        }

        // Const value pulled from `sent_variants` (state QPs hardcode "599").
        for sent_variant in &self.spec.exchange.sent_variants {
            for field in &sent_variant.fields {
                if field.id == "rst" {
                    if let SentValue::Const(value) = &field.value {
                        return Some(value.clone());
                    }
                }
            }
        }

        // Universal fallback when the spec doesn't say.
        match mode {
            "SSB" => Some("59".to_string()),
            _ => Some("599".to_string()),
        }
    }

    fn sent_exchange_pairs(&self, ctx: &EntryContext) -> Vec<(String, String)> {
        // Pick the first matching sent variant. State QPs use a `when`
        // predicate on CONFIG-scope fields (e.g. `my_is_fl = true`) to
        // pick in-state vs out-of-state variants; everything else uses
        // `when: null`.
        let variant = self
            .spec
            .exchange
            .sent_variants
            .iter()
            .find(|v| match &v.when {
                None => true,
                Some(pred) => eval_config_predicate(pred, &ctx.station_config),
            });
        let Some(variant) = variant else {
            return Vec::new();
        };

        let mut out = Vec::with_capacity(variant.fields.len());
        for field in &variant.fields {
            let raw = match &field.value {
                SentValue::Const(v) => v.clone(),
                SentValue::Config(key) => match ctx.station_config.get(key) {
                    Some(v) => value_to_text(v),
                    None => String::new(),
                },
            };
            let value = if field.normalize_upper_trim {
                raw.trim().to_ascii_uppercase()
            } else {
                raw
            };
            out.push((format!("sent_{}", field.id), value));
        }
        out
    }
}

/// Stringify a typed config value the same way contest-engine's
/// `Value::as_text` does internally. Exposed here because that method is
/// private.
fn value_to_text(v: &Value) -> String {
    match v {
        Value::Text(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => b.to_string(),
    }
}

/// Minimal predicate evaluator used only for sent-variant `when` selection
/// at log time. Only `CONFIG`-scope field references are resolved —
/// `SOURCE`/`DEST`/`RCVD`/`SENT`/`SESSION` would require the scorer's
/// resolver and aren't used in any real sent_variants in the contest-engine
/// spec set. If they ever are, this function falls back to `false`
/// conservatively (no variant match → empty sent exchange → visible in
/// tests).
fn eval_config_predicate(pred: &Predicate, config: &HashMap<String, Value>) -> bool {
    match pred {
        Predicate::Eq(a, b) => operand_eq(a, b, config),
        Predicate::Ne(a, b) => !operand_eq(a, b, config),
        Predicate::And(preds) => preds.iter().all(|p| eval_config_predicate(p, config)),
        Predicate::Or(preds) => preds.iter().any(|p| eval_config_predicate(p, config)),
        Predicate::Not(p) => !eval_config_predicate(p, config),
        Predicate::Between(field, lo, hi) => {
            let Some(v) = resolve_operand(&Operand::Field(field.clone()), config) else {
                return false;
            };
            match v {
                Value::Int(i) => i >= *lo && i <= *hi,
                _ => false,
            }
        }
        Predicate::In(field, items) => {
            let Some(v) = resolve_operand(&Operand::Field(field.clone()), config) else {
                return false;
            };
            items.iter().any(|s| *s == value_to_text(&v))
        }
        // Not resolvable at log time without a cty lookup + QSO context.
        Predicate::DestCallIn(_) => false,
    }
}

fn operand_eq(a: &Operand, b: &Operand, config: &HashMap<String, Value>) -> bool {
    match (resolve_operand(a, config), resolve_operand(b, config)) {
        (Some(av), Some(bv)) => av == bv,
        _ => false,
    }
}

fn resolve_operand(op: &Operand, config: &HashMap<String, Value>) -> Option<Value> {
    match op {
        Operand::Value(v) => Some(v.clone()),
        Operand::Field(fr) => match fr.scope {
            Scope::Config => config.get(&fr.key).cloned(),
            _ => None,
        },
    }
}

fn normalized_mode_to_engine(mode: &str) -> Option<Mode> {
    match mode {
        "CW" => Some(Mode::CW),
        "SSB" => Some(Mode::SSB),
        "DIGITAL" => Some(Mode::DIGITAL),
        _ => None,
    }
}

/// Map a clogger `CategoryMode` to the contest-engine `Mode` used in
/// `variants.<mode>.allowed_modes`. `CategoryMode::Mixed` has no
/// single-mode counterpart — callers handle that as "no variant
/// matches" and either fall back to the top-level name or return None.
fn category_mode_to_engine(m: CategoryMode) -> Option<Mode> {
    match m {
        CategoryMode::CW => Some(Mode::CW),
        CategoryMode::SSB => Some(Mode::SSB),
        CategoryMode::Mixed => None,
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
