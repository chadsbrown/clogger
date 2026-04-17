use logger_runtime::AvailSummary;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, avail: &AvailSummary, theme: &Theme) {
    let header = Row::new(vec![
        Cell::from(" Band"),
        Cell::from("  Q"),
        Cell::from("  M"),
    ])
    .style(Style::from(theme.box_header));

    let mut rows: Vec<Row> = avail
        .by_band
        .iter()
        .filter(|(_, qsos, _)| *qsos > 0)
        .map(|(band, qsos, mults)| {
            Row::new(vec![
                Cell::from(format!("{band:>5}")),
                Cell::from(format!("{qsos:>3}")),
                Cell::from(format!("{mults:>3}")),
            ])
        })
        .collect();

    rows.push(
        Row::new(vec![
            Cell::from("  Tot"),
            Cell::from(format!("{:>3}", avail.total_qsos)),
            Cell::from(format!("{:>3}", avail.total_mults)),
        ])
        .style(Style::from(theme.box_total)),
    );

    let table = Table::new(
        std::iter::once(header).chain(rows),
        [Constraint::Length(5), Constraint::Length(3), Constraint::Length(3)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Available ")
            .style(Style::from(theme.box_border)),
    );

    frame.render_widget(table, area);
}
