use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ExportModal;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, modal: &ExportModal, theme: &Theme) {
    let area = centered_rect(50, 12, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Export ")
        .borders(Borders::ALL)
        .border_style(Style::from(theme.modal_border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match &modal.step {
        ExportStep::SelectFormat => render_format_select(frame, inner, theme),
        ExportStep::InputPath { path, cursor } => render_path_input(frame, inner, path, *cursor, theme),
        ExportStep::Result { message, is_error } => {
            render_result(frame, inner, message, *is_error, theme)
        }
    }
}

pub fn handle_key(
    modal: &mut Option<ExportModal>,
    key_code: crossterm::event::KeyCode,
    records: &[logger_runtime::QsoRecord],
    my_call: &str,
    contest_id: &str,
) -> bool {
    let m = match modal {
        Some(m) => m,
        None => return false,
    };

    match &mut m.step {
        ExportStep::SelectFormat => match key_code {
            crossterm::event::KeyCode::Char('a') | crossterm::event::KeyCode::Char('A') => {
                let default_path = format!("{}-{}.adi", my_call.to_lowercase(), contest_id);
                let cursor = default_path.len();
                m.step = ExportStep::InputPath {
                    path: default_path,
                    cursor,
                };
            }
            crossterm::event::KeyCode::Esc => {
                *modal = None;
            }
            _ => {}
        },
        ExportStep::InputPath { path, cursor } => match key_code {
            crossterm::event::KeyCode::Enter => {
                let export_path = std::path::Path::new(&*path);
                match logger_runtime::adif_export::export_adif(
                    records, my_call, contest_id, export_path,
                ) {
                    Ok(count) => {
                        m.step = ExportStep::Result {
                            message: format!("Exported {count} QSOs to {path}"),
                            is_error: false,
                        };
                    }
                    Err(e) => {
                        m.step = ExportStep::Result {
                            message: format!("Export failed: {e}"),
                            is_error: true,
                        };
                    }
                }
            }
            crossterm::event::KeyCode::Esc => {
                *modal = None;
            }
            crossterm::event::KeyCode::Backspace => {
                if *cursor > 0 {
                    path.remove(*cursor - 1);
                    *cursor -= 1;
                }
            }
            crossterm::event::KeyCode::Left => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
            }
            crossterm::event::KeyCode::Right => {
                if *cursor < path.len() {
                    *cursor += 1;
                }
            }
            crossterm::event::KeyCode::Char(c) => {
                path.insert(*cursor, c);
                *cursor += 1;
            }
            _ => {}
        },
        ExportStep::Result { .. } => {
            *modal = None;
        }
    }

    true
}

#[derive(Debug, Clone)]
pub enum ExportStep {
    SelectFormat,
    InputPath { path: String, cursor: usize },
    Result { message: String, is_error: bool },
}

fn render_format_select(frame: &mut Frame, area: Rect, theme: &Theme) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);

    frame.render_widget(Paragraph::new("Select export format:"), chunks[0]);
    frame.render_widget(Paragraph::new(""), chunks[1]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  [A] ", Style::from(theme.modal_active_option)),
            Span::raw("ADIF (.adi)"),
        ])),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  [C] ", Style::from(theme.modal_disabled_option)),
            Span::styled("Cabrillo (not yet implemented)", Style::from(theme.modal_disabled_option)),
        ])),
        chunks[3],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Esc ", Style::from(theme.modal_help_text)),
            Span::raw("Cancel"),
        ])),
        chunks[4],
    );
}

fn render_path_input(frame: &mut Frame, area: Rect, path: &str, cursor: usize, theme: &Theme) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);

    frame.render_widget(Paragraph::new("Export ADIF to:"), chunks[0]);
    frame.render_widget(Paragraph::new(""), chunks[1]);

    let display_path = if path.is_empty() { " " } else { path };
    frame.render_widget(
        Paragraph::new(display_path).style(Style::from(theme.modal_input)),
        chunks[2],
    );

    let cursor_x = area.x + (cursor as u16).min(area.width.saturating_sub(1));
    frame.set_cursor_position((cursor_x, chunks[2].y));

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Enter ", Style::from(theme.modal_help_text)),
            Span::raw("Export  "),
            Span::styled("Esc ", Style::from(theme.modal_help_text)),
            Span::raw("Cancel"),
        ])),
        chunks[3],
    );
}

fn render_result(frame: &mut Frame, area: Rect, message: &str, is_error: bool, theme: &Theme) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);

    let style: Style = if is_error {
        theme.modal_error.into()
    } else {
        theme.modal_success.into()
    };
    frame.render_widget(Paragraph::new(message).style(style), chunks[1]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Press any key ", Style::from(theme.modal_help_text)),
            Span::raw("to close"),
        ])),
        chunks[3],
    );
}

fn centered_rect(width_pct: u16, height: u16, area: Rect) -> Rect {
    let popup_width = (area.width * width_pct / 100).max(30);
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, popup_width, height.min(area.height))
}
