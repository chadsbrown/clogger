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

    // Dupe indicator
    if st.focused_entry().is_dupe {
        left_spans.push(Span::styled(
            " DUPE ",
            Style::default().fg(Color::White).bg(Color::Red),
        ));
        left_spans.push(Span::raw(" "));
    }

    // New mult indicator
    if st.focused_entry().is_new_mult {
        left_spans.push(Span::styled(
            " MULT ",
            Style::default().fg(Color::Black).bg(Color::Green),
        ));
        left_spans.push(Span::raw(" "));
    }

    // Right-aligned connection indicators (tri-state):
    //   green = configured + connected
    //   red   = configured + not connected
    //   omit  = not configured
    let mut right_spans: Vec<Span> = Vec::new();
    let push_indicator =
        |spans: &mut Vec<Span<'static>>, label: &'static str, configured: bool, connected: bool| {
            if !configured {
                return;
            }
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let color = if connected { Color::Green } else { Color::Red };
            spans.push(Span::styled(label, Style::default().fg(color)));
        };
    push_indicator(&mut right_spans, "RIG", tui.rig_configured, tui.rig_connected);
    push_indicator(&mut right_spans, "KEY", tui.keyer_configured, tui.keyer_connected);
    push_indicator(&mut right_spans, "DXF", tui.dxfeed_configured, tui.dxfeed_connected);
    push_indicator(&mut right_spans, "SO2R", tui.so2r_configured, tui.so2r_connected);

    // Scoreboard: tri-state — yellow=idle, green=ok, red=failing
    if tui.scoreboard_configured {
        if !right_spans.is_empty() {
            right_spans.push(Span::raw(" "));
        }
        let color = match tui.scoreboard_status {
            logger_runtime::ScoreboardStatus::Idle => Color::Yellow,
            logger_runtime::ScoreboardStatus::Ok => Color::Green,
            logger_runtime::ScoreboardStatus::Failing => Color::Red,
        };
        right_spans.push(Span::styled("SCRBD", Style::default().fg(color)));
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
    if !st.focused_entry().scp_matches.is_empty() {
        let scp_text = st.focused_entry().scp_matches.iter().take(10).cloned().collect::<Vec<_>>().join(" ");
        lines.push(Line::from(vec![
            Span::styled("SCP: ", Style::default().fg(Color::Cyan)),
            Span::styled(scp_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // N+1 edit-distance matches
    if !st.focused_entry().scp_n1_matches.is_empty() {
        let n1_text = st.focused_entry().scp_n1_matches.iter().take(10).cloned().collect::<Vec<_>>().join(" ");
        lines.push(Line::from(vec![
            Span::styled("N+1: ", Style::default().fg(Color::Cyan)),
            Span::styled(n1_text, Style::default().fg(Color::DarkGray)),
        ]));
    }

    if !lines.is_empty() {
        frame.render_widget(Paragraph::new(lines), area);
    }
}
