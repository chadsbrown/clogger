use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, cw_history: &[String]) {
    let visible_lines = area.height.saturating_sub(2) as usize; // account for borders
    let skip = cw_history.len().saturating_sub(visible_lines);
    let lines: Vec<Line> = cw_history
        .iter()
        .skip(skip)
        .map(|s| Line::raw(s.as_str()))
        .collect();

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .title(" CW ")
                .style(Style::default().fg(Color::DarkGray)),
        )
        .style(Style::default().fg(Color::Cyan));

    frame.render_widget(widget, area);
}
