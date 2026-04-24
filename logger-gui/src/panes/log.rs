//! Log pane — shows the last N QSOs from `LogAdapter`.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_runtime::LogAdapter;

use super::style;

const MAX_ROWS: usize = 12;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let records = log.records();
    let count = records.len();

    let header = row![
        text("#").size(11.0).style(style::muted).width(Length::Fixed(40.0)),
        text("CALL").size(11.0).style(style::muted).width(Length::Fixed(90.0)),
        text("BAND").size(11.0).style(style::muted).width(Length::Fixed(60.0)),
        text("MODE").size(11.0).style(style::muted).width(Length::Fixed(60.0)),
        text("FREQ").size(11.0).style(style::muted).width(Length::Fill),
    ]
    .spacing(4);

    let mut rows: Vec<Element<M>> = Vec::with_capacity(MAX_ROWS);
    let start = records.len().saturating_sub(MAX_ROWS);
    for (offset, rec) in records[start..].iter().enumerate() {
        let i = start + offset;
        rows.push(
            row![
                text(format!("{}", i + 1))
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(40.0)),
                text(rec.callsign_norm.clone())
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(90.0)),
                text(format!("{:?}", rec.band))
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(60.0)),
                text(format!("{:?}", rec.mode))
                    .size(12.0)
                    .style(style::body)
                    .width(Length::Fixed(60.0)),
                text(format!("{:.3}", rec.freq_hz as f64 / 1_000_000.0))
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
            text("(no QSOs yet)")
                .size(12.0)
                .style(style::very_muted)
                .into(),
        );
    }

    container(
        column![
            text(format!("{} QSO{}", count, if count == 1 { "" } else { "s" }))
                .size(11.0)
                .style(style::accent),
            Space::new().height(4),
            header,
            column(rows).spacing(2),
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
