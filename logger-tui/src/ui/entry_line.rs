use logger_core::{AppState, RadioId, Validation};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::theme::Theme;

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    st: &AppState,
    radio_id: RadioId,
    is_focused: bool,
    tx_radio: RadioId,
    cw_text: Option<&str>,
    cw_transmitting: bool,
    theme: &Theme,
) {
    let Some(entry) = st.entry_for(radio_id) else {
        return;
    };
    let mode_str = match entry.mode {
        logger_core::OpMode::Run => "RUN",
        logger_core::OpMode::Sp => "S&P",
    };
    let mut title_spans = vec![
        Span::raw(" R"),
        Span::raw(radio_id.to_string()),
        Span::raw(" "),
        Span::styled(format!(" {} ", mode_str), Style::from(theme.mode_badge)),
        Span::raw(" "),
    ];
    if cw_transmitting && tx_radio == radio_id {
        title_spans.push(Span::styled(" TX ", Style::from(theme.tx_badge)));
        title_spans.push(Span::raw(" "));
    }
    if let Some(serial) = entry.assigned_serial.or(st.serial_counter) {
        title_spans.push(Span::styled(
            format!("NR:{serial} "),
            Style::from(theme.serial_number),
        ));
    }
    let title = Line::from(title_spans);
    let border_style: Style = if is_focused {
        theme.entry_border_focused.into()
    } else {
        theme.entry_border_unfocused.into()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = Vec::new();
    let mut cursor_col: Option<u16> = None;
    let mut col = inner.x;

    for (idx, field) in entry.fields.iter().enumerate() {
        let field_is_focused = idx == entry.focus;
        let field_width = field.width as usize;

        let label = format!("{}:", field.label);
        let label_len = label.len() as u16;
        spans.push(Span::styled(label, Style::from(theme.field_label)));
        col += label_len;

        let style: Style = match field.status {
            Validation::Valid => theme.field_valid.into(),
            Validation::Invalid(_) => theme.field_invalid.into(),
            Validation::Unknown => theme.field_unknown.into(),
        };

        if field_is_focused && is_focused {
            cursor_col = Some(col + field.cursor as u16);
        }

        let scp_check = field.field_id == 1
            && !field.value.is_empty()
            && entry.scp_matches.contains(&field.value);

        if scp_check {
            let used = field.value.len() + 2;
            let pad = field_width.saturating_sub(used);
            spans.push(Span::styled(&field.value, style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled("\u{2713}", Style::from(theme.scp_match)));
            spans.push(Span::styled(format!("{:<pad$}", ""), style));
        } else {
            let display_val = format!("{:<width$}", field.value, width = field_width);
            spans.push(Span::styled(display_val, style));
        }
        col += field_width as u16;

        spans.push(Span::raw(" "));
        col += 1;
    }

    let _ = tx_radio;
    let cw_line = if let Some(text) = cw_text {
        let w = inner.width as usize;
        let text_len = text.len().min(w);
        let pad = (w.saturating_sub(text_len)) / 2;
        Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(&text[..text_len], Style::from(theme.cw_echo)),
        ])
    } else {
        Line::default()
    };

    let freq_line = if let Some(radio) = st.radios.get(&radio_id) {
        let freq_khz = radio.freq_hz as f64 / 1_000.0;
        let text = format!("{:.1} kHz  {}", freq_khz, radio.mode);
        let w = inner.width as usize;
        let pad = w.saturating_sub(text.len());
        Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(text, Style::from(theme.frequency)),
        ])
    } else {
        Line::default()
    };

    let indicator_line = if entry.is_dupe {
        Line::from(vec![Span::styled(" DUPE ", Style::from(theme.dupe_badge))])
    } else {
        Line::default()
    };

    let lines = vec![
        freq_line,
        Line::from(spans),
        indicator_line,
        cw_line,
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(cx) = cursor_col {
        frame.set_cursor_position((cx, inner.y + 1));
    }
}
