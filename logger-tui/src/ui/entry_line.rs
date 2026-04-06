use logger_core::{AppState, Validation};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, st: &AppState, cw_history: &[String], cw_echo: &str, cw_echo_enabled: bool, cw_transmitting: bool) {
    let mode_str = match st.entry.mode {
        logger_core::OpMode::Run => "RUN",
        logger_core::OpMode::Sp => "S&P",
    };
    let mut title_spans = vec![
        Span::raw(" R"),
        Span::raw(st.focused_radio.to_string()),
        Span::raw(" "),
        Span::styled(
            format!(" {} ", mode_str),
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" "),
    ];
    if cw_transmitting {
        title_spans.push(Span::styled(
            " TX ",
            Style::default().fg(Color::White).bg(Color::Red),
        ));
        title_spans.push(Span::raw(" "));
    }
    // Show serial number if contest uses serials
    if let Some(serial) = st.entry.assigned_serial.or(st.serial_counter) {
        title_spans.push(Span::styled(
            format!("NR:{serial} "),
            Style::default().fg(Color::Yellow),
        ));
    }
    let title = Line::from(title_spans);
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
            cursor_col = Some(col + field.cursor as u16);
        }

        let scp_check = field.field_id == 1
            && !field.value.is_empty()
            && st.entry.scp_matches.contains(&field.value);

        if scp_check {
            // Value + checkmark + remaining padding, all within field_width
            let used = field.value.len() + 2; // value + space + checkmark
            let pad = field_width.saturating_sub(used);
            spans.push(Span::styled(&field.value, style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled("\u{2713}", Style::default().fg(Color::Green)));
            spans.push(Span::styled(format!("{:<pad$}", ""), style));
        } else {
            let display_val = format!("{:<width$}", field.value, width = field_width);
            spans.push(Span::styled(display_val, style));
        }
        col += field_width as u16;

        // Separator
        spans.push(Span::raw(" "));
        col += 1;
    }

    let cw_text = if cw_echo_enabled {
        if cw_echo.is_empty() { None } else { Some(cw_echo) }
    } else {
        cw_history.last().map(|s| s.as_str())
    };
    let cw_line = if let Some(text) = cw_text {
        let w = inner.width as usize;
        let text_len = text.len().min(w);
        let pad = (w.saturating_sub(text_len)) / 2;
        Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(&text[..text_len], Style::default().fg(Color::Cyan)),
        ])
    } else {
        Line::default()
    };

    let freq_line = if let Some(radio) = st.radios.get(&st.focused_radio) {
        let freq_khz = radio.freq_hz as f64 / 1_000.0;
        let text = format!("{:.1} kHz  {}", freq_khz, radio.mode);
        let w = inner.width as usize;
        let pad = w.saturating_sub(text.len());
        Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(text, Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::default()
    };

    let lines = vec![
        freq_line,
        Line::from(spans),
        cw_line,
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    // Position cursor on focused field (middle line of inner area)
    if let Some(cx) = cursor_col {
        frame.set_cursor_position((cx, inner.y + 1));
    }
}
