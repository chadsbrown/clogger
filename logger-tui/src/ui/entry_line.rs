use logger_core::{AppState, Validation};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame, area: Rect, st: &AppState) {
    let mut spans = Vec::new();
    let mut cursor_col: Option<u16> = None;
    let mut col = area.x;

    for (idx, field) in st.entry.fields.iter().enumerate() {
        let is_focused = idx == st.entry.focus;

        // Label
        let label = format!("{}:", field.label);
        let label_len = label.len() as u16;
        spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
        col += label_len;

        // Value with validation color
        let fg = match field.status {
            Validation::Valid => Color::Green,
            Validation::Invalid(_) => Color::Red,
            Validation::Unknown => Color::White,
        };
        let mut style = Style::default().fg(fg);
        if is_focused {
            style = style.add_modifier(Modifier::UNDERLINED);
        }

        if is_focused {
            cursor_col = Some(col + field.value.len() as u16);
        }

        let val = if field.value.is_empty() && is_focused {
            "_".to_string()
        } else {
            field.value.clone()
        };
        let val_len = val.len() as u16;
        spans.push(Span::styled(val, style));
        col += val_len;

        // Separator
        spans.push(Span::raw("  "));
        col += 2;
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    // Position cursor on focused field
    if let Some(cx) = cursor_col {
        frame.set_cursor_position((cx, area.y));
    }
}
