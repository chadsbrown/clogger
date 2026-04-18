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

    fn fmt_since(secs: Option<u64>) -> String {
        match secs {
            None => "-".to_string(),
            Some(s) if s < 60 => format!("{s}s"),
            Some(s) if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
            Some(s) => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
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
        Line::from(vec![
            Span::styled(" Since:  ", Style::from(theme.box_label)),
            Span::styled(
                format!("{:>8}", fmt_since(rate.secs_since_last_qso)),
                Style::from(theme.box_value),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
