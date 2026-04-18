pub mod avail_box;
pub mod bandmap;
pub mod condx_box;
pub mod entry_line;
pub mod export_modal;
pub mod log_tail;
pub mod rate_box;
pub mod score_box;
pub mod so2r_box;
pub mod status_bar;

use logger_core::AppState;
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::Block,
};

use crate::config::BandmapMode;

use crate::TuiState;

pub fn render(frame: &mut Frame, app: &AppState, tui: &TuiState) {
    // Fill the entire terminal with the theme background before rendering
    // widgets. Color::Reset means "use the terminal's native background"
    // (backward compat for the default theme).
    if tui.theme.background != Color::Reset {
        frame.render_widget(
            Block::default().style(Style::default().bg(tui.theme.background)),
            frame.area(),
        );
    }

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

    // Center column: filler + log(max 10) + entry R1(6) + [entry R2(6)] + scp(2).
    // Filler goes on top so Log/R1/R2/SCP are bottom-justified against the
    // status bar, keeping the entry boxes near the operator's visual anchor.
    // R2 is only shown when at least two rigs are configured; a single-rig
    // (or headless) setup drops the R2 slot entirely.
    let show_r2 = tui.rigs.len() >= 2;
    let center = if show_r2 {
        Layout::vertical([
            Constraint::Min(0),
            Constraint::Max(10),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(cols[1])
    } else {
        Layout::vertical([
            Constraint::Min(0),
            Constraint::Max(10),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(cols[1])
    };

    // Left: [CondX when enabled] + score + available + rate + [SO2R when
    // configured]. Score uses Min(4) so it absorbs spare vertical space;
    // every other box is fixed-height. Boxes not configured are skipped
    // entirely so non-users pay no visual cost.
    let avail_height = tui.avail.by_band.len() as u16 + 4; // header + band rows + totals + 2 borders
    let mut constraints: Vec<Constraint> = Vec::new();
    if tui.condx_configured {
        constraints.push(Constraint::Length(12)); // CondX: 10 rows + 2 borders
    }
    let score_idx = constraints.len();
    constraints.push(Constraint::Min(4));
    constraints.push(Constraint::Length(avail_height));
    constraints.push(Constraint::Length(6)); // Rate: 4 rows + 2 borders
    if tui.so2r_configured {
        constraints.push(Constraint::Length(5)); // SO2R: 3 rows + 2 borders
    }
    let left = Layout::vertical(constraints).split(cols[0]);

    let theme = &tui.theme;

    // Render in layout order so slot indices stay in sync with `constraints`.
    let mut idx = 0;
    if tui.condx_configured {
        condx_box::render(frame, left[idx], &tui.condx, theme);
        idx += 1;
    }
    debug_assert_eq!(idx, score_idx);
    score_box::render(frame, left[idx], &tui.score, theme);
    idx += 1;
    avail_box::render(frame, left[idx], &tui.avail, theme);
    idx += 1;
    rate_box::render(frame, left[idx], &tui.rate, theme);
    idx += 1;
    if tui.so2r_configured {
        so2r_box::render(
            frame,
            left[idx],
            app.focused_radio,
            tui.tx_radio,
            tui.rx_mode,
            theme,
        );
    }

    // Center: error (top of filler) + log + entry R1 + [entry R2] + scp
    if let Some(ref msg) = tui.error_message {
        let error = ratatui::widgets::Paragraph::new(format!(" {msg}"))
            .style(ratatui::style::Style::from(theme.error_banner));
        frame.render_widget(error, center[0]);
    }
    log_tail::render(frame, center[1], &tui.log_display, &tui.theme);
    let r1_echo = tui.echo_per_radio.get(&1).map(|s| s.as_str());
    entry_line::render(
        frame,
        center[2],
        app,
        1,
        app.focused_radio == 1,
        tui.tx_radio,
        r1_echo,
        tui.cw_transmitting,
        theme,
    );
    if show_r2 {
        let r2_echo = tui.echo_per_radio.get(&2).map(|s| s.as_str());
        entry_line::render(
            frame,
            center[3],
            app,
            2,
            app.focused_radio == 2,
            tui.tx_radio,
            r2_echo,
            tui.cw_transmitting,
            theme,
        );
        status_bar::render_scp(frame, center[4], app, tui);
    } else {
        status_bar::render_scp(frame, center[3], app, tui);
    }

    // Right: bandmap(s)
    match tui.bandmap_mode {
        BandmapMode::Dual => {
            let halves = Layout::vertical([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(cols[2]);
            bandmap::render(frame, halves[0], app, tui, 1);
            bandmap::render(frame, halves[1], app, tui, 2);
        }
        BandmapMode::R1 => bandmap::render(frame, cols[2], app, tui, 1),
        BandmapMode::R2 => bandmap::render(frame, cols[2], app, tui, 2),
    }

    // Status bar
    status_bar::render(frame, rows[1], app, tui);

    // Footer
    let footer = ratatui::widgets::Paragraph::new(
        " F1:CQ  F2:Exch  F3:TU  F5:Call  Esc:Stop  F12:Wipe  Enter:ESM  Ins:Run/S&P  \u{2191}\u{2193}:R1/R2  C-\u{2191}\u{2193}:BM R1  CA-\u{2191}\u{2193}:BM R2  `:RX  C-E:Export  Ctrl-C:Quit",
    )
    .style(ratatui::style::Style::from(theme.footer));
    frame.render_widget(footer, rows[2]);

    // Modal overlay (if open)
    if let Some(ref modal) = tui.export_modal {
        export_modal::render(frame, modal, theme);
    }
}
