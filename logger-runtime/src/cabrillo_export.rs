//! Cabrillo v3 contest log export. Plain-text, two-column-tagged header
//! followed by chronological QSO/X-QSO body, terminated by `END-OF-LOG:`.
//! See <https://wwrof.org/cabrillo/> for the canonical spec.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use chrono::DateTime;
use logger_core::ContestEntry;
use qsolog::qso::QsoRecord;
use qsolog::types::Mode;

use crate::config::{CabrilloConfig, CategoryConfig, CategoryOverlay};
use crate::log_adapter::decode_exchange_pairs;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Export the log as a Cabrillo v3 file at `path`. Returns the number of
/// records written (including voided ones, which appear as `X-QSO:` lines
/// so log-checkers can cross-reference busts).
///
/// `records` should already be in chronological order — `LogAdapter::ordered_records()`
/// satisfies this. `claimed_score` comes from `LogAdapter::score_summary()`.
pub fn export_cabrillo(
    records: &[QsoRecord],
    contest: &dyn ContestEntry,
    category: &CategoryConfig,
    header: &CabrilloConfig,
    my_call: &str,
    claimed_score: i64,
    path: &Path,
) -> Result<usize> {
    let cabrillo_id = contest
        .cabrillo_id(category.mode.to_category_mode())
        .ok_or_else(|| {
            anyhow!(
                "contest '{}' has no Cabrillo ID for mode '{}'",
                contest.contest_id(),
                category.mode.as_str(),
            )
        })?;

    let body = build_cabrillo(records, cabrillo_id, category, header, my_call, claimed_score);
    std::fs::write(path, body)?;
    Ok(records.len())
}

fn build_cabrillo(
    records: &[QsoRecord],
    cabrillo_id: &str,
    category: &CategoryConfig,
    header: &CabrilloConfig,
    my_call: &str,
    claimed_score: i64,
) -> String {
    let mut out = String::new();
    push_line(&mut out, "START-OF-LOG: 3.0");
    push_line(&mut out, &format!("CONTEST: {cabrillo_id}"));
    push_line(&mut out, &format!("CALLSIGN: {my_call}"));
    push_line(&mut out, &format!("CATEGORY-OPERATOR: {}", category.operator.as_str()));
    push_line(&mut out, &format!("CATEGORY-ASSISTED: {}", category.assisted.as_str()));
    push_line(&mut out, &format!("CATEGORY-BAND: {}", category.bands.as_str()));
    push_line(&mut out, &format!("CATEGORY-MODE: {}", category.mode.as_str()));
    push_line(&mut out, &format!("CATEGORY-POWER: {}", category.power.as_str()));
    push_line(&mut out, &format!("CATEGORY-STATION: {}", category.station.as_str()));
    push_line(&mut out, &format!("CATEGORY-TRANSMITTER: {}", category.transmitter.as_str()));
    if category.overlay != CategoryOverlay::Na {
        push_line(&mut out, &format!("CATEGORY-OVERLAY: {}", category.overlay.as_str()));
    }
    push_line(&mut out, &format!("CLAIMED-SCORE: {claimed_score}"));
    push_line(&mut out, &format!("CREATED-BY: clogger {VERSION}"));

    if !header.operators.is_empty() {
        push_line(&mut out, &format!("OPERATORS: {}", header.operators.join(" ")));
    }
    if let Some(name) = &header.name {
        push_line(&mut out, &format!("NAME: {name}"));
    }
    if let Some(loc) = &header.location {
        push_line(&mut out, &format!("LOCATION: {loc}"));
    }
    if let Some(grid) = &header.grid_locator {
        push_line(&mut out, &format!("GRID-LOCATOR: {grid}"));
    }
    if let Some(email) = &header.email {
        push_line(&mut out, &format!("EMAIL: {email}"));
    }
    if let Some(club) = &header.club {
        push_line(&mut out, &format!("CLUB: {club}"));
    }
    for line in &header.address {
        push_line(&mut out, &format!("ADDRESS: {line}"));
    }
    for line in &header.soapbox {
        push_line(&mut out, &format!("SOAPBOX: {line}"));
    }

    for rec in records {
        let tag = if rec.flags.is_void { "X-QSO:" } else { "QSO:" };
        push_line(&mut out, &format!("{tag} {}", format_qso_line(rec, my_call)));
    }
    push_line(&mut out, "END-OF-LOG:");
    out
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push_str("\r\n");
}

fn format_qso_line(rec: &QsoRecord, my_call: &str) -> String {
    let freq_khz = rec.freq_hz / 1000;
    let mo = mode_to_cabrillo(rec.mode);
    let dt = DateTime::from_timestamp_millis(rec.ts_ms as i64)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    let date = dt.format("%Y-%m-%d");
    let time = dt.format("%H%M");
    let (rcvd_tokens, sent_tokens) = exchange_tokens(rec);
    let sent = sent_tokens.join(" ");
    let rcvd = rcvd_tokens.join(" ");
    format!(
        "{:>5} {:<2} {} {} {:<13} {} {:<13} {} 0",
        freq_khz, mo, date, time, my_call, sent, rec.callsign_raw, rcvd,
    )
}

