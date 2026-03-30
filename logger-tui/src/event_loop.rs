use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use logger_core::{AppState, ContestEntry, Effect, Macros, reduce};
use logger_runtime::LogAdapter;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;
use logger_runtime::Keyer;

use crate::TuiState;
use crate::adapters::terminal::TerminalEvent;
use crate::ui;
use crate::ui::log_tail::LogRow;

pub async fn run(
    mut state: AppState,
    contest: Box<dyn ContestEntry>,
    macros: Macros,
    mut log_adapter: LogAdapter,
    keyer: Option<Box<dyn Keyer>>,
    mut rx: mpsc::Receiver<TerminalEvent>,
    initial_log_display: Vec<LogRow>,
) -> Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut tui_state = TuiState {
        log_display: initial_log_display,
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
                            app_event,
                        );
                        if let Err(e) = dispatch_effects(
                            &effects,
                            &mut state,
                            &mut log_adapter,
                            &mut tui_state,
                            keyer.as_deref(),
                        ).await {
                            break Err(e);
                        }
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
                    logger_core::AppEvent::TimerTick { now_ms },
                );
                if let Err(e) = dispatch_effects(
                    &effects,
                    &mut state,
                    &mut log_adapter,
                    &mut tui_state,
                    keyer.as_deref(),
                ).await {
                    break Err(e);
                }
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
            Effect::UiClearEntry => {
                // State already reflects clear behavior in reducer
            }
        }
    }
    Ok(())
}
