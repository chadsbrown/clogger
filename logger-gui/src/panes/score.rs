//! Score pane — band × (QSO / MULT) matrix plus claimed score.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_runtime::LogAdapter;

use super::style;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let _perf = crate::perf::Span::new("pane.score"); // PROFILING
    let summary = log.score_summary();

    let header = row![
        text("BAND").size(11.0).style(style::muted).width(Length::Fixed(60.0)),
        text("QSO").size(11.0).style(style::muted).width(Length::Fixed(60.0)),
        text("MULT").size(11.0).style(style::muted).width(Length::Fill),
    ]
    .spacing(4);

    let mut rows: Vec<Element<M>> = Vec::new();
    for (band, bs) in &summary.by_band {
        rows.push(
            row![
                text(band.clone())
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(60.0)),
                text(format!("{}", bs.qsos))
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(60.0)),
                text(format!("{}", bs.mults))
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fill),
            ]
            .spacing(4)
            .into(),
        );
    }
    if rows.is_empty() {
        rows.push(
            text("(no QSOs logged)")
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
        text(format!("{}", summary.total_qsos))
            .size(12.0)
            .style(style::body)
            .width(Length::Fixed(60.0)),
        text(format!("{}", summary.total_mults))
            .size(12.0)
            .style(style::body)
            .width(Length::Fill),
    ]
    .spacing(4);

    let claimed = text(format!("Claimed: {}", summary.claimed_score))
        .size(14.0)
        .style(style::accent);

    container(
        column![
            header,
            Space::new().height(2),
            column(rows).spacing(2),
            Space::new().height(4),
            totals,
            Space::new().height(6),
            claimed,
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
