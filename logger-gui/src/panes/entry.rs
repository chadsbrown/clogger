//! Entry pane: R1 and (optionally) R2 exchange fields laid out
//! horizontally per radio, matching the "run box" layout contest
//! operators expect. Each field's pixel width comes from the contest
//! spec's `field.width` (in character columns), translated through the
//! bundled monospace font.
//!
//! A CW-echo line sits directly under each radio's field row, so the
//! operator sees what the keyer is sending for *that* radio inline with
//! the exchange. No separate CW pane is needed.
//!
//! This is where the operator looks most, so it gets the strongest
//! hierarchy in the app:
//! * Frequency is in large mono body text, not muted label text.
//! * Only the active radio's card is accent-bordered and raised, and a
//!   transmitting radio switches to a danger border plus a TX badge.
//! * Alerts (DUPE, MULT) are the only solid badges in the pane.

use std::collections::HashMap;

use iced::widget::{column, container, row, text, Space};
use iced::{Border, Element, Length, Theme};
use logger_core::{AppState, RadioId};

use super::style::{self, Icon};
use super::widgets::{self, Tone};

/// Pixels per character column for the value-box width calculation.
/// JetBrains Mono advances 0.6em, so 9px at `TEXT_VALUE` (15px), plus a
/// little slack so a full-width value never touches the box edge.
const PX_PER_COL: f32 = 9.6;
/// Horizontal space inside the value box beyond the characters: left and
/// right padding plus the caret bar.
const BOX_HPAD: f32 = 20.0;
/// Minimum field width even when the spec says something smaller, so
/// single-character fields (like PREC) still have a visible box.
const MIN_BOX_PX: f32 = 44.0;
/// Height of the caret bar. It matches the value line height.
const CARET_H: f32 = style::TEXT_VALUE * 1.3;
/// Fixed height of the echo and status rows, so badges appearing or
/// clearing never shift the R2 card.
const LINE_H: f32 = 22.0;

pub fn view<'a, M: 'a>(
    state: &'a AppState,
    show_r2: bool,
    echoes: &'a HashMap<RadioId, String>,
    tx_radio: Option<RadioId>,
) -> Element<'a, M> {
    let mut sections: Vec<Element<M>> = Vec::with_capacity(2);
    sections.push(render_radio(
        state,
        1,
        echoes.get(&1).map(|s| s.as_str()),
        tx_radio == Some(1),
    ));
    if show_r2 {
        sections.push(render_radio(
            state,
            2,
            echoes.get(&2).map(|s| s.as_str()),
            tx_radio == Some(2),
        ));
    }
    widgets::pane_body(column(sections).spacing(style::SP_3))
}

