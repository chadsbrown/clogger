//! Score pane — band × (QSO / MULT) matrix plus claimed score.

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};
use logger_runtime::LogAdapter;

use super::style;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let summary = log.score_summary();

    let mut rows: Vec<Element<M>> = Vec::new();
    for (band, bs) in &summary.by_band {
        rows.push(band_score_row(band, bs.qsos, bs.mults));
    }
    if rows.is_empty() {
        rows.push(
            container(
                text("(no QSOs logged)")
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
                summary_tile("QSOs", summary.total_qsos),
                summary_tile("Mults", summary.total_mults),
            ]
            .spacing(8),
            column(rows).spacing(2),
            score_footer(summary.claimed_score),
        ]
        .spacing(6),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn summary_tile<'a, M: 'a>(label: &'static str, value: u32) -> Element<'a, M> {
    container(
        column![
            text(label).size(style::TEXT_TINY).style(style::muted),
            text(value.to_string())
                .size(style::TEXT_VALUE)
                .font(Font::MONOSPACE)
                .style(style::body),
        ]
        .spacing(2),
    )
    .padding([5, 10])
    .width(Length::FillPortion(1))
    .style(style::card_style)
    .into()
}

fn band_score_row<'a, M: 'a>(band: &str, qsos: u32, mults: u32) -> Element<'a, M> {
    container(
        row![
            container(
                text(band.to_string())
                    .size(style::TEXT_BODY)
                    .font(Font::MONOSPACE)
                    .style(style::body),
            )
            .padding([3, 8])
            .style(style::muted_badge_style)
            .width(Length::Fixed(64.0)),
            metric_column("QSO", qsos, false),
            metric_column("MULT", mults, mults > 0),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([4, 8])
    .style(style::card_style)
    .into()
}

fn metric_column<'a, M: 'a>(label: &'static str, value: u32, emphasize: bool) -> Element<'a, M> {
    column![
        text(label)
            .size(style::TEXT_TINY)
            .style(move |t: &Theme| if emphasize { style::accent(t) } else { style::muted(t) }),
        text(value.to_string())
            .size(style::TEXT_VALUE)
            .font(Font::MONOSPACE)
            .style(move |t: &Theme| {
                if emphasize {
                    iced::widget::text::Style {
                        color: Some(style::mult_color(t)),
                    }
                } else {
                    style::body(t)
                }
            }),
    ]
    .spacing(1)
    .width(Length::FillPortion(1))
    .into()
}

fn score_footer<'a, M: 'a>(claimed_score: i64) -> Element<'a, M> {
    container(
        row![
            text("Score").size(style::TEXT_TINY).style(style::muted),
            text(claimed_score.to_string())
                .size(style::TEXT_VALUE)
                .font(Font::MONOSPACE)
                .style(style::accent),
        ]
        .spacing(10)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([5, 10])
    .style(style::card_style)
    .width(Length::Fill)
    .into()
}
