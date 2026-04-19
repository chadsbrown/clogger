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

    // Resolve the cursor against the natural (freq-ascending) slice.
    // `On` carries a call (index re-resolved against the current filtered
    // list); `Between` carries a frequency (insertion point computed here).
    let cursor = app.bandmap_cursors.get(&radio_id);
    let highlight_nat: Option<usize> = match cursor {
        Some(BandmapCursor::On { call, .. }) => spots.iter().position(|s| &s.call == call),
        _ => None,
    };
    let divider_nat: Option<usize> = match cursor {
        Some(BandmapCursor::Between { freq_hz }) => {
            Some(spots.partition_point(|s| s.freq_hz < *freq_hz))
        }
        _ => None,
    };

    // Translate to display positions when the bandmap is reversed.
    // Reversed mapping for `len` spots: natural index n → display row
    // (len-1-n); a divider at natural insertion-point p (between spots
    // p-1 and p) → display insertion-point (len-p).
    let reversed = app.bandmap_high_at_top;
    let len = spots.len();
    let highlight_disp = highlight_nat.map(|n| if reversed { len - 1 - n } else { n });
    let divider_disp = divider_nat.map(|p| if reversed { len - p } else { p });

    // Build all rows (including divider if the rig is parked between
    // spots). Iterates by display row, translating back to natural
    // index for the spot lookup.
    let divider_style = Style::from(theme.bandmap_divider);
    let divider_row = || {
        Row::new(vec![
            Cell::from("───────"),
            Cell::from("────────────"),
        ])
        .style(divider_style)
    };
    let mut all_rows: Vec<Row> = Vec::with_capacity(len + 1);
    for d in 0..len {
        if divider_disp == Some(d) {
            all_rows.push(divider_row());
        }
        let n = if reversed { len - 1 - d } else { d };
        let s = &spots[n];
        let freq_khz = s.freq_hz as f64 / 1_000.0;
        let row = Row::new(vec![
            Cell::from(format!("{freq_khz:.1}")),
            Cell::from(s.call.as_str()),
        ]);
        // Classification sets are per-radio so each bandmap reflects
        // its own radio's band+mode dupe/mult state — not the focused
        // radio's.
        let worked_here = tui
            .worked_calls
            .get(&radio_id)
            .is_some_and(|set| set.contains(&s.call));
        let mult_here = tui
            .mult_calls
            .get(&radio_id)
            .is_some_and(|set| set.contains(&s.call));
        let styled = if highlight_disp == Some(d) {
            row.style(Style::from(theme.bandmap_highlight))
        } else if worked_here {
            row.style(Style::from(theme.bandmap_worked))
        } else if mult_here {
            row.style(Style::from(theme.bandmap_mult))
        } else {
            row.style(Style::from(theme.bandmap_unworked))
        };
        all_rows.push(styled);
    }
    if divider_disp == Some(len) {
        all_rows.push(divider_row());
    }

    // Scroll target: the cursor row in display coordinates.
    let target = highlight_disp.or(divider_disp);
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
