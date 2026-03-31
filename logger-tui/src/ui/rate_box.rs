use crate::RateInfo;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, rate: &RateInfo) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rate ")
        .style(Style::default().fg(Color::DarkGray));

    fn fmt_minutes(v: Option<f64>) -> String {
        match v {
            Some(m) => format!("{m:.0}"),
            None => "-".to_string(),
        }
    }

    let lines = vec![
        Line::from(vec![
            Span::styled(" Last 10: ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{:>4} min", fmt_minutes(rate.last_10_minutes)),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Last100: ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{:>4} min", fmt_minutes(rate.last_100_minutes)),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Rate:  ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{:>5}/hr", rate.rate_per_hour),
                Style::default().fg(Color::Cyan),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
