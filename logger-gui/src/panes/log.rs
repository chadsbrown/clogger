//! Log pane: shows the last N QSOs from `LogAdapter`.
//!
//! A flat, striped table. Earlier versions drew every row as a bordered
//! card, which turned up to 512 rows into a wall of boxes. Now the
//! newest QSO is marked with an accent bar and tint, and a voided QSO
//! gets danger call text plus a solid VOID badge.

use iced::widget::text::Wrapping;
use iced::widget::{column, responsive, row, scrollable, text};
use iced::{Element, Length, Size, Theme};
use logger_runtime::{LogAdapter, decode_exchange_pairs};

use super::style;
use super::widgets::{self, Tone};

/// Render a bounded recent-history buffer inside a bottom-anchored
/// scrollable, so the newest QSOs are visible by default while the mouse
/// wheel can navigate upward through recent history.
const ROW_BUFFER: usize = 512;
/// Wide enough for a 4-digit QSO number. Contest logs regularly pass 999,
/// and the old 24px column overflowed at that point.
const COL_INDEX_W: f32 = 36.0;
const COL_CALL_W: f32 = 96.0;
const COL_BAND_W: f32 = 40.0;
const COL_MODE_W: f32 = 36.0;
const COL_UTC_W: f32 = 42.0;
const COL_VOID_W: f32 = 44.0;
const COL_SPACING: f32 = style::SP_3;
/// Number of column gaps in a row (8 columns).
const COL_GAPS: f32 = 7.0;

#[derive(Clone, Copy)]
struct LogColumns {
    rx_w: f32,
    tx_w: f32,
}

struct LogRowView {
    index: usize,
    callsign_norm: String,
    band: &'static str,
    mode: &'static str,
    time_utc: String,
    rx_exchange: String,
    tx_exchange: String,
    is_void: bool,
    newest: bool,
}

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    responsive(move |size| view_for_size(log, size))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_for_size<'a, M: 'a>(log: &'a LogAdapter, size: Size) -> Element<'a, M> {
    let records = log.records();
    // Subtract the pane padding and the row chrome (row padding plus the
    // accent-bar gutter) so the exchange columns fill exactly the
    // remaining width.
    let chrome = style::PAD_PANE.left
        + style::PAD_PANE.right
        + style::PAD_ROW.left
        + style::PAD_ROW.right
        + widgets::ACCENT_BAR_W;
    let cols = compute_columns((size.width - chrome).max(320.0));

    let start = records.len().saturating_sub(ROW_BUFFER);
    let tail = &records[start..];
    let mut rows: Vec<Element<M>> = Vec::with_capacity(tail.len());
    for (offset, rec) in tail.iter().enumerate() {
        let i = start + offset;
        let (rx_exchange, tx_exchange) = split_exchange_values(rec);
        let view = LogRowView {
            index: i + 1,
            callsign_norm: rec.callsign_norm.clone(),
            band: band_label(rec),
            mode: mode_label(rec),
            time_utc: format_log_time_utc(rec.ts_ms),
            rx_exchange,
            tx_exchange,
            is_void: rec.flags.is_void,
            newest: offset + 1 == tail.len(),
        };
        // Stripe by absolute QSO index so a row keeps its shade as new
        // QSOs arrive, instead of every row flickering on each log.
        rows.push(log_row(view, cols, i % 2 == 1));
    }

    let body: Element<M> = if rows.is_empty() {
        widgets::empty_state("No QSOs yet")
    } else {
        scrollable(column(rows))
            .anchor_bottom()
            .height(Length::Fill)
            .into()
    };

    widgets::pane_body(column![header_row(cols), body].spacing(style::SP_1))
}

fn mono<'a>(s: impl Into<String>, size: f32, width: f32) -> iced::widget::Text<'a> {
    text(s.into())
        .size(size)
        .font(style::FONT_MONO)
        .width(Length::Fixed(width))
}

fn log_row<'a, M: 'a>(row_data: LogRowView, cols: LogColumns, stripe: bool) -> Element<'a, M> {
    let LogRowView {
        index,
        callsign_norm,
        band,
        mode,
        time_utc,
        rx_exchange,
        tx_exchange,
        is_void,
        newest,
    } = row_data;
    let content = row![
        mono(index.to_string(), style::TEXT_TINY, COL_INDEX_W).style(style::very_muted),
        text(callsign_norm)
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO_BOLD)
            .width(Length::Fixed(COL_CALL_W))
            .style(move |t: &Theme| {
                if is_void {
                    style::danger(t)
                } else if newest {
                    style::accent(t)
                } else {
                    style::body(t)
                }
            }),
        mono(rx_exchange, style::TEXT_BODY, cols.rx_w)
            .style(style::body)
            .wrapping(Wrapping::WordOrGlyph),
        mono(tx_exchange, style::TEXT_BODY, cols.tx_w)
            .style(style::muted)
            .wrapping(Wrapping::WordOrGlyph),
        mono(band, style::TEXT_LABEL, COL_BAND_W).style(style::muted),
        mono(mode, style::TEXT_LABEL, COL_MODE_W).style(style::muted),
        mono(time_utc, style::TEXT_LABEL, COL_UTC_W).style(style::muted),
        void_badge(is_void),
    ]
    .spacing(COL_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .width(Length::Fill);

    widgets::table_row(content, stripe, newest.then_some(Tone::Accent))
}

