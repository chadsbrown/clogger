use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct LogRow {
    pub nr: u64,
    pub call: String,
    pub band: String,
    pub mode: String,
    pub exchange: String,
}

pub fn render(frame: &mut Frame, area: Rect, rows: &[LogRow], theme: &Theme) {
    let visible = area.height.saturating_sub(3) as usize;
    let skip = rows.len().saturating_sub(visible);

    let table_rows: Vec<Row> = rows
        .iter()
        .skip(skip)
        .map(|r| {
            Row::new(vec![
                Cell::from(r.nr.to_string()),
                Cell::from(r.call.as_str()),
                Cell::from(r.band.as_str()),
                Cell::from(r.mode.as_str()),
                Cell::from(r.exchange.as_str()),
            ])
        })
        .collect();

    let header = Row::new(vec!["#", "Call", "Band", "Mode", "Exchange"])
        .style(Style::from(theme.box_header));

    let table = Table::new(
        table_rows,
        [
            ratatui::layout::Constraint::Length(4),
            ratatui::layout::Constraint::Length(12),
            ratatui::layout::Constraint::Length(6),
            ratatui::layout::Constraint::Length(7),
            ratatui::layout::Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::TOP)
            .title(" Log ")
            .style(Style::from(theme.box_border)),
    );

    frame.render_widget(table, area);
}
