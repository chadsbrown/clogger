//! SCP (Super-Check-Partial) pane — partial-callsign suggestions for the
//! focused radio's entry. Shows both exact partial matches (`scp_matches`)
//! and one-character-off matches (`scp_n1_matches`) in separate sections,
//! with the currently-cycled match (if `scp_cycle_index.is_some()`)
//! visually highlighted.
//!
//! Cycling: pressing `=` in the entry pane advances through `scp_matches`
//! one at a time, replacing the CALL field. The reducer drives that;
//! this pane just renders the current state.

use iced::widget::{column, row, text, Space};
use iced::{Element, Length, Theme};
use logger_core::AppState;

use super::style;
use super::widgets::{self, Tone};

pub fn view<'a, M: 'a>(state: &'a AppState) -> Element<'a, M> {
    let focused = state.focused_radio;

    // When the operator hasn't entered a partial callsign, render
    // nothing. The pane's purpose is self-evident, so header or
    // placeholder text would only add noise. The pane frame still holds
    // its slot in the layout; only the body is empty.
    let Some(entry) = state.entries.get(&focused) else {
        return widgets::pane_body(Space::new().width(Length::Fill).height(Length::Fill));
    };

    let exact_count = entry.scp_matches.len();
    let n1_count = entry.scp_n1_matches.len();

    if exact_count == 0 && n1_count == 0 {
        return widgets::pane_body(Space::new().width(Length::Fill).height(Length::Fill));
    }

    let mut sections: Vec<Element<M>> = Vec::new();

    if !entry.scp_matches.is_empty() {
        sections.push(
            column![
                widgets::section_label(&format!("Matches · R{focused} · {exact_count}")),
                chip_row(&entry.scp_matches, entry.scp_cycle_index, true),
            ]
            .spacing(style::SP_2)
            .into(),
        );
    }

    if !entry.scp_n1_matches.is_empty() {
        sections.push(
            column![
                widgets::section_label(&format!("Off by one · {n1_count}")),
                chip_row(&entry.scp_n1_matches, None, false),
            ]
            .spacing(style::SP_2)
            .into(),
        );
    }

    if entry.scp_cycle_index.is_some() {
        sections.push(
            row![
                widgets::keycap("=", None),
                text("cycles matches").size(style::TEXT_TINY).style(style::very_muted),
            ]
            .spacing(style::SP_2)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
        );
    }

    widgets::pane_body(column(sections).spacing(style::SP_4))
}

/// `cycle_idx` highlights the currently selected match. It only applies
/// to exact matches, since off-by-one matches don't take part in the `=`
/// cycle. Exact matches use soft accent chips, off-by-one matches use
/// neutral chips, and the selected match is a solid accent chip.
fn chip_row<'a, M: 'a>(
    matches: &'a [String],
    cycle_idx: Option<usize>,
    is_exact: bool,
) -> Element<'a, M> {
    let mut chips: Vec<Element<M>> = Vec::with_capacity(matches.len());
    for (i, m) in matches.iter().enumerate() {
        let is_selected = is_exact && cycle_idx == Some(i);
        chips.push(chip(m, is_selected, is_exact));
    }
    row(chips).spacing(style::SP_2).wrap().into()
}

fn chip<'a, M: 'a>(label: &'a str, is_selected: bool, is_exact: bool) -> Element<'a, M> {
    let tone = if is_exact { Tone::Accent } else { Tone::Neutral };
    iced::widget::container(
        text(label.to_string())
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO)
            .style(move |t: &Theme| text::Style {
                color: Some(if is_selected {
                    t.extended_palette().primary.base.text
                } else if is_exact {
                    widgets::tone_text(t, tone)
                } else {
                    style::muted_color(t)
                }),
            }),
    )
    .padding([style::SP_1, style::SP_3 - 2.0])
    .style(move |t: &Theme| iced::widget::container::Style {
        background: Some(
            if is_selected {
                style::accent_color(t)
            } else if is_exact {
                style::lerp(style::surface_pane(t), style::accent_color(t), 0.16)
            } else {
                style::lerp(style::surface_pane(t), style::text_color(t), 0.08)
            }
            .into(),
        ),
        border: iced::Border {
            radius: style::RADIUS_SM.into(),
            ..iced::Border::default()
        },
        ..iced::widget::container::Style::default()
    })
    .into()
}
