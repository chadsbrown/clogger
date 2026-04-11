use logger_core::{AppState, RadioId, contest::{freq_to_band_label, normalize_mode}};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::TuiState;

pub fn render(frame: &mut Frame, area: Rect, app: &AppState, tui: &TuiState, radio_id: RadioId) {
    let radio = app
        .radios
        .get(&radio_id)
        .filter(|r| r.freq_hz > 0);
    let band = radio
        .map(|r| freq_to_band_label(r.freq_hz))
        .unwrap_or("40m");
    // Normalize mode to a `&'static str` that lives in the cache key pool.
    let mode = normalize_mode(radio.map(|r| r.mode.as_str()).unwrap_or("CW"));

    // Fetch through the shared TuiState cache. Interior-mutable via
    // RefCell — cache miss triggers one filter+sort+dedup; subsequent
    // renders at the same (band, mode, bandmap_version) hit the cache.
    let mut cache = tui.bandmap_cache.borrow_mut();
    let spots = cache.get_or_build(&app.bandmap, app.bandmap_version, band, mode);

    let cursor = app.bandmap_cursors.get(&radio_id).copied();
    let visible = area.height.saturating_sub(2) as usize; // borders
    let skip = if let Some(c) = cursor {
        if c < visible / 2 {
            0
        } else {
            (c - visible / 2).min(spots.len().saturating_sub(visible))
        }
    } else {
        spots.len().saturating_sub(visible)
    };

    let rows: Vec<Row> = spots
        .iter()
        .enumerate()
        .skip(skip)
        .map(|(i, s)| {
            let freq_khz = s.freq_hz as f64 / 1_000.0;
            let row = Row::new(vec![
                Cell::from(format!("{freq_khz:.1}")),
                Cell::from(s.call.as_str()),
            ]);
            if cursor == Some(i) {
                row.style(Style::default().add_modifier(Modifier::REVERSED))
            } else if tui.worked_calls.contains(&s.call) {
                row.style(Style::default().fg(Color::DarkGray))
            } else if tui.mult_calls.contains(&s.call) {
                row.style(Style::default().fg(Color::Green))
            } else {
                row.style(Style::default().fg(Color::White))
            }
        })
        .collect();

    let label = format!(" R{radio_id} Bandmap ({band}) ");
    let table = Table::new(
        rows,
        [Constraint::Length(9), Constraint::Min(8)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(label)
            .style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(table, area);
}
