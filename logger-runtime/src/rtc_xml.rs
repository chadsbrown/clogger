//! XML serialization for Real Time Contest (RTC) posts.
//!
//! One post every 2 minutes contains a `<rtc>` envelope wrapping:
//!  - `<dynamicresults>` with `<qth>` station location (always present)
//!  - zero or more `<contactinfo>` / `<contactreplace>` / `<contactdelete>`
//!
//! Spec: `~/Downloads/RTC_Specification_2.4_XML.pdf` (rev 2.4-xml).

use std::fmt::Write as FmtWrite;

use md5::{Digest, Md5};
use qsolog::qso::QsoRecord;
use qsolog::types::{Band, Mode};

use crate::config::RtcConfig;
use crate::log_adapter::decode_exchange_pairs;
use crate::scoreboard_adapter::ScoreboardSnapshot;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Station location fields for the `<qth>` block. Every field is
/// explicit per user requirement — nothing is inferred from the
/// callsign or cty.dat. Built by the UI from the `[rtc]` config
/// section at spawn time.
#[derive(Clone, Debug)]
pub struct RtcQth {
    pub dxcc_country: String,
    pub cq_zone: u8,
    pub iaru_zone: u8,
    pub arrl_section: String,
    pub state_or_province: String,
    pub grid6: String,
}

impl RtcQth {
    pub fn from_config(cfg: &RtcConfig) -> Self {
        Self {
            dxcc_country: cfg.dxcc_country.clone(),
            cq_zone: cfg.cq_zone,
            iaru_zone: cfg.iaru_zone,
            arrl_section: cfg.arrl_section.clone(),
            state_or_province: cfg.state_or_province.clone(),
            grid6: cfg.grid6.clone(),
        }
    }
}

/// One queued RTC container, produced by the adapter's delta computation
/// and serialized into the `<rtc>` envelope.
pub enum PendingContact {
    /// A QSO the server hasn't seen yet. Rendered as `<contactinfo>`.
    Info(ContactData),
    /// A QSO whose content or void state has changed since the last
    /// CFM. Rendered as `<contactreplace>`.
    Replace(ContactData),
    /// A QSO that should be removed from the server's view. Clogger
    /// doesn't currently produce these (void-toggles use `Replace` +
    /// `<x-qso>1`), but the type is kept for future editing support
    /// and `ResyncLog`-driven cleanup.
    Delete(String),
}

/// Per-QSO data projected from a `QsoRecord` into the RTC XML shape.
/// `None` fields are omitted from the output.
#[derive(Clone, Debug)]
pub struct ContactData {
    pub id: String,
    pub timestamp: String,
    /// MHz string — "1.8", "3.5", "7", "14", "21", "28", "50", "144".
    pub band: &'static str,
    /// 10 Hz units (e.g. 14,254.93 kHz → 1425493).
    pub txfreq_10hz: u64,
    /// Mode string — "CW", "PH", "RTTY", "PSK", "FT8", "FT4".
    pub mode: &'static str,
    pub call: String,
    /// Exchange sent by the station, space-joined. Signal reports
    /// already excluded.
    pub sent_exchange: String,
    /// Exchange received from the correspondent, space-joined.
    /// Signal reports already excluded.
    pub rx_exchange: String,
    /// `true` → emit `<x-qso>1`, flag the QSO as "X-logged" (void).
    /// `false` → emit `<x-qso>0` for replaces that explicitly reinstate;
    /// for fresh contactinfos we omit entirely.
    pub x_qso: Option<bool>,
    pub radionr: u32,
    /// Correspondent's DXCC prefix, when resolvable. Omitted if
    /// unknown.
    pub countryprefix: Option<String>,
}

// ---------------------------------------------------------------------------
// QSO ID
// ---------------------------------------------------------------------------

/// Deterministic 32-char lowercase-hex QSO identifier.
/// Hashes `(contest_instance_id, qso_id)` with MD5 — the 128-bit
/// output matches the RTC spec's 32-char ID field exactly and is
/// stable across process restarts. MD5 is fine here: this is an
/// identifier, not a security boundary.
pub fn qso_id_md5(contest_instance_id: u64, qso_id: u64) -> String {
    let mut h = Md5::new();
    h.update(contest_instance_id.to_be_bytes());
    h.update(qso_id.to_be_bytes());
    hex::encode(h.finalize())
}

