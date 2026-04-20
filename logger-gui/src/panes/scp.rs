//! SCP (Super-Check-Partial) pane — partial-callsign suggestions for the
//! focused radio's entry. Shows both exact partial matches (`scp_matches`)
//! and one-character-off matches (`scp_n1_matches`) in separate sections,
//! with the currently-cycled match (if `scp_cycle_index.is_some()`)
//! visually highlighted.
//!
//! Cycling: pressing `=` in the entry pane advances through `scp_matches`
//! one at a time, replacing the CALL field. The reducer drives that;
//! this pane just renders the current state.

use iced::widget::{column, container, row, text, Space};
use iced::{Border, Color, Element, Font, Length, Theme};
use logger_core::AppState;

use super::style;

pub fn view<'a, M: 'a>(state: &'a AppState) -> Element<'a, M> {
    let focused = state.focused_radio;
    let entry = state.entries.get(&focused);

    // When the operator hasn't entered a partial callsign, don't render
    // anything — the pane's purpose is self-evident, so header/placeholder
    // text just adds noise. The pane frame itself still occupies its slot
    // in the layout; we just leave the body empty.
    let Some(entry) = entry else {
        return wrap(Space::new().width(Length::Fill).height(Length::Fill));
    };

    let exact_count = entry.scp_matches.len();
    let n1_count = entry.scp_n1_matches.len();

    if exact_count == 0 && n1_count == 0 {
        return wrap(Space::new().width(Length::Fill).height(Length::Fill));
    }

    let header = text(format!("SCP — R{focused}"))
        .size(style::TEXT_LABEL)
        .style(style::accent);
    let mut sections: Vec<Element<M>> = vec![header.into(), Space::new().height(6).into()];

    if !entry.scp_matches.is_empty() {
        sections.push(
            text(format!("Matches ({exact_count})"))
                .size(style::TEXT_TINY)
                .style(style::muted)
                .into(),
        );
        sections.push(Space::new().height(2).into());
        sections.push(chip_row(&entry.scp_matches, entry.scp_cycle_index, true));
    }

    if !entry.scp_n1_matches.is_empty() {
        if !entry.scp_matches.is_empty() {
            sections.push(Space::new().height(8).into());
        }
        sections.push(
            text(format!("Off-by-one ({n1_count})"))
                .size(style::TEXT_TINY)
                .style(style::muted)
                .into(),
        );
        sections.push(Space::new().height(2).into());
        sections.push(chip_row(&entry.scp_n1_matches, None, false));
    }

    if entry.scp_cycle_index.is_some() {
        sections.push(Space::new().height(8).into());
        sections.push(
            text("Press = to cycle")
                .size(style::TEXT_TINY)
                .style(style::very_muted)
                .into(),
        );
    }

    wrap(column(sections))
}

fn wrap<'a, M: 'a>(body: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(body.into())
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// `cycle_idx` highlights the currently-selected match (only meaningful for
/// the exact-matches column; off-by-one matches don't participate in the
/// `=` cycle). `is_exact` controls the chip color tone.
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
    row(chips).spacing(4).wrap().into()
}

fn chip<'a, M: 'a>(label: &'a str, is_selected: bool, is_exact: bool) -> Element<'a, M> {
    container(
        text(label.to_string())
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(move |t: &Theme| {
                let pal = t.extended_palette();
                let color = if is_selected {
                    pal.primary.strong.text
                } else if is_exact {
                    style::accent_text_color(t)
                } else {
                    style::muted_color(t)
                };
                iced::widget::text::Style { color: Some(color) }
            }),
    )
    .padding([2, 6])
    .style(move |t: &Theme| {
        let pal = t.extended_palette();
        let bg = if is_selected {
            pal.primary.strong.color
        } else if is_exact {
            pal.primary.weak.color
        } else {
            pal.background.strong.color
        };
        container::Style {
            background: Some(bg.into()),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: style::RADIUS_CHIP.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}
