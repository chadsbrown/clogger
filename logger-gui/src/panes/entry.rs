//! Entry pane — R1 and (optionally) R2 exchange fields laid out horizontally
//! per radio, matching the "run box" layout contest operators expect. Each
//! field's pixel width comes from the contest spec's `field.width` (in
//! character columns), translated through a monospace font.
//!
//! A small CW-echo line sits directly under each radio's field row so the
//! operator sees what the keyer is keying for *that* radio, inline with
//! the exchange — no separate CW pane needed.

use std::collections::HashMap;

use iced::widget::{column, container, row, text, Space};
use iced::{Border, Color, Element, Font, Length, Theme};
use logger_core::{AppState, RadioId};

use super::style;

/// Pixels per character-column for the value-box width calculation.
/// Picked to look roughly right at value text size 14 in the default
/// monospace font.
const PX_PER_COL: f32 = 11.0;
/// Extra horizontal padding inside the value box (left + right combined).
const BOX_HPAD: f32 = 12.0;
/// Minimum field width even if the spec says something smaller, so
/// single-char fields (like PREC) still have a visible box.
const MIN_BOX_PX: f32 = 42.0;

pub fn view<'a, M: 'a>(
    state: &'a AppState,
    show_r2: bool,
    echoes: &'a HashMap<RadioId, String>,
) -> Element<'a, M> {
    let mut sections: Vec<Element<M>> = Vec::with_capacity(2);
    sections.push(render_radio(state, 1, echoes.get(&1).map(|s| s.as_str())));
    if show_r2 {
        sections.push(render_radio(state, 2, echoes.get(&2).map(|s| s.as_str())));
    }
    container(column(sections).spacing(8))
        .padding(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn render_radio<'a, M: 'a>(
    state: &'a AppState,
    radio_id: RadioId,
    echo: Option<&'a str>,
) -> Element<'a, M> {
    let is_active = state.focused_radio == radio_id;
    let entry = state.entries.get(&radio_id);
    let radio = state.radios.get(&radio_id);

    let header_line = {
        let freq_str = radio
            .map(|r| format!("{:.3} MHz {}", r.freq_hz as f64 / 1_000_000.0, r.mode))
            .unwrap_or_else(|| "—".to_string());
        let mode_str = match entry.map(|e| e.mode) {
            Some(logger_core::OpMode::Sp) => "S&P",
            _ => "RUN",
        };
        let is_cw = radio.map(|r| r.mode.eq_ignore_ascii_case("CW")).unwrap_or(false);
        let wpm_str = if is_cw {
            let wpm = radio
                .map(|r| r.cw_speed)
                .unwrap_or(state.default_cw_speed);
            format!("  •  {wpm} WPM")
        } else {
            String::new()
        };
        format!("R{radio_id}  •  {mode_str}{wpm_str}  •  {freq_str}")
    };
    let header = text(header_line).size(11.0).style(move |t: &Theme| {
        if is_active {
            style::accent(t)
        } else {
            style::very_muted(t)
        }
    });

    // Horizontal field row — one column per field (label above value box).
    let fields_row: Element<M> = if let Some(entry) = entry {
        let mut columns: Vec<Element<M>> = Vec::with_capacity(entry.fields.len());
        for (idx, field) in entry.fields.iter().enumerate() {
            let is_focused_field = is_active && idx == entry.focus;
            let box_width = (field.width as f32 * PX_PER_COL + BOX_HPAD).max(MIN_BOX_PX);

            let label = text(field.label.clone()).size(10.0).style(style::muted);

            let empty = field.value.is_empty();
            let value_style = move |t: &Theme| container::Style {
                background: Some(
                    if is_focused_field {
                        t.extended_palette().primary.weak.color
                    } else {
                        t.extended_palette().background.base.color
                    }
                    .into(),
                ),
                text_color: Some(if empty && !is_focused_field {
                    style::very_muted_color(t)
                } else if is_focused_field {
                    t.extended_palette().primary.weak.text
                } else {
                    style::text_color(t)
                }),
                border: Border {
                    color: if is_focused_field {
                        style::focused_border_color(t)
                    } else {
                        style::border_color(t)
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..container::Style::default()
            };
            let scp_check = field.field_id == 1
                && !field.value.is_empty()
                && entry.scp_matches.contains(&field.value);

            // In the focused field, split the value at field.cursor and
            // insert a visible caret glyph so the operator can see where
            // typing will land. Unfocused fields render the value verbatim
            // (or an em-dash placeholder when empty).
            let mk_text = |s: String| {
                text(s)
                    .size(14.0)
                    .font(Font::MONOSPACE)
                    .style(style::body)
            };
            let text_part: Element<M> = if is_focused_field {
                let cur = field.cursor.min(field.value.len());
                let (before, after) = field.value.split_at(cur);
                row![
                    mk_text(before.to_string()),
                    text("|")
                        .size(14.0)
                        .font(Font::MONOSPACE)
                        .style(|t: &Theme| iced::widget::text::Style {
                            color: Some(style::accent_color(t)),
                        }),
                    mk_text(after.to_string()),
                ]
                .spacing(0)
                .into()
            } else if empty {
                mk_text("—".to_string()).into()
            } else {
                mk_text(field.value.clone()).into()
            };
            let inner: Element<M> = if scp_check {
                row![
                    text_part,
                    Space::new().width(Length::Fill),
                    text("\u{2713}")
                        .size(14.0)
                        .font(Font {
                            weight: iced::font::Weight::Bold,
                            ..Font::default()
                        })
                        .style(|t: &Theme| iced::widget::text::Style {
                            color: Some(style::success_color(t)),
                        }),
                ]
                .align_y(iced::alignment::Vertical::Center)
                .into()
            } else {
                text_part
            };
            let value = container(inner)
                .padding([3, 6])
                .width(Length::Fixed(box_width))
                .style(value_style);

            columns.push(
                column![label, value]
                    .spacing(2)
                    .width(Length::Fixed(box_width))
                    .into(),
            );
        }
        row(columns)
            .spacing(8)
            .align_y(iced::alignment::Vertical::Bottom)
            .into()
    } else {
        text("(no entry state)").style(style::muted).into()
    };

    let mut status_bits: Vec<Element<M>> = Vec::new();
    if let Some(entry) = entry {
        if entry.is_dupe {
            status_bits.push(badge(" DUPE ", style::dupe_badge).into());
        }
        if entry.is_new_mult {
            status_bits.push(badge(" MULT ", style::mult_color).into());
        }
        if entry.is_passband_qrm {
            status_bits.push(badge(" QRM ", style::qrm_badge).into());
        }
        if let Some(sn) = entry.assigned_serial {
            status_bits.push(text(format!("# {sn}")).size(12.0).style(style::body).into());
        }
    }

    // Status row needs a fixed-height container — otherwise appearance/
    // absence of badges on R1 would push R2 up and down as the operator
    // types. SCP suggestions live in their own pane now (panes::scp).
    let status_row = container(
        row(status_bits)
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(22.0));

    let echo_row = echo_line_view(echo);

    let body_children: Vec<Element<M>> = vec![
        header.into(),
        Space::new().height(4).into(),
        fields_row,
        Space::new().height(4).into(),
        echo_row,
        Space::new().height(4).into(),
        status_row.into(),
    ];

    container(column(body_children))
        .padding(6)
        .width(Length::Fill)
        .style(move |t: &Theme| container::Style {
            border: Border {
                color: if is_active {
                    style::focused_border_color(t)
                } else {
                    style::border_color(t)
                },
                width: if is_active { 1.5 } else { 1.0 },
                radius: 3.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// One-line inline CW-echo display for a specific radio. Plain text,
/// horizontally centered, no box and no background change — just sits in
/// the entry's normal surface so the operator's eye doesn't have to chase
/// a separate region. Fixed height so the status row below doesn't jump
/// when echo appears/clears.
fn echo_line_view<'a, M: 'a>(echo: Option<&'a str>) -> Element<'a, M> {
    let body = echo
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();

    container(
        text(body)
            .size(style::TEXT_VALUE)
            .font(Font::MONOSPACE)
            .style(style::body),
    )
    .padding([3, 8])
    .width(Length::Fill)
    .height(Length::Fixed(24.0))
    .center_x(Length::Fill)
    .into()
}

fn badge<'a, M: 'a>(label: &'static str, bg: fn(&Theme) -> Color) -> Element<'a, M> {
    container(text(label).size(11.0).color(Color::WHITE))
        .padding([1, 4])
        .style(move |t: &Theme| container::Style {
            background: Some(bg(t).into()),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 2.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}
