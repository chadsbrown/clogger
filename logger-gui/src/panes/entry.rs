//! Entry pane — shows the exchange fields for R1 and (optionally) R2 so
//! SO2R operators can see both simultaneously. Plus SCP (super-check-partial)
//! suggestions under each radio's call field so partial callsigns get
//! completed the way every real contest logger does it.

use iced::widget::{column, container, row, text, Space};
use iced::{Border, Color, Element, Length, Theme};
use logger_core::{AppState, RadioId};

use super::style;

pub fn view<'a, M: 'a>(state: &'a AppState, show_r2: bool) -> Element<'a, M> {
    let mut sections: Vec<Element<M>> = Vec::with_capacity(2);
    sections.push(render_radio(state, 1));
    if show_r2 {
        sections.push(render_radio(state, 2));
    }
    container(column(sections).spacing(8))
        .padding(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn render_radio<'a, M: 'a>(state: &'a AppState, radio_id: RadioId) -> Element<'a, M> {
    let is_active = state.focused_radio == radio_id;
    let entry = state.entries.get(&radio_id);
    let radio = state.radios.get(&radio_id);

    let header_line = {
        let freq_str = radio
            .map(|r| format!("{:.3} MHz {}", r.freq_hz as f64 / 1_000_000.0, r.mode))
            .unwrap_or_else(|| "—".to_string());
        let mode_str = entry
            .map(|e| format!("{:?}", e.mode))
            .unwrap_or_else(|| "Run".to_string());
        let wpm = radio
            .map(|r| r.cw_speed)
            .unwrap_or(state.default_cw_speed);
        format!("R{radio_id}  •  {mode_str}  •  {wpm} WPM  •  {freq_str}")
    };
    let header = text(header_line).size(11).style(move |t: &Theme| {
        if is_active {
            style::accent(t)
        } else {
            style::very_muted(t)
        }
    });

    let mut field_rows: Vec<Element<M>> = Vec::new();
    if let Some(entry) = entry {
        for (idx, field) in entry.fields.iter().enumerate() {
            let is_focused_field = is_active && idx == entry.focus;
            let value_str = if field.value.is_empty() {
                "—".to_string()
            } else {
                field.value.clone()
            };
            let label = text(format!("{:>6}", field.label))
                .size(13)
                .style(style::muted);
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
            let value = container(text(value_str).size(14))
                .padding([2, 6])
                .width(Length::Fill)
                .style(value_style);
            field_rows.push(
                row![label, value]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center)
                    .into(),
            );
        }
    } else {
        field_rows.push(text("(no entry state)").style(style::muted).into());
    }

    let mut status_bits: Vec<Element<M>> = Vec::new();
    if let Some(entry) = entry {
        if entry.is_dupe {
            status_bits.push(badge(" DUPE ", style::DUPE_BADGE).into());
        }
        if entry.is_new_mult {
            status_bits.push(badge(" MULT ", style::MULT_BADGE).into());
        }
        if entry.is_passband_qrm {
            status_bits.push(badge(" QRM ", style::QRM_BADGE).into());
        }
        if let Some(sn) = entry.assigned_serial {
            status_bits.push(text(format!("# {sn}")).size(12).style(style::body).into());
        }
    }

    let status_row = row(status_bits)
        .spacing(6)
        .align_y(iced::alignment::Vertical::Center);

    // SCP suggestions — partial-callsign autocompletion from the master
    // database. Shows up to 10 suggestions when the call field has content
    // and produces matches. This is contest logger table stakes.
    let scp_row = entry
        .filter(|e| !e.scp_matches.is_empty())
        .map(|e| scp_row_view(&e.scp_matches));

    let mut body_children: Vec<Element<M>> = vec![
        header.into(),
        Space::with_height(4).into(),
        column(field_rows).spacing(4).into(),
        Space::with_height(4).into(),
        status_row.into(),
    ];
    if let Some(scp) = scp_row {
        body_children.push(Space::with_height(4).into());
        body_children.push(scp);
    }

    let body = column(body_children);

    container(body)
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

fn scp_row_view<'a, M: 'a>(matches: &'a [String]) -> Element<'a, M> {
    let take = matches.len().min(10);
    let mut chips: Vec<Element<M>> = Vec::with_capacity(take + 1);
    chips.push(text("SCP").size(10).style(style::muted).into());
    for m in matches.iter().take(take) {
        chips.push(
            container(text(m.clone()).size(12).style(style::accent))
                .padding([1, 5])
                .style(|t: &Theme| container::Style {
                    background: Some(t.extended_palette().primary.weak.color.into()),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 2.0.into(),
                    },
                    ..container::Style::default()
                })
                .into(),
        );
    }
    row(chips).spacing(4).wrap().into()
}

fn badge<'a, M: 'a>(label: &'static str, bg: Color) -> Element<'a, M> {
    container(text(label).size(11).color(Color::WHITE))
        .padding([1, 4])
        .style(move |_t: &Theme| container::Style {
            background: Some(bg.into()),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 2.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}
