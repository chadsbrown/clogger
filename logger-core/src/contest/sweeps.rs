use crate::{
    contest::traits::{ContestEntry, EntryContext, EntryError},
    entry::{
        spec::{EntryFieldSpec, EntryFormSpec},
        state::{EntryState, Validation},
        validation::EntryValidation,
    },
    state::QsoDraft,
};

pub struct SweepsContest;

const CALL_ID: u16 = 1;
const NR_ID: u16 = 2;
const PREC_ID: u16 = 3;
const CHECK_ID: u16 = 4;
const SECTION_ID: u16 = 5;

impl ContestEntry for SweepsContest {
    fn form_spec(&self) -> EntryFormSpec {
        EntryFormSpec {
            fields: vec![
                EntryFieldSpec {
                    field_id: CALL_ID,
                    label: "CALL".to_string(),
                    required: true,
                },
                EntryFieldSpec {
                    field_id: NR_ID,
                    label: "NR".to_string(),
                    required: true,
                },
                EntryFieldSpec {
                    field_id: PREC_ID,
                    label: "PREC".to_string(),
                    required: true,
                },
                EntryFieldSpec {
                    field_id: CHECK_ID,
                    label: "CHECK".to_string(),
                    required: true,
                },
                EntryFieldSpec {
                    field_id: SECTION_ID,
                    label: "SECTION".to_string(),
                    required: true,
                },
            ],
        }
    }

    fn validate_entry(&self, input: &EntryState, _ctx: &EntryContext) -> EntryValidation {
        let mut fields = Vec::with_capacity(input.fields.len());

        for field in &input.fields {
            let value = field.value.trim();
            let status = match field.field_id {
                CALL_ID => {
                    if value.is_empty() {
                        Validation::Invalid("CALL required".to_string())
                    } else {
                        Validation::Valid
                    }
                }
                NR_ID => {
                    let ok = value
                        .parse::<u32>()
                        .ok()
                        .map(|n| (1..=99_999).contains(&n))
                        .unwrap_or(false);
                    if ok {
                        Validation::Valid
                    } else {
                        Validation::Invalid("NR must be 1..99999".to_string())
                    }
                }
                PREC_ID => {
                    let upper = value.to_ascii_uppercase();
                    if matches!(upper.as_str(), "A" | "B" | "Q" | "U" | "M" | "S") {
                        Validation::Valid
                    } else {
                        Validation::Invalid("PREC must be A/B/Q/U/M/S".to_string())
                    }
                }
                CHECK_ID => {
                    if value.len() == 2 && value.chars().all(|c| c.is_ascii_digit()) {
                        Validation::Valid
                    } else {
                        Validation::Invalid("CHECK must be 2 digits".to_string())
                    }
                }
                SECTION_ID => {
                    let len = value.len();
                    let ok = (2..=6).contains(&len) && value.chars().all(|c| c.is_ascii_alphanumeric());
                    if ok {
                        Validation::Valid
                    } else {
                        Validation::Invalid("SECTION must be 2-6 alnum".to_string())
                    }
                }
                _ => Validation::Valid,
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

    fn build_qso_draft(&self, input: &EntryState, ctx: &EntryContext) -> Result<QsoDraft, EntryError> {
        let call = input
            .get_field_value_by_id(CALL_ID)
            .unwrap_or_default()
            .trim()
            .to_uppercase();
        let nr = input
            .get_field_value_by_id(NR_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let prec = input
            .get_field_value_by_id(PREC_ID)
            .unwrap_or_default()
            .trim()
            .to_uppercase();
        let check = input
            .get_field_value_by_id(CHECK_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let section = input
            .get_field_value_by_id(SECTION_ID)
            .unwrap_or_default()
            .trim()
            .to_uppercase();

        if call.is_empty() {
            return Err(EntryError {
                message: "empty callsign".to_string(),
            });
        }

        let rig = ctx.rig.clone();
        Ok(QsoDraft {
            contest_id: "sweeps".to_string(),
            callsign: call,
            band: freq_to_band_label(rig.as_ref().map(|r| r.freq_hz).unwrap_or(0)),
            mode: rig
                .as_ref()
                .map(|r| r.mode.to_ascii_uppercase())
                .unwrap_or_else(|| "CW".to_string()),
            freq_hz: rig.as_ref().map(|r| r.freq_hz).unwrap_or(0),
            exchange_schema_id: 2,
            exchange_pairs: vec![
                ("nr".to_string(), nr),
                ("prec".to_string(), prec),
                ("check".to_string(), check),
                ("section".to_string(), section),
            ],
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

#[cfg(test)]
mod tests {
    use crate::{
        contest::traits::ContestEntry,
        entry::state::EntryState,
        SweepsContest,
    };

    #[test]
    fn lowercase_prec_is_valid() {
        let contest = SweepsContest;
        let mut entry = EntryState::from_spec(&contest.form_spec());
        entry.fields[0].value = "K1ABC".to_string();
        entry.fields[1].value = "123".to_string();
        entry.fields[2].value = "a".to_string();
        entry.fields[3].value = "77".to_string();
        entry.fields[4].value = "EMA".to_string();
        let out = contest.validate_entry(
            &entry,
            &crate::EntryContext {
                my_call: "N0CALL".to_string(),
                my_zone: 4,
                rst_sent: "599".to_string(),
                rig: None,
            },
        );
        assert!(out.overall.is_valid());
    }
}
