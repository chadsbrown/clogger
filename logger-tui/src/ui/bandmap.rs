use logger_core::{
    AppState, BandmapCursor, RadioId,
    contest::{freq_to_band_label, normalize_mode},
};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::TuiState;

pub fn render(frame: &mut Frame, area: Rect, app: &AppState, tui: &TuiState, radio_id: RadioId) {
    let theme = &tui.theme;
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

    // Resolve the cursor to concrete indices each render. `On` carries
    // a call (index re-resolved against the current filtered list);
    // `Between` carries a frequency (insertion point computed here).
    let cursor = app.bandmap_cursors.get(&radio_id);
    let highlight_idx: Option<usize> = match cursor {
        Some(BandmapCursor::On { call, .. }) => spots.iter().position(|s| &s.call == call),
        _ => None,
    };
    let divider_idx: Option<usize> = match cursor {
        Some(BandmapCursor::Between { freq_hz }) => {
            Some(spots.partition_point(|s| s.freq_hz < *freq_hz))
        }
        _ => None,
    };

    // Build all rows (including divider if the rig is parked between
    // spots). Indexing below is over the final post-insertion vec.
    let divider_style = Style::from(theme.bandmap_divider);
    let divider_row = || {
        Row::new(vec![
            Cell::from("───────"),
            Cell::from("────────────"),
        ])
        .style(divider_style)
    };
    let mut all_rows: Vec<Row> = Vec::with_capacity(spots.len() + 1);
    for (i, s) in spots.iter().enumerate() {
        if divider_idx == Some(i) {
            all_rows.push(divider_row());
        }
        let freq_khz = s.freq_hz as f64 / 1_000.0;
        let row = Row::new(vec![
            Cell::from(format!("{freq_khz:.1}")),
            Cell::from(s.call.as_str()),
        ]);
        let styled = if highlight_idx == Some(i) {
            row.style(Style::from(theme.bandmap_highlight))
        } else if tui.worked_calls.contains(&s.call) {
            row.style(Style::from(theme.bandmap_worked))
        } else if tui.mult_calls.contains(&s.call) {
            row.style(Style::from(theme.bandmap_mult))
        } else {
            row.style(Style::from(theme.bandmap_unworked))
        };
        all_rows.push(styled);
    }
    if divider_idx == Some(spots.len()) {
        all_rows.push(divider_row());
    }

    // Scroll target: the cursor row (highlight or divider). Whichever
    // variant is active, we already resolved it to an index above.
    let target = highlight_idx.or(divider_idx);
    let total = all_rows.len();
    let visible = area.height.saturating_sub(2) as usize;
    let skip = if let Some(c) = target {
        if c < visible / 2 {
            0
        } else {
            (c - visible / 2).min(total.saturating_sub(visible))
        }
    } else {
        total.saturating_sub(visible)
    };
    let rows: Vec<Row> = all_rows.into_iter().skip(skip).collect();

    let label = format!(" R{radio_id} Bandmap ({band}) ");
    let table = Table::new(
        rows,
        [Constraint::Length(9), Constraint::Min(8)],
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(label)
            .style(Style::from(theme.bandmap_border)),
    );

    frame.render_widget(table, area);
}
