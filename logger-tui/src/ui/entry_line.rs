use logger_core::{AppState, Validation};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, st: &AppState, cw_history: &[String]) {
    let mode_str = match st.entry.mode {
        logger_core::OpMode::Run => "RUN",
        logger_core::OpMode::Sp => "S&P",
    };
    let title = Line::from(vec![
        Span::raw(" R"),
        Span::raw(st.focused_radio.to_string()),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", mode_str),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" "),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = Vec::new();
    let mut cursor_col: Option<u16> = None;
    let mut col = inner.x;

    for (idx, field) in st.entry.fields.iter().enumerate() {
        let is_focused = idx == st.entry.focus;
        let field_width = field.width as usize;

        // Label
        let label = format!("{}:", field.label);
        let label_len = label.len() as u16;
        spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
        col += label_len;

        // Value with validation color, padded to fixed width
        let fg = match field.status {
            Validation::Valid => Color::Green,
            Validation::Invalid(_) => Color::Red,
            Validation::Unknown => Color::White,
        };
        let style = Style::default().fg(fg);

        if is_focused {
            cursor_col = Some(col + field.value.len() as u16);
        }

        let display_val = format!("{:<width$}", field.value, width = field_width);
        let val_len = display_val.len() as u16;
        spans.push(Span::styled(display_val, style));
        col += val_len;

        // Separator
        spans.push(Span::raw(" "));
        col += 1;
    }

    let cw_line = if let Some(last) = cw_history.last() {
        let w = inner.width as usize;
        let text_len = last.len().min(w);
        let pad = (w.saturating_sub(text_len)) / 2;
        Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(&last[..text_len], Style::default().fg(Color::Cyan)),
        ])
    } else {
        Line::default()
    };

    let lines = vec![
        Line::default(),
        Line::from(spans),
        cw_line,
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    // Position cursor on focused field (middle line of inner area)
    if let Some(cx) = cursor_col {
        frame.set_cursor_position((cx, inner.y + 1));
    }
}
