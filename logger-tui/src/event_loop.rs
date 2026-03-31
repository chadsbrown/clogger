use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use logger_core::{AppState, CallHistoryLookup, ContestEntry, DupeChecker, Effect, Macros, MultChecker, ScpLookup, contest::{band_freq_range, filtered_bandmap_spots, freq_to_band_label}, reduce};
use logger_runtime::LogAdapter;
use crate::{AvailSummary, RateInfo};
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;
use std::sync::Arc;
use logger_runtime::{Keyer, ReceiverId, Rig};
use tracing::warn;

use crate::TuiState;
use crate::adapters::terminal::TerminalEvent;
use crate::ui;
use crate::ui::log_tail::LogRow;

pub async fn run(
    mut state: AppState,
    contest: Box<dyn ContestEntry>,
    macros: Macros,
    mut log_adapter: LogAdapter,
    rig: Option<Arc<dyn Rig>>,
    keyer: Option<Box<dyn Keyer>>,
    call_history: Box<dyn CallHistoryLookup>,
    scp: Box<dyn ScpLookup>,
    mut rx: mpsc::Receiver<TerminalEvent>,
    initial_log_display: Vec<LogRow>,
    rig_connected: bool,
    keyer_connected: bool,
    dxfeed_connected: bool,
) -> Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let initial_score = log_adapter.score_summary();
    let mut tui_state = TuiState {
        log_display: initial_log_display,
        score: initial_score,
        rig_connected,
        keyer_connected,
        dxfeed_connected,
        ..Default::default()
    };

    let mut render_interval = tokio::time::interval(Duration::from_millis(50)); // 20 FPS
    let mut timer_interval = tokio::time::interval(Duration::from_secs(1));

    let result = loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Some(TerminalEvent::App(app_event)) => {
                        let effects = reduce(
                            &mut state,
                            contest.as_ref(),
                            &macros,
                            &log_adapter,
                            &log_adapter,
                            call_history.as_ref(),
                            scp.as_ref(),
                            app_event,
                        );
                        if let Err(e) = dispatch_effects(
                            &effects,
                            &mut state,
                            &mut log_adapter,
                            &mut tui_state,
                            rig.as_deref(),
                            keyer.as_deref(),
                        ).await {
                            break Err(e);
                        }
                        recompute_worked_calls(&state, &log_adapter, &mut tui_state);
                        recompute_avail(&state, &log_adapter, &mut tui_state);
                        recompute_rate(&log_adapter, &mut tui_state);
                        tui_state.score = log_adapter.score_summary();
                    }
                    Some(TerminalEvent::Shutdown) | None => {
                        break Ok(());
                    }
                }
            }
            _ = render_interval.tick() => {
                terminal.draw(|frame| {
                    ui::render(frame, &state, &tui_state);
                })?;
            }
            _ = timer_interval.tick() => {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let effects = reduce(
                    &mut state,
                    contest.as_ref(),
                    &macros,
                    &log_adapter,
                    &log_adapter,
                    call_history.as_ref(),
                    scp.as_ref(),
                    logger_core::AppEvent::TimerTick { now_ms },
                );
                if let Err(e) = dispatch_effects(
                    &effects,
                    &mut state,
                    &mut log_adapter,
                    &mut tui_state,
                    rig.as_deref(),
                    keyer.as_deref(),
                ).await {
                    break Err(e);
                }
                recompute_worked_calls(&state, &log_adapter, &mut tui_state);
                recompute_avail(&state, &log_adapter, &mut tui_state);
                recompute_rate(&log_adapter, &mut tui_state);
                tui_state.score = log_adapter.score_summary();
            }
        }
    };

    // Restore terminal
    terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;

    result
}

