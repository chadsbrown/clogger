use std::collections::HashMap;
use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use logger_core::{AppState, CallHistoryLookup, ContestEntry, Effect, Macros, RadioId, ScpLookup, reduce};
use logger_runtime::LogAdapter;
use ratatui::backend::CrosstermBackend;
use tokio::sync::{broadcast, mpsc};
use std::sync::Arc;
use logger_runtime::{Keyer, KeyerEvent, ReceiverId, Rig, So2rSwitch};
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
    rigs: HashMap<RadioId, Arc<dyn Rig>>,
    keyer: Option<Box<dyn Keyer>>,
    mut keyer_rx: Option<broadcast::Receiver<KeyerEvent>>,
    cw_echo_enabled: bool,
    cursor_style: crate::config::CursorStyle,
    call_history: Box<dyn CallHistoryLookup>,
    scp: Box<dyn ScpLookup>,
    mut rx: mpsc::Receiver<TerminalEvent>,
    initial_log_display: Vec<LogRow>,
    conn: crate::ConnectionStatus,
    so2r_switch: Option<Box<dyn So2rSwitch>>,
    so2r_default_rx_mode: logger_core::So2rRxMode,
) -> Result<()> {
    // Setup terminal
    terminal::enable_raw_mode()?;
    let cs = match cursor_style {
        crate::config::CursorStyle::BlinkingBlock => cursor::SetCursorStyle::BlinkingBlock,
        crate::config::CursorStyle::SteadyBlock => cursor::SetCursorStyle::SteadyBlock,
        crate::config::CursorStyle::BlinkingUnderline => cursor::SetCursorStyle::BlinkingUnderScore,
        crate::config::CursorStyle::SteadyUnderline => cursor::SetCursorStyle::SteadyUnderScore,
        crate::config::CursorStyle::BlinkingBar => cursor::SetCursorStyle::BlinkingBar,
        crate::config::CursorStyle::SteadyBar => cursor::SetCursorStyle::SteadyBar,
    };
    crossterm::execute!(io::stdout(), EnterAlternateScreen, cs)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let initial_score = log_adapter.score_summary();
    let mut tui_state = TuiState {
        log_display: initial_log_display,
        score: initial_score,
        rig_configured: conn.rig_configured,
        rig_connected: conn.rig_connected,
        keyer_configured: conn.keyer_configured,
        keyer_connected: conn.keyer_connected,
        dxfeed_configured: conn.dxfeed_configured,
        dxfeed_connected: conn.dxfeed_connected,
        so2r_configured: conn.so2r_configured,
        so2r_connected: conn.so2r_connected,
        cw_echo_enabled,
        tx_radio: state.focused_radio,
        ..Default::default()
    };

    // Initial OTRSP setup: route TX and RX to the focused radio at startup so
    // the box is in a known state before any CW is sent.
    logger_runtime::so2r_adapter::set_tx(so2r_switch.as_deref(), state.focused_radio).await;
    logger_runtime::so2r_adapter::set_rx(so2r_switch.as_deref(), state.focused_radio, so2r_default_rx_mode).await;

    let mut render_interval = tokio::time::interval(Duration::from_millis(50)); // 20 FPS
    let mut timer_interval = tokio::time::interval(Duration::from_secs(1));

    let result = loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Some(TerminalEvent::App(app_event)) => {
                        if matches!(&app_event, logger_core::AppEvent::RigDisconnected { .. }) {
                            tui_state.rig_connected = false;
                            tui_state.error_message = Some("ERROR: RIG CONNECTION LOST".to_string());
                        }

                        // Capture previous freq/mode so we can detect meaningful
                        // rig changes and skip expensive analytics when nothing moved.
                        let prev_freq_mode = state
                            .radios
                            .get(&state.focused_radio)
                            .map(|r| (r.freq_hz, r.mode.clone()));

                        let needs_analytics = needs_analytics_recompute(&app_event);

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
                            &rigs,
                            keyer.as_deref(),
                            so2r_switch.as_deref(),
                            so2r_default_rx_mode,
                        ).await {
                            break Err(e);
                        }

                        // Score is always cheap (cached read).
                        tui_state.score = log_adapter.score_summary();

                        // Analytics is expensive — only recompute when state
                        // that affects it has actually changed.
                        if needs_analytics {
                            recompute_analytics(&state, &log_adapter, &mut tui_state);
                        } else {
                            // For RigStatus: recompute only if freq or mode changed
                            let cur_freq_mode = state
                                .radios
                                .get(&state.focused_radio)
                                .map(|r| (r.freq_hz, r.mode.clone()));
                            if cur_freq_mode != prev_freq_mode {
                                recompute_analytics(&state, &log_adapter, &mut tui_state);
                            }
                        }
                    }
                    Some(TerminalEvent::Shutdown) | None => {
                        break Ok(());
                    }
                }
            }
            ev = recv_keyer_event(&mut keyer_rx) => {
                match ev {
                    KeyerEvent::CharacterSent(ch) => {
                        // Append to the currently-keying radio's echo buffer
                        tui_state
                            .echo_per_radio
                            .entry(tui_state.tx_radio)
                            .or_default()
                            .push(ch);
                    }
                    KeyerEvent::StatusChanged(status) if !status.busy => {
                        tui_state.cw_transmitting = false;
                    }
                    _ => {}
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
                    &rigs,
                    keyer.as_deref(),
                    so2r_switch.as_deref(),
                    so2r_default_rx_mode,
                ).await {
                    break Err(e);
                }
                // Score is cheap; analytics skipped on timer ticks.
                tui_state.score = log_adapter.score_summary();
            }
        }
    };

    // Restore terminal
    terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen, cursor::Show, cursor::SetCursorStyle::DefaultUserShape)?;

    result
}