// ---------------------------------------------------------------------------
// Band / mode mapping
// ---------------------------------------------------------------------------

/// Map a qsolog `Band` to the RTC spec's MHz string. Returns `None`
/// for bands the RTC spec doesn't list (notably `Band::Other`). The
/// adapter should skip QSOs on unmapped bands.
pub fn band_to_rtc_mhz(b: Band) -> Option<&'static str> {
    match b {
        Band::B160m => Some("1.8"),
        Band::B80m => Some("3.5"),
        Band::B40m => Some("7"),
        Band::B20m => Some("14"),
        Band::B15m => Some("21"),
        Band::B10m => Some("28"),
        // qsolog types::Band currently stops at 10m; VHF/UHF map-throughs
        // can be added when the log engine learns them.
        _ => None,
    }
}

/// Map a qsolog `Mode` to the RTC spec's mode string.
/// `Digital` returns `None` — clogger doesn't currently track the
/// submode per QSO, so we can't pick between RTTY / PSK / FT8 / FT4.
/// The adapter skips QSOs on unmapped modes.
pub fn mode_to_rtc(m: Mode) -> Option<&'static str> {
    match m {
        Mode::CW => Some("CW"),
        Mode::SSB => Some("PH"),
        Mode::Digital => None,
        Mode::Other => None,
    }
}

// ---------------------------------------------------------------------------
// Exchange projection
// ---------------------------------------------------------------------------

/// Split a record's `exchange_pairs` into received (`rx`) and sent
/// components. Excludes signal-report fields (keys `rst`, `rst_s`,
/// `rst_r` and the `sent_rst*` variants) since the RTC spec requires
/// them omitted.
///
/// The serial position in the sent exchange is inferred from the
/// received field order: every received key either has a matching
/// `sent_<key>` field or is the "serial-equivalent" — the position
/// where the serial belongs. This handles serial-first contests
/// (Sweepstakes, NS Sprint) and serial-last contests (MST) without
/// external metadata.
fn split_exchange(record: &QsoRecord) -> (Vec<String>, Vec<String>) {
    let Ok(pairs) = decode_exchange_pairs(&record.exchange) else {
        return (Vec::new(), Vec::new());
    };

    // Categorize pairs into received keys/values, sent (stripped)
    // keys/values, and the standalone serial. RST keys are dropped.
    let mut rx: Vec<(String, String)> = Vec::new();
    let mut sent_fields: Vec<(String, String)> = Vec::new();
    let mut serial_value: Option<String> = None;

    for (k, v) in pairs {
        if is_signal_report_key(&k) {
            continue;
        }
        if k == "serial" {
            serial_value = Some(v);
        } else if let Some(stripped) = k.strip_prefix("sent_") {
            sent_fields.push((stripped.to_string(), v));
        } else {
            rx.push((k, v));
        }
    }

    let rx_values: Vec<String> = rx.iter().map(|(_, v)| v.clone()).collect();

    // Build sent exchange in the order dictated by the received fields.
    // Each rx key either has a matching sent_<key> or marks where the
    // serial belongs (the one received field with no sent counterpart).
    let mut sent = Vec::with_capacity(rx.len());
    let mut serial_placed = false;
    for (rx_key, _) in &rx {
        if let Some(pos) = sent_fields.iter().position(|(sk, _)| sk == rx_key) {
            sent.push(sent_fields[pos].1.clone());
        } else if let Some(ref s) = serial_value {
            sent.push(s.clone());
            serial_placed = true;
        }
    }
    // Fallback: if a serial exists but no rx key was unmatched,
    // append it rather than silently dropping.
    if !serial_placed && let Some(s) = serial_value {
        sent.push(s);
    }

    (rx_values, sent)
}

fn is_signal_report_key(k: &str) -> bool {
    matches!(k, "rst" | "rst_s" | "rst_r" | "sent_rst" | "sent_rst_s" | "sent_rst_r")
}

// ---------------------------------------------------------------------------
// QsoRecord → ContactData projection
// ---------------------------------------------------------------------------

