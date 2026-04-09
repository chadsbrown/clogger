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

    // Date and time from timestamp
    let dt = chrono::DateTime::from_timestamp_millis(rec.ts_ms as i64)
        .unwrap_or_default();
    adif.add_field(Field::new("QSO_DATE", &dt.format("%Y%m%d").to_string()));
    adif.add_field(Field::new("TIME_ON", &dt.format("%H%M%S").to_string()));

    // Band
    adif.add_field(Field::new("BAND", band_to_adif(rec.band)));

    // Frequency in MHz
    if rec.freq_hz > 0 {
        let mhz = rec.freq_hz as f64 / 1_000_000.0;
        adif.add_field(Field::new("FREQ", &format!("{mhz:.6}")));
    }

    // Mode
    adif.add_field(Field::new("MODE", mode_to_adif(rec.mode)));

    // Exchange fields (RST + contest-specific)
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
                    adif.add_field(Field::new("SRX", value));
                }
                _ => {
                    // Store unknown exchange fields as contest-specific comment
                    adif.add_field(Field::new(
                        "COMMENT",
                        &format!("{key}={value}"),
                    ));
                }
            }
        }
    }

    // Contest ID
    adif.add_field(Field::new("CONTEST_ID", contest_id));

    adif
}

fn band_to_adif(band: Band) -> &'static str {
    match band {
        Band::B160m => "160m",
        Band::B80m => "80m",
        Band::B40m => "40m",
        Band::B20m => "20m",
        Band::B15m => "15m",
        Band::B10m => "10m",
        Band::Other => "OTHER",
    }
}

fn mode_to_adif(mode: Mode) -> &'static str {
    match mode {
        Mode::CW => "CW",
        Mode::SSB => "SSB",
        Mode::Digital => "RTTY",
        Mode::Other => "OTHER",
    }
}