async fn dispatch_effects(
    effects: &[Effect],
    state: &mut AppState,
    log_adapter: &mut LogAdapter,
    tui_state: &mut TuiState,
    rigs: &HashMap<RadioId, Arc<dyn Rig>>,
    keyer: Option<&dyn Keyer>,
    so2r_switch: Option<&dyn So2rSwitch>,
    so2r_default_rx_mode: logger_core::So2rRxMode,
) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::CwSend { radio, text } => {
                // If TX is currently routed to a different radio, abort the
                // in-flight CW and switch OTRSP before starting the new one.
                if *radio != tui_state.tx_radio {
                    logger_runtime::abort_cw(keyer).await;
                    logger_runtime::so2r_adapter::set_tx(so2r_switch, *radio).await;
                    tui_state.tx_radio = *radio;
                }
                tui_state.cw_transmitting = true;
                // Initialize the echo buffer for this radio. With live echo
                // enabled, start empty and let CharacterSent events fill it.
                // Without live echo, populate immediately with the stripped
                // (no speed markers) text since we'll get no per-character feedback.
                if tui_state.cw_echo_enabled {
                    tui_state.echo_per_radio.insert(*radio, String::new());
                } else {
                    tui_state
                        .echo_per_radio
                        .insert(*radio, strip_speed_markers(text));
                }
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
                    .focused_entry()
                    .fields
                    .iter()
                    .position(|f| f.field_id == *field_id)
                {
                    state.focused_entry_mut().focus = idx;
                }
            }
            Effect::RigSet { radio, freq_hz } => {
                if let Some(rig) = rigs.get(radio) {
                    // Each rig adapter is bound to a specific radio_id, so we
                    // address its primary receiver (index 0).
                    let rx = ReceiverId::from_index(0);
                    if let Err(e) = rig.set_frequency(rx, *freq_hz).await {
                        warn!("rig set_frequency failed: {e}");
                    }
                }
            }
            Effect::CwAbort => {
                logger_runtime::abort_cw(keyer).await;
            }
            Effect::UiClearEntry => {
                // State already reflects clear behavior in reducer
            }
            Effect::So2rFocusChanged { radio } => {
                // Entry focus changed: update RX audio to follow the operator's
                // attention. Do NOT change TX routing — that stays where CW is
                // currently being keyed.
                logger_runtime::so2r_adapter::set_rx(so2r_switch, *radio, so2r_default_rx_mode).await;
            }
        }
    }
    Ok(())
}

/// Strip `{NN}` speed control markers from CW text. The markers are inserted by
/// macro expansion (e.g., from `<` and `>` speed shift operators) and translated
/// by the keyer into WK speed-change commands. They never appear in the keyer's
/// echo stream, so they shouldn't appear in any displayed CW either.
///
/// Prosigns like `<AR>` use angle brackets and are preserved.
fn strip_speed_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            // Skip until matching `}` (or end of string).
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Returns true if this event type always requires analytics recomputation.
/// RigStatus is handled separately (only recompute if freq/mode changed).
fn needs_analytics_recompute(event: &logger_core::AppEvent) -> bool {
    use logger_core::AppEvent;
    match event {
        // Bandmap changes affect worked/mult/avail displays
        AppEvent::SpotReceived { .. } | AppEvent::SpotWithdrawn { .. } => true,
        // Keyboard/ESM may log a QSO or change entry fields
        AppEvent::KeyPress { .. }
        | AppEvent::TextInput { .. }
        | AppEvent::EsmTrigger => true,
        // Radio focus changes which band's analytics are shown
        AppEvent::FocusRadio { .. } | AppEvent::SwapRadios => true,
        // Bandmap cursor navigation
        AppEvent::BandmapUp | AppEvent::BandmapDown => true,
        // RigStatus handled by freq/mode comparison in caller
        AppEvent::RigStatus { .. } => false,
        // Timer, disconnect, op-mode, operator changes don't affect analytics
        AppEvent::TimerTick { .. }
        | AppEvent::RigDisconnected { .. }
        | AppEvent::SetOpMode { .. }
        | AppEvent::ToggleOpMode
        | AppEvent::SetOperator { .. } => false,
    }
}

fn recompute_analytics(state: &AppState, log_adapter: &LogAdapter, tui_state: &mut TuiState) {
    let (freq_hz, mode) = state
        .radios
        .get(&state.focused_radio)
        .map(|r| (r.freq_hz, r.mode.as_str()))
        .unwrap_or((0, "CW"));

    let wc = logger_runtime::compute_worked_calls(&state.bandmap, freq_hz, mode, log_adapter);
    tui_state.worked_calls = wc.worked;
    tui_state.mult_calls = wc.mults;

    tui_state.avail = logger_runtime::compute_avail(&state.bandmap, mode, log_adapter);

    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    tui_state.rate = logger_runtime::compute_rate(log_adapter, now_ms);
}

async fn recv_keyer_event(rx: &mut Option<broadcast::Receiver<KeyerEvent>>) -> KeyerEvent {
    match rx {
        Some(rx) => loop {
            match rx.recv().await {
                Ok(ev) => return ev,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => std::future::pending().await,
            }
        },
        None => std::future::pending().await,
    }
}
