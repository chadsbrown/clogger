pub mod avail_box;
pub mod bandmap;
pub mod entry_line;
pub mod log_tail;
pub mod rate_box;
pub mod score_box;
pub mod status_bar;

use logger_core::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::TuiState;

pub fn render(frame: &mut Frame, app: &AppState, tui: &TuiState) {
    let half_width = frame.area().width / 2;
    let left_width = (frame.area().width - half_width) / 2;
    let right_width = frame.area().width - half_width - left_width;

    // Vertical: main_area + status bar + footer
    let rows = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    // Horizontal 3-column split
    let cols = Layout::horizontal([
        Constraint::Length(left_width),
        Constraint::Length(half_width),
        Constraint::Length(right_width),
    ])
    .split(rows[0]);

    // Center column: log(max 10) + entry(5) + scp(2) + filler
    let center = Layout::vertical([
        Constraint::Max(10),
        Constraint::Length(5),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .split(cols[1]);

    // Left: score + available + rate
    let avail_height = tui.avail.by_band.len() as u16 + 4; // header + band rows + totals + 2 borders
    let left = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(avail_height),
        Constraint::Length(5),
    ])
    .split(cols[0]);

    score_box::render(frame, left[0], &tui.score);
    avail_box::render(frame, left[1], &tui.avail);
    rate_box::render(frame, left[2], &tui.rate);

    // Center: log + entry + scp
    log_tail::render(frame, center[0], &tui.log_display);
    entry_line::render(frame, center[1], app, &tui.cw_history);
    status_bar::render_scp(frame, center[2], app);

    // Right: bandmap
    bandmap::render(frame, cols[2], app, tui);

    // Status bar
    status_bar::render(frame, rows[1], app, tui);

    // Footer
    let footer = ratatui::widgets::Paragraph::new(
        " F1:CQ  F2:Exch  F3:TU  Space:Next  Enter:ESM  Ins:Run/S&P  C-\u{2191}\u{2193}:Bandmap  Esc:Clear  Ctrl-C:Quit",
    )
    .style(ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray));
    frame.render_widget(footer, rows[2]);
}
