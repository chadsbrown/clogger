use logger_core::{AppState, contest::{filtered_bandmap_spots, freq_to_band_label}};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::TuiState;

pub fn render(frame: &mut Frame, area: Rect, app: &AppState, tui: &TuiState) {
    let radio = app
        .radios
        .get(&app.focused_radio)
        .filter(|r| r.freq_hz > 0);
    let band = radio
        .map(|r| freq_to_band_label(r.freq_hz))
        .unwrap_or_else(|| "40m".to_string());
    let mode = radio.map(|r| r.mode.as_str()).unwrap_or("CW");

    let spots = filtered_bandmap_spots(&app.bandmap, &band, mode);

    let visible = area.height.saturating_sub(2) as usize; // borders
    let skip = spots.len().saturating_sub(visible);

    let rows: Vec<Row> = spots
        .iter()
        .enumerate()
        .skip(skip)
        .map(|(i, s)| {
            let freq_mhz = s.freq_hz as f64 / 1_000_000.0;
            let row = Row::new(vec![
                Cell::from(format!("{freq_mhz:.3}")),
                Cell::from(s.call.as_str()),
            ]);
            if app.bandmap_cursor == Some(i) {
                row.style(Style::default().add_modifier(Modifier::REVERSED))
            } else if tui.worked_calls.contains(&s.call) {
                row.style(Style::default().fg(Color::DarkGray))
            } else {
                row
            }
        })
        .collect();

    let table = Table::new(
        rows,
        [Constraint::Length(8), Constraint::Min(8)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Bandmap ({band}) "))
            .style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(table, area);
}
