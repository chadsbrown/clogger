//! CW preview pane — recent `Effect::CwSend` payloads, captured at dispatch
//! time. Stand-in for the live keyer-echo display until a real keyer
//! adapter exists.

use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Element, Length};
use logger_core::RadioId;

use super::style;

pub fn view<'a, M: 'a>(history: &'a [(RadioId, String)]) -> Element<'a, M> {
    let mut rows: Vec<Element<M>> = Vec::with_capacity(history.len());
    for (radio, line) in history
        .iter()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        rows.push(
            row![
                text(format!("R{}", radio))
                    .size(11)
                    .style(style::accent)
                    .width(Length::Fixed(28.0)),
                text(strip_speed_markers(line)).size(13).style(style::body),
            ]
            .spacing(6)
            .into(),
        );
    }
    if rows.is_empty() {
        rows.push(
            text("(nothing keyed yet — press F1 to send the CQ macro)")
                .size(12)
                .style(style::very_muted)
                .into(),
        );
    }

    container(
        column![
            text("CW preview").size(11).style(style::accent),
            Space::with_height(4),
            scrollable(column(rows).spacing(2))
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Strip `{NN}` WPM-change markers from macro-expanded CW text.
fn strip_speed_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_marker = false;
    for c in s.chars() {
        match c {
            '{' => in_marker = true,
            '}' if in_marker => in_marker = false,
            _ if !in_marker => out.push(c),
            _ => {}
        }
    }
    out
}
