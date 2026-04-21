use std::path::Path;

use adif_parser::{AdifFile, AdifHeader, Field, Record};
use anyhow::Result;
use qsolog::qso::QsoRecord;
use qsolog::types::{Band, Mode};

use crate::log_adapter::decode_exchange_pairs;

/// Export non-voided QSO records to an ADIF file at the given path.
pub fn export_adif(
    records: &[QsoRecord],
    my_call: &str,
    contest_id: &str,
    path: &Path,
) -> Result<usize> {
    let file = build_adif_file(records, my_call, contest_id);
    let count = file.records.len();
    let adi = file.to_adi_string();
    std::fs::write(path, adi)?;
    Ok(count)
}

fn build_adif_file(records: &[QsoRecord], my_call: &str, contest_id: &str) -> AdifFile {
    let mut file = AdifFile::new();

    file.header = AdifHeader {
        preamble: format!("ADIF export by clogger for {my_call}"),
        fields: vec![
            Field::new("ADIF_VER", "3.1.4"),
            Field::new("PROGRAMID", "clogger"),
            Field::new("PROGRAMVERSION", env!("CARGO_PKG_VERSION")),
        ],
        adif_version: Some("3.1.4".to_string()),
        program_id: Some("clogger".to_string()),
        program_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        created_timestamp: None,
    };

    for rec in records.iter().filter(|r| !r.flags.is_void) {
        file.records.push(qso_to_adif_record(rec, my_call, contest_id));
    }

    file
}

fn qso_to_adif_record(rec: &QsoRecord, my_call: &str, contest_id: &str) -> Record {
    let mut adif = Record::new();

    adif.add_field(Field::new("CALL", &rec.callsign_raw));
    adif.add_field(Field::new("STATION_CALLSIGN", my_call));

    let dt = chrono::DateTime::from_timestamp_millis(rec.ts_ms as i64)
        .unwrap_or_default();
    adif.add_field(Field::new("QSO_DATE", dt.format("%Y%m%d").to_string()));
    adif.add_field(Field::new("TIME_ON", dt.format("%H%M%S").to_string()));

    if let Some(band) = band_to_adif(rec.band) {
        adif.add_field(Field::new("BAND", band));
    }

    if rec.freq_hz > 0 {
        let mhz = rec.freq_hz as f64 / 1_000_000.0;
        adif.add_field(Field::new("FREQ", format!("{mhz:.6}")));
    }

    if let Some(mode) = mode_to_adif(rec.mode) {
        adif.add_field(Field::new("MODE", mode));
    }

    // RST_SENT — derive from mode (correct for 99.9% of contest QSOs)
    let rst_sent = match rec.mode {
        Mode::SSB => "59",
        _ => "599",
    };
    adif.add_field(Field::new("RST_SENT", rst_sent));

    // Exchange fields (received + sent serial)
    if let Ok(pairs) = decode_exchange_pairs(&rec.exchange) {
        for (key, value) in &pairs {
            match key.to_ascii_lowercase().as_str() {
                "rst" => {
                    adif.add_field(Field::new("RST_RCVD", value));
                }
                "zone" => {
                    adif.add_field(Field::new("CQZ", value));
                }
                "section" => {
                    adif.add_field(Field::new("ARRL_SECT", value));
                }
                "name" => {
                    adif.add_field(Field::new("NAME", value));
                }
                "serial" => {
                    adif.add_field(Field::new("STX", value));
                }
                "nr" => {
                    adif.add_field(Field::new("SRX", value));
                }
                "prec" => {
                    adif.add_field(Field::new("PRECEDENCE", value));
                }
                "check" => {
                    adif.add_field(Field::new("CHECK", value));
                }
                "loc" => {
                    adif.add_field(Field::new("STATE", value));
                }
                "country" => {
                    adif.add_field(Field::new("COUNTRY", value));
                }
                _ => {
                    adif.add_field(Field::new(
                        "COMMENT",
                        format!("{key}={value}"),
                    ));
                }
            }
        }
    }

    // Contest ID — map to ADIF enumeration per QSO mode
    let adif_contest_id = to_adif_contest_id(contest_id, rec.mode);
    adif.add_field(Field::new("CONTEST_ID", adif_contest_id));

    adif
}

fn to_adif_contest_id(internal_id: &str, mode: Mode) -> &'static str {
    let suffix = match mode {
        Mode::SSB => "-SSB",
        Mode::Digital => "-RTTY",
        _ => "-CW",
    };
    match internal_id {
        "cqww" => match suffix {
            "-SSB" => "CQ-WW-SSB",
            "-RTTY" => "CQ-WW-RTTY",
            _ => "CQ-WW-CW",
        },
        "arrl_dx" => match suffix {
            "-SSB" => "ARRL-DX-SSB",
            _ => "ARRL-DX-CW",
        },
        "sweeps" => match suffix {
            "-SSB" => "ARRL-SS-SSB",
            _ => "ARRL-SS-CW",
        },
        "naqp" => match suffix {
            "-SSB" => "NAQP-SSB",
            "-RTTY" => "NAQP-RTTY",
            _ => "NAQP-CW",
        },
        "cwt" => "CWOPS-CWT",
        "mst" => "ICWC-MST",
        "ns_sprint" => "NA-SPRINT-CW",
        // State QSO parties — ADIF uses the same Cabrillo ID
        "flqp" => "FL-QSO-PARTY",
        "gaqp" => "GA-QSO-PARTY",
        "inqp" => "IN-QSO-PARTY",
        "miqp" => "MI-QSO-PARTY",
        "moqp" => "MO-QSO-PARTY",
        "ndqp" => "ND-QSO-PARTY",
        "neqp" => "NE-QSO-PARTY",
        "nhqp" => "NH-QSO-PARTY",
        "nmqp" => "NM-QSO-PARTY",
        "onqp" => "ON-QSO-PARTY",
        "qcqp" => "QC-QSO-PARTY",
        "deqp" => "DE-QSO-PARTY",
        "neqsop" => "NEQSO",
        // Fallback: use the internal id uppercased
        other => {
            // Can't return a &'static str for dynamic strings, so leak.
            // This only fires for unrecognized contest IDs.
            Box::leak(other.to_ascii_uppercase().into_boxed_str())
        }
    }
}

fn band_to_adif(band: Band) -> Option<&'static str> {
    match band {
        Band::B160m => Some("160m"),
        Band::B80m => Some("80m"),
        Band::B40m => Some("40m"),
        Band::B20m => Some("20m"),
        Band::B15m => Some("15m"),
        Band::B10m => Some("10m"),
        Band::Other => None,
    }
}

fn mode_to_adif(mode: Mode) -> Option<&'static str> {
    match mode {
        Mode::CW => Some("CW"),
        Mode::SSB => Some("SSB"),
        // TODO: Digital sub-mode (FT8, FT4, PSK) is lost when QsoDraft.mode
        // gets bucketed into Mode::Digital. RTTY is correct for most contest
        // digital QSOs. Proper fix: store the raw rig mode string in QsoRecord.
        Mode::Digital => Some("RTTY"),
        Mode::Other => None,
    }
}