fn mode_to_cabrillo(mode: Mode) -> &'static str {
    match mode {
        Mode::CW => "CW",
        Mode::SSB => "PH",
        // qsolog buckets RTTY and other digital under Mode::Digital;
        // RTTY is the dominant contest digital mode, matching adif_export.
        Mode::Digital => "RY",
        Mode::Other => "CW",
    }
}

/// Walk decoded exchange pairs and split them into received and sent
/// token lists in matched order. Mirrors `rtc_xml::split_exchange` but
/// retains RST keys (Cabrillo emits them; RTC strips them).
///
/// The received side preserves the order pairs were stored in. The sent
/// side is reconstructed by looking up `sent_<key>` for each rx key, or
/// falling back to the standalone `serial` value at the position of the
/// one rx field with no `sent_<key>` counterpart.
fn exchange_tokens(rec: &QsoRecord) -> (Vec<String>, Vec<String>) {
    let Ok(pairs) = decode_exchange_pairs(&rec.exchange) else {
        return (Vec::new(), Vec::new());
    };
    let mut rx: Vec<(String, String)> = Vec::new();
    let mut sent_map: HashMap<String, String> = HashMap::new();
    let mut serial: Option<String> = None;

    for (k, v) in pairs {
        if k == "serial" {
            serial = Some(v);
        } else if let Some(stripped) = k.strip_prefix("sent_") {
            sent_map.insert(stripped.to_string(), v);
        } else {
            rx.push((k, v));
        }
    }

    let rcvd: Vec<String> = rx.iter().map(|(_, v)| v.clone()).collect();
    let mut sent: Vec<String> = Vec::with_capacity(rx.len());
    let mut serial_placed = false;
    for (rx_key, _) in &rx {
        if let Some(v) = sent_map.get(rx_key) {
            sent.push(v.clone());
        } else if let Some(ref s) = serial {
            sent.push(s.clone());
            serial_placed = true;
        }
    }
    if !serial_placed
        && let Some(s) = serial
    {
        sent.push(s);
    }

    (rcvd, sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qsolog::qso::{ExchangeBlob, QsoFlags, QsoRecord};
    use qsolog::types::{Band, Mode};

    fn cwt_record(is_void: bool) -> QsoRecord {
        let pairs: Vec<(String, String)> = vec![
            ("rst".to_string(), "599".to_string()),
            ("name".to_string(), "BOB".to_string()),
            ("nr".to_string(), "1234".to_string()),
            ("sent_rst".to_string(), "599".to_string()),
            ("sent_name".to_string(), "ALICE".to_string()),
            ("sent_nr".to_string(), "5678".to_string()),
        ];
        QsoRecord {
            id: 1,
            contest_instance_id: 1,
            callsign_raw: "K2ABC".to_string(),
            callsign_norm: "K2ABC".to_string(),
            band: Band::B20m,
            mode: Mode::CW,
            freq_hz: 14_042_000,
            ts_ms: 1_770_000_000_000, // 2026-02-02 02:40:00 UTC
            radio_id: 0,
            operator_id: 0,
            exchange: ExchangeBlob {
                bytes: serde_json::to_vec(&pairs).unwrap(),
            },
            flags: QsoFlags { is_void, dupe_override: false },
        }
    }

    #[test]
    fn qso_line_has_correct_fields_in_order() {
        let rec = cwt_record(false);
        let line = format_qso_line(&rec, "N1XYZ");
        // Expected: "14042 CW 2026-02-01 1440 N1XYZ         599 ALICE 5678 K2ABC         599 BOB 1234 0"
        assert!(line.contains("14042"));
        assert!(line.contains(" CW "));
        assert!(line.contains("2026-02-02"));
        assert!(line.contains(" 0240 "));
        assert!(line.contains("N1XYZ"));
        assert!(line.contains("K2ABC"));
        assert!(line.contains("599 ALICE 5678"));
        assert!(line.contains("599 BOB 1234"));
        assert!(line.ends_with(" 0"));
        // Sent side comes BEFORE received side.
        let alice_pos = line.find("ALICE").unwrap();
        let bob_pos = line.find("BOB").unwrap();
        assert!(alice_pos < bob_pos);
    }

    #[test]
    fn voided_record_emits_x_qso_tag() {
        let rec = cwt_record(true);
        let mut out = String::new();
        let tag = if rec.flags.is_void { "X-QSO:" } else { "QSO:" };
        push_line(&mut out, &format!("{tag} {}", format_qso_line(&rec, "N1XYZ")));
        assert!(out.starts_with("X-QSO: "));
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn lines_use_crlf() {
        let mut out = String::new();
        push_line(&mut out, "START-OF-LOG: 3.0");
        assert_eq!(out, "START-OF-LOG: 3.0\r\n");
    }

    #[test]
    fn mode_mapping() {
        assert_eq!(mode_to_cabrillo(Mode::CW), "CW");
        assert_eq!(mode_to_cabrillo(Mode::SSB), "PH");
        assert_eq!(mode_to_cabrillo(Mode::Digital), "RY");
    }
}
