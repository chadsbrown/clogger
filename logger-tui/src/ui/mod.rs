pub mod cw_buffer;
pub mod entry_line;
pub mod log_tail;
pub mod status_bar;

use logger_core::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::TuiState;

pub fn render(frame: &mut Frame, app: &AppState, tui: &TuiState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // status bar
        Constraint::Length(1), // entry line
        Constraint::Length(4), // cw buffer
        Constraint::Min(3),    // log tail
        Constraint::Length(1), // footer
    ])
    .split(frame.area());

    status_bar::render(frame, chunks[0], app);
    entry_line::render(frame, chunks[1], app);
    cw_buffer::render(frame, chunks[2], &tui.cw_history);
    log_tail::render(frame, chunks[3], &tui.log_display);

    let footer = ratatui::widgets::Paragraph::new(
        " F1:CQ  F2:Exch  F3:TU  Space:Next  Enter:ESM  Esc:Clear  Ctrl-C:Quit",
    )
    .style(ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray));
    frame.render_widget(footer, chunks[4]);
}
