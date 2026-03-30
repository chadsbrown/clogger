pub mod bandmap;
pub mod entry_line;
pub mod log_tail;
pub mod status_bar;

use logger_core::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
};

use crate::TuiState;

pub fn render(frame: &mut Frame, app: &AppState, tui: &TuiState) {
    let chunks = Layout::vertical([
        Constraint::Min(3),    // log tail + bandmap
        Constraint::Length(5), // entry line (bordered, with padding)
        Constraint::Length(2), // scp / n+1
        Constraint::Length(1), // status bar
        Constraint::Length(1), // footer
    ])
    .split(frame.area());

    let top = Layout::horizontal([
        Constraint::Percentage(70), // log tail
        Constraint::Percentage(30), // bandmap
    ])
    .split(chunks[0]);

    log_tail::render(frame, top[0], &tui.log_display);
    bandmap::render(frame, top[1], app);

    // Center the entry box and SCP area at half screen width
    let half_width = chunks[1].width / 2;
    let entry_area = center_horizontal(chunks[1], half_width);
    entry_line::render(frame, entry_area, app, &tui.cw_history);

    let scp_area = center_horizontal(chunks[2], half_width);
    status_bar::render_scp(frame, scp_area, app);

    status_bar::render(frame, chunks[3], app);

    let footer = ratatui::widgets::Paragraph::new(
        " F1:CQ  F2:Exch  F3:TU  Space:Next  Enter:ESM  Ins:Run/S&P  Esc:Clear  Ctrl-C:Quit",
    )
    .style(ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray));
    frame.render_widget(footer, chunks[4]);
}

fn center_horizontal(area: Rect, width: u16) -> Rect {
    let [centered] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    centered
}
