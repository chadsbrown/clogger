use logger_core::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame, area: Rect, st: &AppState) {
    let mut spans = Vec::new();

    // My callsign
    spans.push(Span::styled(
        format!(" {} ", st.my_call),
        Style::default().fg(Color::White).bg(Color::Blue),
    ));
    spans.push(Span::raw(" "));

    // Radio freq/mode
    if let Some(rig) = st.radios.get(&st.focused_radio) {
        let freq_mhz = rig.freq_hz as f64 / 1_000_000.0;
        spans.push(Span::styled(
            format!("{:.3} MHz", freq_mhz),
            Style::default().fg(Color::Yellow),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(&rig.mode, Style::default().fg(Color::Yellow)));
        spans.push(Span::raw(" "));
    }

    // Dupe indicator
    if st.entry.is_dupe {
        spans.push(Span::styled(
            " DUPE ",
            Style::default().fg(Color::White).bg(Color::Red),
        ));
        spans.push(Span::raw(" "));
    }

    // New mult indicator
    if st.entry.is_new_mult {
        spans.push(Span::styled(
            " MULT ",
            Style::default().fg(Color::Black).bg(Color::Green),
        ));
        spans.push(Span::raw(" "));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn render_scp(frame: &mut Frame, area: Rect, st: &AppState) {
    let mut lines = Vec::new();

    // SCP prefix matches
    if !st.entry.scp_matches.is_empty() {
        let scp_text = st.entry.scp_matches.iter().take(10).cloned().collect::<Vec<_>>().join(" ");
        lines.push(Line::from(vec![
            Span::styled("SCP: ", Style::default().fg(Color::Cyan)),
            Span::styled(scp_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // N+1 edit-distance matches
    if !st.entry.scp_n1_matches.is_empty() {
        let n1_text = st.entry.scp_n1_matches.iter().take(10).cloned().collect::<Vec<_>>().join(" ");
        lines.push(Line::from(vec![
            Span::styled("N+1: ", Style::default().fg(Color::Cyan)),
            Span::styled(n1_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    if !lines.is_empty() {
        frame.render_widget(Paragraph::new(lines), area);
    }
}