fn header_row<'a, M: 'a>(cols: LogColumns) -> Element<'a, M> {
    widgets::table_header(
        vec![
            widgets::col_header("#", Length::Fixed(COL_INDEX_W)),
            widgets::col_header("Call", Length::Fixed(COL_CALL_W)),
            widgets::col_header("Rcvd", Length::Fixed(cols.rx_w)),
            widgets::col_header("Sent", Length::Fixed(cols.tx_w)),
            widgets::col_header("Band", Length::Fixed(COL_BAND_W)),
            widgets::col_header("Mode", Length::Fixed(COL_MODE_W)),
            widgets::col_header("UTC", Length::Fixed(COL_UTC_W)),
            widgets::col_header("", Length::Fixed(COL_VOID_W)),
        ],
        COL_SPACING,
    )
}

fn void_badge<'a, M: 'a>(is_void: bool) -> Element<'a, M> {
    if is_void {
        widgets::badge_solid("VOID", Tone::Danger)
    } else {
        iced::widget::Space::new().width(Length::Fixed(COL_VOID_W)).into()
    }
}

fn band_label(rec: &logger_runtime::QsoRecord) -> &'static str {
    match rec.band {
        logger_runtime::Band::B160m => "160m",
        logger_runtime::Band::B80m => "80m",
        logger_runtime::Band::B40m => "40m",
        logger_runtime::Band::B20m => "20m",
        logger_runtime::Band::B15m => "15m",
        logger_runtime::Band::B10m => "10m",
        logger_runtime::Band::Other => "?",
    }
}

fn mode_label(rec: &logger_runtime::QsoRecord) -> &'static str {
    match rec.mode {
        logger_runtime::Mode::CW => "CW",
        logger_runtime::Mode::SSB => "SSB",
        logger_runtime::Mode::Digital => "DIG",
        logger_runtime::Mode::Other => "?",
    }
}

fn split_exchange_values(rec: &logger_runtime::QsoRecord) -> (String, String) {
    let Ok(pairs) = decode_exchange_pairs(&rec.exchange) else {
        return ("-".to_string(), "-".to_string());
    };

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

    let mut tx_values = Vec::with_capacity(rx.len());
    let mut serial_placed = false;
    for (rx_key, _) in &rx {
        if let Some(pos) = sent_fields.iter().position(|(sk, _)| sk == rx_key) {
            tx_values.push(sent_fields[pos].1.clone());
        } else if let Some(ref s) = serial_value {
            tx_values.push(s.clone());
            serial_placed = true;
        }
    }
    if !serial_placed && let Some(s) = serial_value {
        tx_values.push(s);
    }

    (join_exchange_values(&rx_values), join_exchange_values(&tx_values))
}

fn join_exchange_values(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(" ")
    }
}

fn is_signal_report_key(k: &str) -> bool {
    matches!(k, "rst" | "rst_s" | "rst_r" | "sent_rst" | "sent_rst_s" | "sent_rst_r")
}

fn format_log_time_utc(ts_ms: u64) -> String {
    let secs = (ts_ms / 1000) as i64;
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => "--:--".to_string(),
    }
}

fn compute_columns(available_width: f32) -> LogColumns {
    let fixed = COL_INDEX_W
        + COL_CALL_W
        + COL_BAND_W
        + COL_MODE_W
        + COL_UTC_W
        + COL_VOID_W
        + COL_GAPS * COL_SPACING;
    let exchange_total = (available_width - fixed).max(140.0);
    let rx_w = (exchange_total * 0.6).max(80.0);
    let tx_w = (exchange_total - rx_w).max(60.0);
    LogColumns { rx_w, tx_w }
}
