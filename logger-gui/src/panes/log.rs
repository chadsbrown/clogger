//! Log pane — shows the last N QSOs from `LogAdapter`.

use iced::widget::{column, container, responsive, row, scrollable, text};
use iced::widget::text::Wrapping;
use iced::{Element, Font, Length, Size, Theme};
use logger_runtime::{LogAdapter, decode_exchange_pairs};

use super::style;

/// Render a bounded recent-history buffer inside a bottom-anchored
/// scrollable, so the newest QSOs are visible by default while the mouse
/// wheel can navigate upward through recent history.
const ROW_BUFFER: usize = 512;
const COL_INDEX_W: f32 = 24.0;
const COL_CALL_W: f32 = 86.0;
const COL_BAND_W: f32 = 40.0;
const COL_MODE_W: f32 = 36.0;
const COL_UTC_W: f32 = 42.0;
const COL_VOID_W: f32 = 44.0;
const COL_SPACING: f32 = 8.0;
const HEADER_GAP: f32 = 4.0;
const ROW_GAP: f32 = 2.0;

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
    let cols = compute_columns(size.width.max(320.0));

    let mut row_views: Vec<LogRowView> = Vec::with_capacity(ROW_BUFFER);
    let start = records.len().saturating_sub(ROW_BUFFER);
    let tail = &records[start..];
    for (offset, rec) in tail.iter().enumerate() {
        let i = start + offset;
        let (rx_exchange, tx_exchange) = split_exchange_values(rec);
        row_views.push(LogRowView {
            index: i + 1,
            callsign_norm: rec.callsign_norm.clone(),
            band: band_label(rec),
            mode: mode_label(rec),
            time_utc: format_log_time_utc(rec.ts_ms),
            rx_exchange,
            tx_exchange,
            is_void: rec.flags.is_void,
            newest: offset + 1 == tail.len(),
        });
    }
    let mut rows: Vec<Element<M>> = Vec::with_capacity(row_views.len());
    for row_data in row_views {
        rows.push(log_row(row_data, cols));
    }

    if rows.is_empty() {
        rows.push(
            container(
                text("(no QSOs yet)")
                    .size(style::TEXT_BODY)
                    .style(style::very_muted),
            )
            .padding([8, 10])
            .style(style::card_style)
            .into(),
        );
    }

    container(
        column![
            header_row(cols),
            scrollable(column(rows).spacing(ROW_GAP))
                .anchor_bottom()
                .height(Length::Fill),
        ]
        .spacing(HEADER_GAP),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn log_row<'a, M: 'a>(row_data: LogRowView, cols: LogColumns) -> Element<'a, M> {
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
    container(
        row![
            text(format!("{index:>2}"))
                .size(style::TEXT_TINY)
                .font(Font::MONOSPACE)
                .style(style::muted)
                .width(Length::Fixed(24.0)),
            text(callsign_norm)
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| {
                    if newest {
                        style::accent(t)
                    } else if is_void {
                        iced::widget::text::Style {
                            color: Some(style::danger_color(t)),
                        }
                    } else {
                        style::body(t)
                    }
                })
                .width(Length::Fixed(86.0)),
            container(
                text(rx_exchange)
                    .size(style::TEXT_BODY)
                    .font(Font::MONOSPACE)
                    .style(style::body)
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::Fixed(cols.rx_w)),
            )
            .width(Length::Fixed(cols.rx_w)),
            container(
                text(tx_exchange)
                    .size(style::TEXT_BODY)
                    .font(Font::MONOSPACE)
                    .style(style::body)
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::Fixed(cols.tx_w)),
            )
            .width(Length::Fixed(cols.tx_w)),
            text(band)
                .size(style::TEXT_TINY)
                .font(Font::MONOSPACE)
                .style(style::muted)
                .width(Length::Fixed(COL_BAND_W)),
            text(mode)
                .size(style::TEXT_TINY)
                .font(Font::MONOSPACE)
                .style(style::muted)
                .width(Length::Fixed(COL_MODE_W)),
            text(time_utc)
                .size(style::TEXT_TINY)
                .font(Font::MONOSPACE)
                .style(style::muted)
                .width(Length::Fixed(COL_UTC_W)),
            void_badge(is_void),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center)
        .width(Length::Fill),
    )
    .padding([4, 6])
    .width(Length::Fill)
    .style(move |t: &Theme| {
        if newest {
            style::accent_card_style(t)
        } else {
            style::card_style(t)
        }
    })
    .into()
}

fn header_row<'a, M: 'a>(cols: LogColumns) -> Element<'a, M> {
    row![
        text("#")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_INDEX_W)),
        text("CALL")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_CALL_W)),
        text("RX")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(cols.rx_w)),
        text("TX")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(cols.tx_w)),
        text("BAND")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_BAND_W)),
        text("MODE")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_MODE_W)),
        text("UTC")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_UTC_W)),
        text("")
            .size(style::TEXT_TINY)
            .width(Length::Fixed(COL_VOID_W)),
    ]
    .spacing(COL_SPACING)
    .width(Length::Fill)
    .into()
}

fn void_badge<'a, M: 'a>(is_void: bool) -> Element<'a, M> {
    if !is_void {
        return text("").size(style::TEXT_TINY).width(Length::Shrink).into();
    }
    container(text("VOID").size(style::TEXT_TINY).font(Font::MONOSPACE))
        .padding([2, 6])
        .style(move |t: &Theme| style::badge_style(t, style::dupe_pair(t)))
        .into()
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
        + 7.0 * COL_SPACING;
    let exchange_total = (available_width - fixed).max(140.0);
    let rx_w = (exchange_total * 0.6).max(80.0);
    let tx_w = (exchange_total - rx_w).max(60.0);
    LogColumns { rx_w, tx_w }
}
