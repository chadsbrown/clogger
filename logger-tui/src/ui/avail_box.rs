use crate::AvailSummary;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table},
};

pub fn render(frame: &mut Frame, area: Rect, avail: &AvailSummary) {
    let header = Row::new(vec![
        Cell::from(" Band"),
        Cell::from("  Q"),
        Cell::from("  M"),
    ])
    .style(Style::default().fg(Color::Yellow));

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
        .style(Style::default().fg(Color::White)),
    );

    let table = Table::new(
        std::iter::once(header).chain(rows),
        [Constraint::Length(5), Constraint::Length(3), Constraint::Length(3)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Available ")
            .style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(table, area);
}