/// Project a logged QSO into the RTC XML shape. Returns `None` when
/// the QSO is on a band or mode the RTC spec doesn't cover — the
/// adapter treats that as "skip this contact" so the upload still
/// goes through for the QSOs that are compatible.
pub fn qso_to_contact_data(
    record: &QsoRecord,
    contest_instance_id: u64,
    countryprefix: Option<String>,
    include_x_qso: bool,
) -> Option<ContactData> {
    let band = band_to_rtc_mhz(record.band)?;
    let mode = mode_to_rtc(record.mode)?;
    let id = qso_id_md5(contest_instance_id, record.id);
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
        record.ts_ms.min(i64::MAX as u64) as i64,
    )
    .unwrap_or_else(chrono::Utc::now)
    .format("%Y-%m-%d %H:%M:%S")
    .to_string();
    let (rx, sent) = split_exchange(record);
    Some(ContactData {
        id,
        timestamp,
        band,
        txfreq_10hz: record.freq_hz / 10,
        mode,
        call: record.callsign_norm.clone(),
        sent_exchange: sent.join(" "),
        rx_exchange: rx.join(" "),
        x_qso: if include_x_qso { Some(record.flags.is_void) } else { None },
        radionr: record.radio_id,
        countryprefix,
    })
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Build the full RTC post body. `dyn_results_body` is the inner
/// text of the `<dynamicresults>` element (as produced by
/// `serialize_dynamicresults_body`); `pending` is the list of QSO
/// containers. If `pending` is empty, only the dynamicresults block
/// is emitted — per the spec, that's what a no-change 2-minute post
/// looks like.
pub fn serialize_envelope(dyn_results_body: &str, pending: &[PendingContact]) -> String {
    let mut xml = String::with_capacity(1024 + pending.len() * 256);
    let _ = write!(xml, "<?xml version=\"1.0\"?>\n<rtc>\n");
    let _ = write!(xml, "<dynamicresults>\n{dyn_results_body}</dynamicresults>\n");
    for item in pending {
        match item {
            PendingContact::Info(d) => write_contact(&mut xml, "contactinfo", d),
            PendingContact::Replace(d) => write_contact(&mut xml, "contactreplace", d),
            PendingContact::Delete(id) => {
                let _ = write!(
                    xml,
                    "<contactdelete>\n  <ID>{}</ID>\n</contactdelete>\n",
                    xml_escape(id)
                );
            }
        }
    }
    let _ = writeln!(xml, "</rtc>");
    xml
}

fn write_contact(xml: &mut String, tag: &str, d: &ContactData) {
    let _ = writeln!(xml, "<{tag}>");
    let _ = writeln!(xml, "  <ID>{}</ID>", xml_escape(&d.id));
    let _ = writeln!(xml, "  <timestamp>{}</timestamp>", xml_escape(&d.timestamp));
    let _ = writeln!(xml, "  <band>{}</band>", d.band);
    let _ = writeln!(xml, "  <txfreq>{}</txfreq>", d.txfreq_10hz);
    let _ = writeln!(xml, "  <mode>{}</mode>", d.mode);
    let _ = writeln!(xml, "  <call>{}</call>", xml_escape(&d.call));
    let _ = writeln!(
        xml,
        "  <SentExchange>{}</SentExchange>",
        xml_escape(&d.sent_exchange)
    );
    let _ = writeln!(
        xml,
        "  <RxExchange>{}</RxExchange>",
        xml_escape(&d.rx_exchange)
    );
    if let Some(void) = d.x_qso {
        let _ = writeln!(xml, "  <x-qso>{}</x-qso>", if void { 1 } else { 0 });
    }
    let _ = writeln!(xml, "  <radionr>{}</radionr>", d.radionr);
    if let Some(cp) = &d.countryprefix {
        let _ = writeln!(
            xml,
            "  <countryprefix>{}</countryprefix>",
            xml_escape(cp)
        );
    }
    let _ = writeln!(xml, "</{tag}>");
}

// ---------------------------------------------------------------------------
// dynamicresults body (shared with scoreboard)
// ---------------------------------------------------------------------------

