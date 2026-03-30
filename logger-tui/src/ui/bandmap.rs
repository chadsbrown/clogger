use logger_core::{AppState, contest::{band_freq_range, freq_to_band_label}};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Row, Table},
};

pub fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    let band = app
        .radios
        .get(&app.focused_radio)
        .filter(|r| r.freq_hz > 0)
        .map(|r| freq_to_band_label(r.freq_hz))
        .unwrap_or_else(|| "40m".to_string());

    let (min, max) = band_freq_range(&band);

    let mut spots: Vec<_> = app
        .bandmap
        .iter()
        .filter(|s| s.freq_hz >= min && s.freq_hz <= max)
        .collect();
    spots.sort_by_key(|s| s.freq_hz);
    spots.dedup_by_key(|s| s.call.clone());

    let visible = area.height.saturating_sub(2) as usize; // borders
    let skip = spots.len().saturating_sub(visible);

    let rows: Vec<Row> = spots
        .iter()
        .skip(skip)
        .map(|s| {
            let freq_mhz = s.freq_hz as f64 / 1_000_000.0;
            Row::new(vec![
                Cell::from(format!("{freq_mhz:.3}")),
                Cell::from(s.call.as_str()),
            ])
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
