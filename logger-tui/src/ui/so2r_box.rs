use logger_core::{RadioId, So2rRxMode};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::theme::Theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused_radio: RadioId,
    tx_radio: RadioId,
    rx_mode: So2rRxMode,
    theme: &Theme,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" SO2R ")
        .style(Style::from(theme.box_border));

    let (left_ear, right_ear) = match rx_mode {
        So2rRxMode::Mono => (focused_radio, focused_radio),
        So2rRxMode::Stereo => (1, 2),
        So2rRxMode::ReverseStereo => (2, 1),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(" FOCUS: ", Style::from(theme.box_label)),
            Span::styled(format!("R{focused_radio}"), Style::from(theme.box_value)),
        ]),
        Line::from(vec![
            Span::styled(" RX:    ", Style::from(theme.box_label)),
            Span::styled(
                format!("R{left_ear} / R{right_ear}"),
                Style::from(theme.box_value),
            ),
        ]),
        Line::from(vec![
            Span::styled(" TX:    ", Style::from(theme.box_label)),
            Span::styled(format!("R{tx_radio}"), Style::from(theme.box_value)),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
