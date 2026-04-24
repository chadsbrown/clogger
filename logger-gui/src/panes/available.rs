//! Available pane — per-band counts of unworked spotted callsigns and
//! still-needed multipliers.

use iced::widget::{column, container, row, text, Space};
use iced::{Color, Element, Length, Theme};
use logger_runtime::AvailSummary;

use super::style;

pub fn view<'a, M: 'a>(info: &'a AvailSummary) -> Element<'a, M> {
    let header = row![
        text("BAND").size(11.0).style(style::muted).width(Length::Fixed(60.0)),
        text("Q").size(11.0).style(style::muted).width(Length::Fixed(50.0)),
        text("MULT").size(11.0).style(style::muted).width(Length::Fill),
    ]
    .spacing(4);

    let mut rows: Vec<Element<M>> = Vec::new();
    for (band, qsos, mults) in &info.by_band {
        if *qsos == 0 && *mults == 0 {
            continue;
        }
        let mult_is_present = *mults > 0;
        rows.push(
            row![
                text(band.clone())
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(60.0)),
                text(format!("{}", qsos))
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(50.0)),
                text(format!("{}", mults))
                    .size(12.0)
                    .style(move |t: &Theme| if mult_is_present {
                        iced::widget::text::Style {
                            color: Some(style::mult_color(t)),
                        }
                    } else {
                        style::muted(t)
                    })
                    .width(Length::Fill),
            ]
            .spacing(4)
            .into(),
        );
    }
    if rows.is_empty() {
        rows.push(
            text("(no available spots)")
                .size(12.0)
                .style(style::very_muted)
                .into(),
        );
    }

    let totals = row![
        text("TOTAL")
            .size(12.0)
            .style(style::body)
            .width(Length::Fixed(60.0)),
        text(format!("{}", info.total_qsos))
            .size(12.0)
            .style(style::body)
            .width(Length::Fixed(50.0)),
        text(format!("{}", info.total_mults))
            .size(12.0)
            .style(style::body)
            .width(Length::Fill),
    ]
    .spacing(4);

    let _ = Color::TRANSPARENT; // silence unused import if removed later
    container(
        column![
            header,
            Space::new().height(2),
            column(rows).spacing(2),
            Space::new().height(4),
            totals,
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
