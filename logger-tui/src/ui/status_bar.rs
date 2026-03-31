use logger_core::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::TuiState;

pub fn render(frame: &mut Frame, area: Rect, st: &AppState, tui: &TuiState) {
    let mut left_spans = Vec::new();

    // My callsign
    left_spans.push(Span::styled(
        format!(" {} ", st.my_call),
        Style::default().fg(Color::White).bg(Color::Blue),
    ));
    left_spans.push(Span::raw(" "));

    // Radio freq/mode
    if let Some(rig) = st.radios.get(&st.focused_radio) {
        let freq_khz = rig.freq_hz as f64 / 1_000.0;
        left_spans.push(Span::styled(
            format!("{:.1} kHz", freq_khz),
            Style::default().fg(Color::Yellow),
        ));
        left_spans.push(Span::raw(" "));
        left_spans.push(Span::styled(&rig.mode, Style::default().fg(Color::Yellow)));
        left_spans.push(Span::raw(" "));
    }

    // Dupe indicator
    if st.entry.is_dupe {
        left_spans.push(Span::styled(
            " DUPE ",
            Style::default().fg(Color::White).bg(Color::Red),
        ));
        left_spans.push(Span::raw(" "));
    }

    // New mult indicator
    if st.entry.is_new_mult {
        left_spans.push(Span::styled(
            " MULT ",
            Style::default().fg(Color::Black).bg(Color::Green),
        ));
        left_spans.push(Span::raw(" "));
    }

    // Right-aligned connection indicators
    let mut right_spans: Vec<Span> = Vec::new();
    if tui.rig_connected {
        right_spans.push(Span::styled("RIG", Style::default().fg(Color::Green)));
    } else if st.radios.is_empty() && !tui.rig_connected {
        // Only show red if rig was configured (we detect this by checking rig_connected is false
        // but we don't track "configured"; omit if not connected and no radio state exists)
    }
    if tui.keyer_connected {
        if !right_spans.is_empty() {
            right_spans.push(Span::raw(" "));
        }
        right_spans.push(Span::styled("KEY", Style::default().fg(Color::Green)));
    }
    if tui.dxfeed_connected {
        if !right_spans.is_empty() {
            right_spans.push(Span::raw(" "));
        }
        right_spans.push(Span::styled("DXF", Style::default().fg(Color::Green)));
    }

    if right_spans.is_empty() {
        frame.render_widget(Paragraph::new(Line::from(left_spans)), area);
    } else {
        let left_width: usize = left_spans.iter().map(|s| s.width()).sum();
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let total = area.width as usize;
        let pad = total.saturating_sub(left_width + right_width + 1);
        left_spans.push(Span::raw(" ".repeat(pad)));
        left_spans.push(Span::raw(" "));
        left_spans.extend(right_spans);
        frame.render_widget(Paragraph::new(Line::from(left_spans)), area);
    }
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