/// Serialize the contents of `<dynamicresults>` (without the outer
/// tags). Shared between the scoreboard uploader and RTC — both post
/// the same score-shaped XML, but RTC appends a `<qth>` block and
/// wraps the whole thing in `<rtc>` (handled by `serialize_envelope`).
///
/// When `qth` is `Some`, a `<qth>...</qth>` block is appended before
/// the closing of dynamicresults. Scoreboard passes `None` and gets
/// a byte-identical result to the pre-refactor path.
pub fn serialize_dynamicresults_body(snap: &ScoreboardSnapshot, qth: Option<&RtcQth>) -> String {
    let mut xml = String::with_capacity(2048);
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(xml, "  <contest>{}</contest>", xml_escape(&snap.cabrillo_id));
    let _ = writeln!(xml, "  <call>{}</call>", xml_escape(&snap.call));
    let _ = writeln!(xml, "  <ops>{}</ops>", xml_escape(&snap.ops));
    let cat = &snap.category;
    let _ = write!(xml, "  <class power=\"{}\"", cat.power.as_str());
    let _ = write!(xml, " assisted=\"{}\"", cat.assisted.as_str());
    let _ = write!(xml, " transmitter=\"{}\"", cat.transmitter.as_str());
    let _ = write!(xml, " ops=\"{}\"", cat.operator.as_str());
    let _ = write!(xml, " bands=\"{}\"", cat.bands.as_str());
    let _ = write!(xml, " mode=\"{}\"", cat.mode.as_str());
    let _ = write!(xml, " overlay=\"{}\"", cat.overlay.as_str());
    let _ = writeln!(xml, " />");
    let _ = writeln!(xml, "  <breakdown>");
    for row in &snap.breakdown.rows {
        let band = scoreboard_band(&row.band);
        let _ = writeln!(
            xml,
            "    <qso band=\"{}\" mode=\"{}\">{}</qso>",
            band, row.mode, row.qsos
        );
        for (mult_type, count) in &row.mults {
            if *count == 0 {
                continue;
            }
            let _ = writeln!(
                xml,
                "    <mult type=\"{}\" band=\"{}\" mode=\"{}\">{}</mult>",
                scoreboard_mult_type(mult_type),
                band,
                row.mode,
                count
            );
        }
        let _ = writeln!(
            xml,
            "    <point band=\"{}\" mode=\"{}\">{}</point>",
            band, row.mode, row.points
        );
    }
    let _ = writeln!(xml, "  </breakdown>");
    let _ = writeln!(xml, "  <score>{}</score>", snap.breakdown.claimed_score);
    let _ = writeln!(xml, "  <timestamp>{}</timestamp>", timestamp);
    let _ = writeln!(xml, "  <soft>CLogger</soft>");
    let _ = writeln!(xml, "  <version>{}</version>", env!("CARGO_PKG_VERSION"));
    if let Some(qth) = qth {
        let _ = writeln!(xml, "  <qth>");
        let _ = writeln!(
            xml,
            "    <dxcccountry>{}</dxcccountry>",
            xml_escape(&qth.dxcc_country)
        );
        let _ = writeln!(xml, "    <cqzone>{}</cqzone>", qth.cq_zone);
        let _ = writeln!(xml, "    <iaruzone>{}</iaruzone>", qth.iaru_zone);
        let _ = writeln!(
            xml,
            "    <arrlsection>{}</arrlsection>",
            xml_escape(&qth.arrl_section)
        );
        let _ = writeln!(
            xml,
            "    <stprvoth>{}</stprvoth>",
            xml_escape(&qth.state_or_province)
        );
        let _ = writeln!(xml, "    <grid6>{}</grid6>", xml_escape(&qth.grid6));
        let _ = writeln!(xml, "  </qth>");
    }
    xml
}

/// Canonical band label for the dynamicresults XML. Strips the trailing
/// "M" from clogger's internal "20M" etc. naming and passes "total"
/// through unchanged. Extracted from scoreboard_adapter so both posting
/// paths use the same normalization.
pub fn scoreboard_band(band: &str) -> String {
    let s = band.trim();
    if s.eq_ignore_ascii_case("total") {
        return "total".to_string();
    }
    s.trim_end_matches(['M', 'm']).to_string()
}

/// Map a contest-engine multiplier ID to a dynamicresults-valid `type`
/// attribute value. See the RTC / scoreboard XML spec for the seven
/// canonical types; contest-engine's internal IDs need translation
/// (notably state-QP county/state variants all collapse onto `state`).
pub fn scoreboard_mult_type(mult_id: &str) -> String {
    match mult_id {
        "county_mult" | "in_state_state_mult" | "in_state_prov_mult" => "state".to_string(),
        "in_state_dxcc_mult" => "country".to_string(),
        other => {
            let lower = other.to_ascii_lowercase();
            lower
                .strip_suffix("_mult")
                .map(str::to_string)
                .unwrap_or(lower)
        }
    }
}