fn render_radio<'a, M: 'a>(
    state: &'a AppState,
    radio_id: RadioId,
    echo: Option<&'a str>,
    is_tx: bool,
) -> Element<'a, M> {
    let is_active = state.focused_radio == radio_id;
    let entry = state.entries.get(&radio_id);
    let radio = state.radios.get(&radio_id);

    // --- Header: identity badges on the left, rig state on the right.
    let freq_str = radio
        .map(|r| format!("{:.3}", r.freq_hz as f64 / 1_000_000.0))
        .unwrap_or_else(|| "—".to_string());
    let rig_mode = radio.map(|r| r.mode.clone()).unwrap_or_default();
    let is_cw = rig_mode.eq_ignore_ascii_case("CW");
    let wpm_str = if is_cw {
        let wpm = radio.map(|r| r.cw_speed).unwrap_or(state.default_cw_speed);
        format!("{wpm} WPM")
    } else {
        String::new()
    };
    let op_mode = match entry.map(|e| e.mode) {
        Some(logger_core::OpMode::Sp) => "S&P",
        _ => "RUN",
    };

    let mut badges: Vec<Element<M>> = vec![
        widgets::badge(
            format!("R{radio_id}"),
            if is_active { Tone::Accent } else { Tone::Neutral },
        ),
        widgets::badge(op_mode, Tone::Neutral),
    ];
    if is_tx {
        badges.push(widgets::badge_solid("TX", Tone::Danger));
    }

    let header: Element<M> = row![
        row(badges)
            .spacing(style::SP_2)
            .align_y(iced::alignment::Vertical::Center),
        Space::new().width(Length::Fill),
        text(wpm_str)
            .size(style::TEXT_LABEL)
            .font(style::FONT_MONO)
            .style(style::muted),
        text(rig_mode)
            .size(style::TEXT_LABEL)
            .font(style::FONT_UI_SEMIBOLD)
            .style(style::muted),
        text(freq_str)
            .size(style::TEXT_VALUE)
            .font(style::FONT_MONO)
            .style(move |t: &Theme| {
                if is_active {
                    style::body(t)
                } else {
                    style::muted(t)
                }
            }),
    ]
    .spacing(style::SP_3)
    .align_y(iced::alignment::Vertical::Center)
    .into();

    // --- Field row: one column per field (label above value box).
    let fields_row: Element<M> = if let Some(entry) = entry {
        let mut columns: Vec<Element<M>> = Vec::with_capacity(entry.fields.len());
        for (idx, field) in entry.fields.iter().enumerate() {
            let is_focused_field = is_active && idx == entry.focus;
            let box_width = (field.width as f32 * PX_PER_COL + BOX_HPAD).max(MIN_BOX_PX);
            let empty = field.value.is_empty();

            let label = text(field.label.to_uppercase())
                .size(style::TEXT_TINY)
                .font(style::FONT_UI_SEMIBOLD)
                .style(move |t: &Theme| {
                    if is_focused_field {
                        style::accent(t)
                    } else {
                        style::muted(t)
                    }
                });

            let value_style = move |t: &Theme| container::Style {
                background: Some(
                    if is_focused_field {
                        style::surface_pane(t)
                    } else {
                        style::surface_sunken(t)
                    }
                    .into(),
                ),
                border: Border {
                    color: if is_focused_field {
                        style::focused_border_color(t)
                    } else {
                        style::border_color(t)
                    },
                    width: if is_focused_field { 1.5 } else { 1.0 },
                    radius: style::RADIUS_SM.into(),
                },
                ..container::Style::default()
            };
            let scp_check = field.field_id == 1
                && !field.value.is_empty()
                && entry.scp_matches.contains(&field.value);

            let mk_text = |s: String| {
                text(s)
                    .size(style::TEXT_VALUE)
                    .font(style::FONT_MONO)
                    .style(style::body)
            };
            // The focused field shows a 2px accent caret bar at
            // `field.cursor`. Earlier versions inserted a "|" glyph,
            // which took up a character cell and shifted the text as the
            // caret moved. A thin bar is what operators expect from a
            // text box. Unfocused empty fields show a faint em-dash
            // placeholder.
            let text_part: Element<M> = if is_focused_field {
                let cur = field.cursor.min(field.value.len());
                let (before, after) = field.value.split_at(cur);
                row![
                    mk_text(before.to_string()),
                    caret(),
                    mk_text(after.to_string()),
                ]
                .align_y(iced::alignment::Vertical::Center)
                .into()
            } else if empty {
                text("—")
                    .size(style::TEXT_VALUE)
                    .font(style::FONT_MONO)
                    .style(style::very_muted)
                    .into()
            } else {
                mk_text(field.value.clone()).into()
            };
            let inner: Element<M> = if scp_check {
                row![
                    text_part,
                    Space::new().width(Length::Fill),
                    style::icon(Icon::Check, style::TEXT_BODY).style(|t: &Theme| {
                        text::Style {
                            color: Some(style::new_spot_color(t)),
                        }
                    }),
                ]
                .align_y(iced::alignment::Vertical::Center)
                .into()
            } else {
                text_part
            };
            let value = container(inner)
                .padding([4.0, 8.0])
                .width(Length::Fixed(box_width))
                .style(value_style);

            columns.push(
                column![label, value]
                    .spacing(style::SP_1 + 1.0)
                    .width(Length::Fixed(box_width))
                    .into(),
            );
        }
        row(columns)
            .spacing(style::SP_3)
            .align_y(iced::alignment::Vertical::Bottom)
            .into()
    } else {
        widgets::empty_state("No entry state")
    };

    // --- Status row: alerts and assigned serial.
    let mut status_bits: Vec<Element<M>> = Vec::new();
    if let Some(entry) = entry {
        if entry.is_dupe {
            status_bits.push(widgets::badge_solid("DUPE", Tone::Danger));
        }
        if entry.is_new_mult {
            status_bits.push(widgets::badge_solid("MULT", Tone::Warning));
        }
        if entry.is_passband_qrm {
            status_bits.push(widgets::badge("QRM", Tone::Danger));
        }
        if let Some(sn) = entry.assigned_serial {
            status_bits.push(
                row![
                    text("SERIAL")
                        .size(style::TEXT_TINY)
                        .font(style::FONT_UI_SEMIBOLD)
                        .style(style::muted),
                    text(sn.to_string())
                        .size(style::TEXT_LABEL)
                        .font(style::FONT_MONO)
                        .style(style::body),
                ]
                .spacing(style::SP_2)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            );
        }
    }

    // Fixed height so badges appearing or clearing on R1 never push R2
    // up and down while the operator types.
    let status_row = container(
        row(status_bits)
            .spacing(style::SP_3)
            .align_y(iced::alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(LINE_H))
    .align_y(iced::alignment::Vertical::Center);

    let body = column![
        header,
        fields_row,
        echo_line_view(echo),
        status_row,
    ]
    .spacing(style::SP_3);

    container(body)
        .padding([style::SP_3 + 2.0, style::SP_4])
        .width(Length::Fill)
        .style(move |t: &Theme| container::Style {
            background: Some(
                if is_active {
                    style::surface_raised(t)
                } else {
                    style::surface_pane(t)
                }
                .into(),
            ),
            border: Border {
                color: if is_tx {
                    style::danger_color(t)
                } else if is_active {
                    style::focused_border_color(t)
                } else {
                    style::border_color(t)
                },
                width: if is_tx || is_active { 1.5 } else { 1.0 },
                radius: style::RADIUS_MD.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The text caret: a 2px accent bar.
fn caret<'a, M: 'a>() -> Element<'a, M> {
    container(Space::new().width(2).height(CARET_H))
        .width(2)
        .height(CARET_H)
        .style(|t: &Theme| container::Style {
            background: Some(style::accent_color(t).into()),
            border: Border {
                radius: 1.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// One-line inline CW-echo display for a specific radio: plain centered
/// text with no box, sitting on the card surface so the operator's eye
/// doesn't have to chase a separate region. Fixed height so the status
/// row below doesn't jump when the echo appears or clears.
fn echo_line_view<'a, M: 'a>(echo: Option<&'a str>) -> Element<'a, M> {
    let body = echo
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();

    container(
        text(body)
            .size(style::TEXT_VALUE)
            .font(style::FONT_MONO)
            .style(style::accent),
    )
    .width(Length::Fill)
    .height(Length::Fixed(LINE_H))
    .center_x(Length::Fill)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
