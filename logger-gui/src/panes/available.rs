//! Available pane — per-band counts of unworked spotted callsigns and
//! still-needed multipliers.

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};
use logger_runtime::AvailSummary;

use super::style;

pub fn view<'a, M: 'a>(info: &'a AvailSummary) -> Element<'a, M> {
    let max_qsos = info.by_band.iter().map(|(_, qsos, _)| *qsos).max().unwrap_or(0);
    let max_mults = info.by_band.iter().map(|(_, _, mults)| *mults).max().unwrap_or(0);

    let mut rows: Vec<Element<M>> = Vec::new();
    for (band, qsos, mults) in &info.by_band {
        if *qsos == 0 && *mults == 0 {
            continue;
        }
        rows.push(available_row(
            band,
            *qsos,
            *mults,
            *qsos == max_qsos && max_qsos > 0,
            *mults == max_mults && max_mults > 0,
        ));
    }
    if rows.is_empty() {
        rows.push(
            container(
                text("(no available spots)")
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
            row![
                summary_chip("Calls", info.total_qsos, false),
                summary_chip("Mults", info.total_mults, true),
            ]
            .spacing(8),
            text("Best switch targets")
                .size(style::TEXT_TINY)
                .style(style::muted),
            column(rows).spacing(2),
            total_line(info.total_qsos, info.total_mults),
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn summary_chip<'a, M: 'a>(label: &'static str, value: u32, emphasize: bool) -> Element<'a, M> {
    container(
        row![
            text(label).size(style::TEXT_TINY).style(move |t: &Theme| {
                if emphasize {
                    style::accent(t)
                } else {
                    style::muted(t)
                }
            }),
            text(value.to_string())
                .size(style::TEXT_VALUE)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| {
                    if emphasize {
                        iced::widget::text::Style {
                            color: Some(style::accent_text_color(t)),
                        }
                    } else {
                        style::body(t)
                    }
                }),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([6, 10])
    .width(Length::FillPortion(1))
    .style(style::card_style)
    .into()
}

fn available_row<'a, M: 'a>(
    band: &str,
    qsos: u32,
    mults: u32,
    qso_leader: bool,
    mult_leader: bool,
) -> Element<'a, M> {
    container(
        row![
            text(band.to_string())
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| {
                    if qso_leader || mult_leader {
                        style::accent(t)
                    } else {
                        style::body(t)
                    }
                })
                .width(Length::Fixed(48.0)),
            text(format!("{qsos} call{}", if qsos == 1 { "" } else { "s" }))
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| {
                    if qso_leader {
                        iced::widget::text::Style {
                            color: Some(style::accent_text_color(t)),
                        }
                    } else {
                        style::body(t)
                    }
                })
                .width(Length::FillPortion(2)),
            mult_badge(mults),
            leader_badges(qso_leader, mult_leader),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([6, 8])
    .style(move |t: &Theme| {
        let mut card = style::card_style(t);
        if qso_leader || mult_leader {
            card.border.color = style::focused_border_color(t);
            card.border.width = 1.2;
        }
        card
    })
    .into()
}

fn mult_badge<'a, M: 'a>(mults: u32) -> Element<'a, M> {
    let label = if mults == 0 {
        "No mult".to_string()
    } else if mults == 1 {
        "1 mult".to_string()
    } else {
        format!("{mults} mults")
    };

    container(
        text(label)
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE),
    )
    .padding([3, 8])
    .style(move |t: &Theme| {
        if mults > 0 {
            style::card_style(t)
        } else {
            style::muted_badge_style(t)
        }
    })
    .into()
}

fn leader_badges<'a, M: 'a>(qso_leader: bool, mult_leader: bool) -> Element<'a, M> {
    let mut bits: Vec<Element<M>> = Vec::new();
    if qso_leader {
        bits.push(
            container(text("Q").size(style::TEXT_TINY).font(Font::MONOSPACE))
                .padding([2, 5])
                .style(style::muted_badge_style)
                .into(),
        );
    }
    if mult_leader {
        bits.push(
            container(text("M").size(style::TEXT_TINY).font(Font::MONOSPACE))
                .padding([2, 5])
                .style(style::muted_badge_style)
                .into(),
        );
    }
    row(bits).spacing(4).width(Length::Shrink).into()
}

fn total_line<'a, M: 'a>(qsos: u32, mults: u32) -> Element<'a, M> {
    container(
        row![
            text("TOTAL").size(style::TEXT_TINY).style(style::muted),
            text(format!("{qsos} calls"))
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(style::body),
            text(format!("{mults} mults"))
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| iced::widget::text::Style {
                    color: Some(style::mult_color(t)),
                }),
        ]
        .spacing(10)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(style::card_style)
    .into()
}
