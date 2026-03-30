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

    // Mode (Run/S&P)
    let mode_str = match st.entry.mode {
        logger_core::OpMode::Run => "RUN",
        logger_core::OpMode::Sp => "S&P",
    };
    spans.push(Span::styled(
        format!(" {} ", mode_str),
        Style::default().fg(Color::Black).bg(Color::Cyan),
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

    // SCP matches
    if !st.entry.scp_matches.is_empty() {
        let scp_text = st.entry.scp_matches.iter().take(5).cloned().collect::<Vec<_>>().join(" ");
        spans.push(Span::styled(scp_text, Style::default().fg(Color::DarkGray)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
