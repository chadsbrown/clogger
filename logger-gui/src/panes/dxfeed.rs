//! DX feed activity pane: recent spot add/remove events, so the operator
//! can see that cluster traffic is still flowing. It uses the shared
//! flat-table look (`widgets::table_row`).

use std::collections::VecDeque;

use iced::widget::{column, responsive, row, text};
use iced::{Element, Length, Size, Theme};

use super::style;
use super::widgets::{self, Tone};

const COL_KIND_W: f32 = 48.0;
const COL_TIME_W: f32 = 64.0;
const COL_CALL_W: f32 = 86.0;
const COL_FREQ_W: f32 = 72.0;
const COL_MODE_W: f32 = 40.0;
const COL_SPACING: f32 = style::SP_3;
const HEADER_GAP: f32 = style::SP_1;
// Row-count estimate used to show only the rows that fit. The values
// track the shared table metrics: a row is PAD_ROW plus one 13px mono
// line, and the header is an 11px label, a 4px pad and a hairline. They
// are rounded up so the last row is never clipped.
const PANE_PADDING_Y: f32 = style::PAD_PANE.top + style::PAD_PANE.bottom;
const HEADER_H: f32 = 20.0;
const ROW_H: f32 = 24.0;
const ROW_GAP: f32 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxActivityKind {
    Spot,
    Withdraw,
}

#[derive(Debug, Clone)]
pub struct DxActivityItem {
    pub ts_ms: i64,
    pub kind: DxActivityKind,
    pub call: String,
    pub freq_hz: Option<u64>,
    pub mode: Option<String>,
}

pub fn view<'a, M: 'a>(items: &'a VecDeque<DxActivityItem>) -> Element<'a, M> {
    responsive(move |size| view_for_size(items, size))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_for_size<'a, M: 'a>(items: &'a VecDeque<DxActivityItem>, size: Size) -> Element<'a, M> {
    let visible_rows = visible_row_count(size.height);
    let start = items.len().saturating_sub(visible_rows);

    let mut rows: Vec<Element<M>> = Vec::with_capacity(visible_rows.max(1));
    for (i, item) in items.iter().enumerate().skip(start) {
        rows.push(widgets::table_row(activity_cells(item), i % 2 == 1, None));
    }

    let body: Element<M> = if rows.is_empty() {
        widgets::empty_state("No DX feed activity yet")
    } else {
        column(rows).into()
    };

    widgets::pane_body(column![header_row(), body].spacing(HEADER_GAP))
}

fn header_row<'a, M: 'a>() -> Element<'a, M> {
    widgets::table_header(
        vec![
            widgets::col_header("", Length::Fixed(COL_KIND_W)),
            widgets::col_header("UTC", Length::Fixed(COL_TIME_W)),
            widgets::col_header("Call", Length::Fixed(COL_CALL_W)),
            widgets::col_header("Freq", Length::Fixed(COL_FREQ_W)),
            widgets::col_header("Mode", Length::Fixed(COL_MODE_W)),
        ],
        COL_SPACING,
    )
}

/// New spots get a soft accent SPOT badge and accent call. Drops get a
/// neutral DROP badge and plain text, so cluster churn reads at a glance.
fn activity_cells<'a, M: 'a>(item: &'a DxActivityItem) -> Element<'a, M> {
    let is_spot = item.kind == DxActivityKind::Spot;
    row![
        iced::widget::container(match item.kind {
            DxActivityKind::Spot => widgets::badge("SPOT", Tone::Accent),
            DxActivityKind::Withdraw => widgets::badge("DROP", Tone::Neutral),
        })
        .width(Length::Fixed(COL_KIND_W)),
        text(format_time_utc(item.ts_ms))
            .size(style::TEXT_LABEL)
            .font(style::FONT_MONO)
            .style(style::muted)
            .width(Length::Fixed(COL_TIME_W)),
        text(item.call.clone())
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO_BOLD)
            .style(move |t: &Theme| if is_spot { style::accent(t) } else { style::muted(t) })
            .width(Length::Fixed(COL_CALL_W)),
        text(format_freq(item.freq_hz))
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO)
            .style(style::body)
            .width(Length::Fixed(COL_FREQ_W)),
        text(item.mode.as_deref().unwrap_or("—"))
            .size(style::TEXT_LABEL)
            .font(style::FONT_MONO)
            .style(style::muted)
            .width(Length::Fixed(COL_MODE_W)),
    ]
    .spacing(COL_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}

fn visible_row_count(height: f32) -> usize {
    let usable = (height - PANE_PADDING_Y - HEADER_H - HEADER_GAP).max(ROW_H);
    let row_stride = ROW_H + ROW_GAP;
    ((usable + ROW_GAP) / row_stride).floor().max(1.0) as usize
}

fn format_time_utc(ts_ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ts_ms)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn format_freq(freq_hz: Option<u64>) -> String {
    freq_hz
        .map(|hz| format!("{:.3}", hz as f64 / 1_000_000.0))
        .unwrap_or_else(|| "—".to_string())
}