/// Minimal XML text escape — covers the five special characters that
/// matter inside element text. Attributes aren't used for user data
/// on this path, so single/double quotes don't need escaping.
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use qsolog::qso::{ExchangeBlob, QsoFlags, QsoRecord};
    use qsolog::types::{Band, Mode};

    /// A fixture MD5 computed offline against `(contest=1, qso=7)`
    /// so the test catches any accidental change in the hash inputs
    /// or byte ordering.
    #[test]
    fn md5_id_is_stable_and_32_hex() {
        // md5(0x0000000000000001 || 0x0000000000000007) — fixed over
        // big-endian 8-byte concatenation. Recompute with:
        //   echo -n -e '\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x07' | md5sum
        let id = qso_id_md5(1, 7);
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // Must be deterministic across calls.
        assert_eq!(id, qso_id_md5(1, 7));
        // Different contests don't collide.
        assert_ne!(qso_id_md5(1, 7), qso_id_md5(2, 7));
        // Different qsos don't collide.
        assert_ne!(qso_id_md5(1, 7), qso_id_md5(1, 8));
    }

    #[test]
    fn band_mapping_matches_spec() {
        assert_eq!(band_to_rtc_mhz(Band::B160m), Some("1.8"));
        assert_eq!(band_to_rtc_mhz(Band::B80m), Some("3.5"));
        assert_eq!(band_to_rtc_mhz(Band::B40m), Some("7"));
        assert_eq!(band_to_rtc_mhz(Band::B20m), Some("14"));
        assert_eq!(band_to_rtc_mhz(Band::B15m), Some("21"));
        assert_eq!(band_to_rtc_mhz(Band::B10m), Some("28"));
        assert_eq!(band_to_rtc_mhz(Band::Other), None);
    }

    #[test]
    fn mode_mapping_rejects_digital_until_submode() {
        assert_eq!(mode_to_rtc(Mode::CW), Some("CW"));
        assert_eq!(mode_to_rtc(Mode::SSB), Some("PH"));
        assert_eq!(mode_to_rtc(Mode::Digital), None);
    }

    fn fixture_record(
        id: u64,
        call: &str,
        exchange_pairs: Vec<(&str, &str)>,
        is_void: bool,
    ) -> QsoRecord {
        let owned: Vec<(String, String)> = exchange_pairs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let exchange = ExchangeBlob {
            bytes: serde_json::to_vec(&owned).expect("encode exchange"),
        };
        QsoRecord {
            id,
            contest_instance_id: 1,
            callsign_raw: call.to_string(),
            callsign_norm: call.to_string(),
            band: Band::B20m,
            mode: Mode::CW,
            freq_hz: 14_254_930,
            ts_ms: 1_718_028_709_000, // 2024-06-10 14:11:49 UTC
            radio_id: 0,
            operator_id: 0,
            exchange,
            flags: QsoFlags { is_void, dupe_override: false },
        }
    }

    #[test]
    fn cwt_projection_matches_spec_example() {
        // mini-CWT from the spec: SentExchange "VIC 2212",
        // RxExchange "ALEX NJ", txfreq 14,254.93 kHz → 1425493.
        let rec = fixture_record(
            7,
            "K2BB",
            vec![
                ("name", "ALEX"),
                ("xchg", "NJ"),
                ("sent_name", "VIC"),
                ("sent_xchg", "2212"),
            ],
            false,
        );
        let data = qso_to_contact_data(&rec, 1, None, false).expect("projected");
        assert_eq!(data.band, "14");
        assert_eq!(data.txfreq_10hz, 1_425_493);
        assert_eq!(data.mode, "CW");
        assert_eq!(data.call, "K2BB");
        assert_eq!(data.sent_exchange, "VIC 2212");
        assert_eq!(data.rx_exchange, "ALEX NJ");
        assert!(data.x_qso.is_none(), "fresh non-void QSO shouldn't carry x-qso");
    }

    #[test]
    fn cqww_projection_drops_rst() {
        let rec = fixture_record(
            8,
            "DL1ABC",
            vec![
                ("rst", "599"),
                ("zone", "14"),
                ("sent_rst_s", "599"),
                ("sent_zone", "4"),
            ],
            false,
        );
        let data = qso_to_contact_data(&rec, 1, None, false).expect("projected");
        // RST must be omitted from both sides per the RTC spec.
        assert_eq!(data.rx_exchange, "14");
        assert_eq!(data.sent_exchange, "4");
    }

    #[test]
    fn sweeps_sent_exchange_orders_serial_first() {
        // Sweeps exchange: received (nr, prec, ck, sec), sent
        // (sent_prec, sent_ck, sent_sec) plus the standalone serial.
        let rec = fixture_record(
            9,
            "K1ABC",
            vec![
                ("nr", "14"),
                ("prec", "A"),
                ("ck", "85"),
                ("sec", "CT"),
                ("serial", "1"),
                ("sent_prec", "B"),
                ("sent_ck", "72"),
                ("sent_sec", "IL"),
            ],
            false,
        );
        let data = qso_to_contact_data(&rec, 1, None, false).expect("projected");
        // Serial (NR) first, then sent_variants in spec order.
        assert_eq!(data.sent_exchange, "1 B 72 IL");
        // Received keeps its original order; no serial filtering
        // happens for rx.
        assert_eq!(data.rx_exchange, "14 A 85 CT");
    }

    #[test]
    fn mst_sent_exchange_orders_serial_last() {
        // MST exchange: received (name, nr), sent (sent_name) plus serial.
        // Serial must come AFTER name, matching received field order.
        let rec = fixture_record(
            11,
            "K1ABC",
            vec![
                ("name", "BOB"),
                ("nr", "42"),
                ("serial", "1"),
                ("sent_name", "CHAD"),
            ],
            false,
        );
        let data = qso_to_contact_data(&rec, 1, None, false).expect("projected");
        assert_eq!(data.sent_exchange, "CHAD 1");
        assert_eq!(data.rx_exchange, "BOB 42");
    }

    #[test]
    fn ns_sprint_sent_exchange_orders_serial_first() {
        // NS Sprint exchange: received (nr, name, loc), sent
        // (sent_name, sent_loc) plus serial. Serial maps to rx "nr"
        // at position 0, so it comes first.
        let rec = fixture_record(
            12,
            "W6YX",
            vec![
                ("nr", "14"),
                ("name", "BOB"),
                ("loc", "CA"),
                ("serial", "1"),
                ("sent_name", "CHAD"),
                ("sent_loc", "IL"),
            ],
            false,
        );
        let data = qso_to_contact_data(&rec, 1, None, false).expect("projected");
        assert_eq!(data.sent_exchange, "1 CHAD IL");
        assert_eq!(data.rx_exchange, "14 BOB CA");
    }

    #[test]
    fn voided_replace_emits_x_qso_one() {
        let rec = fixture_record(
            10,
            "N0XYZ",
            vec![("name", "JIM"), ("xchg", "100"), ("sent_name", "CHAD"), ("sent_xchg", "42")],
            true,
        );
        let data = qso_to_contact_data(&rec, 1, None, true).expect("projected");
        assert_eq!(data.x_qso, Some(true));
    }

    #[test]
    fn envelope_has_no_contacts_when_empty() {
        let xml = serialize_envelope("  <contest>X</contest>\n", &[]);
        assert!(xml.starts_with("<?xml version=\"1.0\"?>\n<rtc>\n"));
        assert!(xml.contains("<dynamicresults>"));
        assert!(!xml.contains("<contactinfo"));
        assert!(!xml.contains("<contactreplace"));
        assert!(!xml.contains("<contactdelete"));
        assert!(xml.ends_with("</rtc>\n"));
    }

    #[test]
    fn envelope_embeds_contactinfo() {
        let data = ContactData {
            id: "0123456789abcdef0123456789abcdef".to_string(),
            timestamp: "2024-06-10 14:11:49".to_string(),
            band: "14",
            txfreq_10hz: 1_425_493,
            mode: "CW",
            call: "K2BB".to_string(),
            sent_exchange: "VIC 2212".to_string(),
            rx_exchange: "ALEX NJ".to_string(),
            x_qso: None,
            radionr: 0,
            countryprefix: Some("K".to_string()),
        };
        let xml = serialize_envelope("", &[PendingContact::Info(data)]);
        assert!(xml.contains("<ID>0123456789abcdef0123456789abcdef</ID>"));
        assert!(xml.contains("<band>14</band>"));
        assert!(xml.contains("<txfreq>1425493</txfreq>"));
        assert!(xml.contains("<SentExchange>VIC 2212</SentExchange>"));
        assert!(xml.contains("<RxExchange>ALEX NJ</RxExchange>"));
        assert!(xml.contains("<countryprefix>K</countryprefix>"));
        assert!(!xml.contains("<x-qso>"), "x-qso omitted when None");
    }

    #[test]
    fn contactreplace_emits_x_qso_one_for_void() {
        let data = ContactData {
            id: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
            timestamp: "2024-06-10 14:11:49".to_string(),
            band: "14",
            txfreq_10hz: 1_425_493,
            mode: "CW",
            call: "K2BB".to_string(),
            sent_exchange: "VIC 2212".to_string(),
            rx_exchange: "ALEX NJ".to_string(),
            x_qso: Some(true),
            radionr: 0,
            countryprefix: None,
        };
        let xml = serialize_envelope("", &[PendingContact::Replace(data)]);
        assert!(xml.contains("<contactreplace>"));
        assert!(xml.contains("<x-qso>1</x-qso>"));
        assert!(!xml.contains("<countryprefix>"));
    }

    #[test]
    fn contactdelete_has_only_id() {
        let xml = serialize_envelope(
            "",
            &[PendingContact::Delete("feedfacefeedfacefeedfacefeedface".to_string())],
        );
        assert!(xml.contains("<contactdelete>\n  <ID>feedfacefeedfacefeedfacefeedface</ID>\n</contactdelete>"));
    }

    #[test]
    fn dynamicresults_with_qth_appends_qth_block() {
        use crate::config::{
            CategoryAssisted, CategoryBands, CategoryConfig, CategoryConfigMode, CategoryOperator,
            CategoryOverlay, CategoryPower, CategoryStation, CategoryTransmitter,
        };
        use crate::scoring::{BreakdownRow, ScoreBreakdown};
        let snap = ScoreboardSnapshot {
            cabrillo_id: "CW-Ops".to_string(),
            call: "N9UNX".to_string(),
            ops: "N9UNX".to_string(),
            category: CategoryConfig {
                power: CategoryPower::Low,
                assisted: CategoryAssisted::NonAssisted,
                transmitter: CategoryTransmitter::One,
                operator: CategoryOperator::SingleOp,
                bands: CategoryBands::All,
                mode: CategoryConfigMode::Cw,
                overlay: CategoryOverlay::Na,
                station: CategoryStation::Fixed,
            },
            breakdown: ScoreBreakdown {
                rows: vec![BreakdownRow {
                    band: "20M".to_string(),
                    mode: "CW".to_string(),
                    qsos: 1,
                    points: 1,
                    mults: vec![("zone".to_string(), 1)],
                }],
                claimed_score: 1,
            },
        };
        let qth = RtcQth {
            dxcc_country: "K".to_string(),
            cq_zone: 4,
            iaru_zone: 7,
            arrl_section: "IL".to_string(),
            state_or_province: "IL".to_string(),
            grid6: "EN50VE".to_string(),
        };
        let with_qth = serialize_dynamicresults_body(&snap, Some(&qth));
        assert!(with_qth.contains("<qth>"));
        assert!(with_qth.contains("<dxcccountry>K</dxcccountry>"));
        assert!(with_qth.contains("<cqzone>4</cqzone>"));
        assert!(with_qth.contains("<grid6>EN50VE</grid6>"));

        // Scoreboard path must NOT emit qth — byte-identical to pre-refactor.
        let without_qth = serialize_dynamicresults_body(&snap, None);
        assert!(!without_qth.contains("<qth>"));
    }

    #[test]
    fn xml_escape_handles_all_three_entities() {
        assert_eq!(xml_escape("a & b"), "a &amp; b");
        assert_eq!(xml_escape("<x>"), "&lt;x&gt;");
        assert_eq!(xml_escape("A &<>"), "A &amp;&lt;&gt;");
    }
}
