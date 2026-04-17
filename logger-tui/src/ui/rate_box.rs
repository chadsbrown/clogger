use logger_runtime::RateInfo;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, rate: &RateInfo, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rate ")
        .style(Style::from(theme.box_border));

    fn fmt_minutes(v: Option<f64>) -> String {
        match v {
            Some(m) => format!("{m:.0}"),
            None => "-".to_string(),
        }
    }

    let lines = vec![
        Line::from(vec![
            Span::styled(" Last 10: ", Style::from(theme.box_label)),
            Span::styled(
                format!("{:>4} min", fmt_minutes(rate.last_10_minutes)),
                Style::from(theme.box_value),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Last100: ", Style::from(theme.box_label)),
            Span::styled(
                format!("{:>4} min", fmt_minutes(rate.last_100_minutes)),
                Style::from(theme.box_value),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Rate:  ", Style::from(theme.box_label)),
            Span::styled(
                format!("{:>5}/hr", rate.rate_per_hour),
                Style::from(theme.box_value),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