async fn dispatch_effects(
    effects: &[Effect],
    state: &mut AppState,
    log_adapter: &mut LogAdapter,
    tui_state: &mut TuiState,
    rig: Option<&dyn Rig>,
    keyer: Option<&dyn Keyer>,
) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::CwSend { radio: _, text } => {
                tui_state.cw_history.push(text.clone());
                logger_runtime::send_cw(keyer, text).await;
            }
            Effect::LogInsert { draft } => {
                let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                let id = log_adapter.insert(
                    draft.clone(),
                    now_ms,
                    state.focused_radio as u32,
                    state.active_operator as u32,
                )?;
                state.last_logged = Some(id);

                // Add to display log
                let exchange_str = draft
                    .exchange_pairs
                    .iter()
                    .map(|(_, v)| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                tui_state.log_display.push(LogRow {
                    nr: tui_state.log_display.len() as u64 + 1,
                    call: draft.callsign.clone(),
                    band: draft.band.clone(),
                    mode: draft.mode.clone(),
                    exchange: exchange_str,
                });
            }
            Effect::Beep { kind: _ } => {
                // Terminal bell
                print!("\x07");
            }
            Effect::UiSetFocus { field_id } => {
                if let Some(idx) = state
                    .entry
                    .fields
                    .iter()
                    .position(|f| f.field_id == *field_id)
                {
                    state.entry.focus = idx;
                }
            }
            Effect::RigSet { radio, freq_hz } => {
                if let Some(rig) = rig {
                    let rx = ReceiverId::from_index((*radio - 1) as u8);
                    if let Err(e) = rig.set_frequency(rx, *freq_hz).await {
                        warn!("rig set_frequency failed: {e}");
                    }
                }
            }
            Effect::UiClearEntry => {
                // State already reflects clear behavior in reducer
            }
        }
    }
    Ok(())
}

fn recompute_worked_calls(state: &AppState, log_adapter: &LogAdapter, tui_state: &mut TuiState) {
    tui_state.worked_calls.clear();
    tui_state.mult_calls.clear();
    let Some(radio) = state.radios.get(&state.focused_radio).filter(|r| r.freq_hz > 0) else {
        return;
    };
    let band = freq_to_band_label(radio.freq_hz);
    let mode = &radio.mode;
    for spot in filtered_bandmap_spots(&state.bandmap, &band, mode) {
        let call_norm = spot.call.to_ascii_uppercase();
        if log_adapter.is_dupe(&call_norm, &band, mode) {
            tui_state.worked_calls.insert(spot.call);
        } else if log_adapter.is_new_mult(&call_norm, &band, mode) {
            tui_state.mult_calls.insert(spot.call);
        }
    }
}

const BANDS: &[&str] = &["160m", "80m", "40m", "20m", "15m", "10m"];

fn recompute_avail(state: &AppState, log_adapter: &LogAdapter, tui_state: &mut TuiState) {
    let mode = state
        .radios
        .get(&state.focused_radio)
        .map(|r| r.mode.as_str())
        .unwrap_or("CW");

    let mut by_band = Vec::new();
    let mut total_qsos = 0u32;
    let mut total_mults = 0u32;
    for &band in BANDS {
        let (min, _max) = band_freq_range(band);
        if min == 0 {
            continue;
        }
        let spots = filtered_bandmap_spots(&state.bandmap, band, mode);
        let mut qsos = 0u32;
        let mut mults = 0u32;
        for s in &spots {
            let call_norm = s.call.to_ascii_uppercase();
            if !log_adapter.is_dupe(&call_norm, band, mode) {
                qsos += 1;
                if log_adapter.is_new_mult(&call_norm, band, mode) {
                    mults += 1;
                }
            }
        }
        if qsos > 0 {
            by_band.push((band.to_string(), qsos, mults));
            total_qsos += qsos;
            total_mults += mults;
        }
    }
    tui_state.avail = AvailSummary { by_band, total_qsos, total_mults };
}

fn recompute_rate(log_adapter: &LogAdapter, tui_state: &mut TuiState) {
    let records = log_adapter.ordered_records();
    let timestamps: Vec<u64> = records
        .iter()
        .filter(|r| !r.flags.is_void)
        .map(|r| r.ts_ms)
        .collect();

    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let count = timestamps.len();

    let last_10_minutes = if count >= 10 {
        let t = timestamps[count - 10];
        Some((now_ms.saturating_sub(t)) as f64 / 60_000.0)
    } else {
        None
    };

    let last_100_minutes = if count >= 100 {
        let t = timestamps[count - 100];
        Some((now_ms.saturating_sub(t)) as f64 / 60_000.0)
    } else {
        None
    };

    let rate_per_hour = match last_10_minutes {
        Some(mins) if mins > 0.0 => (10.0 / mins * 60.0) as u32,
        _ => 0,
    };

    tui_state.rate = RateInfo {
        last_10_minutes,
        last_100_minutes,
        rate_per_hour,
    };
}
