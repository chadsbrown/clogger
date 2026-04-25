//! DX feed activity pane — recent spot add/remove events so the operator
//! can see cluster traffic is still flowing.

use std::collections::VecDeque;

use iced::widget::{column, container, responsive, row, text};
use iced::{Element, Font, Length, Size, Theme};

use super::style;

const COL_KIND_W: f32 = 42.0;
const COL_TIME_W: f32 = 64.0;
const COL_CALL_W: f32 = 86.0;
const COL_FREQ_W: f32 = 72.0;
const COL_MODE_W: f32 = 40.0;
const ROW_GAP: f32 = 2.0;
const HEADER_GAP: f32 = 4.0;
const PANE_PADDING_Y: f32 = 16.0;
const HEADER_H: f32 = 12.0;
const ROW_H: f32 = 16.0;

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
    for item in items.iter().skip(start) {
        rows.push(activity_row(item));
    }

    let body: Element<M> = if rows.is_empty() {
        text("(no dxfeed activity yet)")
            .size(style::TEXT_BODY)
            .style(style::very_muted)
            .into()
    } else {
        column(rows).spacing(ROW_GAP).into()
    };

    container(column![header_row(), body].spacing(HEADER_GAP))
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn header_row<'a, M: 'a>() -> Element<'a, M> {
    row![
        text("")
            .size(style::TEXT_TINY)
            .width(Length::Fixed(COL_KIND_W)),
        text("UTC")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_TIME_W)),
        text("CALL")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_CALL_W)),
        text("FREQ")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_FREQ_W)),
        text("MODE")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_MODE_W)),
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}

fn activity_row<'a, M: 'a>(item: &'a DxActivityItem) -> Element<'a, M> {
    row![
            text(match item.kind {
                DxActivityKind::Spot => "SPOT",
                DxActivityKind::Withdraw => "DROP",
            })
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(move |t: &Theme| match item.kind {
                DxActivityKind::Spot => style::accent(t),
                DxActivityKind::Withdraw => style::very_muted(t),
            })
            .width(Length::Fixed(COL_KIND_W)),
            text(format_time_utc(item.ts_ms))
                .size(style::TEXT_TINY)
                .font(Font::MONOSPACE)
                .style(style::muted)
                .width(Length::Fixed(COL_TIME_W)),
            text(item.call.clone())
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| match item.kind {
                    DxActivityKind::Spot => style::accent(t),
                    DxActivityKind::Withdraw => style::body(t),
                })
                .width(Length::Fixed(COL_CALL_W)),
            text(format_freq(item.freq_hz))
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(style::body)
                .width(Length::Fixed(COL_FREQ_W)),
            text(item.mode.as_deref().unwrap_or("—"))
                .size(style::TEXT_TINY)
                .font(Font::MONOSPACE)
                .style(style::muted)
                .width(Length::Fixed(COL_MODE_W)),
        ]
    .spacing(8)
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
