use logger_runtime::ScoreSummary;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table},
};

pub fn render(frame: &mut Frame, area: Rect, score: &ScoreSummary) {
    let header = Row::new(vec![
        Cell::from(" Band"),
        Cell::from("  Q"),
        Cell::from("  M"),
    ])
    .style(Style::default().fg(Color::Yellow));

    let mut rows: Vec<Row> = score
        .by_band
        .iter()
        .filter(|(_, bs)| bs.qsos > 0 || bs.mults > 0)
        .map(|(band, bs)| {
            Row::new(vec![
                Cell::from(format!("{band:>5}")),
                Cell::from(format!("{:>3}", bs.qsos)),
                Cell::from(format!("{:>3}", bs.mults)),
            ])
        })
        .collect();

    // Totals row
    rows.push(
        Row::new(vec![
            Cell::from("  Tot"),
            Cell::from(format!("{:>3}", score.total_qsos)),
            Cell::from(format!("{:>3}", score.total_mults)),
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
            .title(" Score ")
            .style(Style::default().fg(Color::DarkGray)),
    )
    .footer(
        Row::new(vec![Cell::from(format!(" Score: {}", score.claimed_score))])
            .style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(table, area);
}
