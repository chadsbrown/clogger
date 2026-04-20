use std::collections::{HashMap, HashSet};
use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    cursor,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use logger_core::{AppState, CallHistoryLookup, ContestEntry, Effect, Macros, RadioId, ScpLookup, reduce};
use logger_runtime::LogAdapter;
use ratatui::backend::CrosstermBackend;
use tokio::sync::{broadcast, mpsc};
use logger_runtime::{
    CategoryConfig, KeyerEvent, ScoreboardHandle, ScoreboardSnapshot,
    ScoreboardStatus,
};

use crate::TuiState;
use crate::adapters::terminal::TerminalEvent;
use crate::ui;
use crate::ui::log_tail::LogRow;

pub async fn run(
    mut state: AppState,
    contest: Box<dyn ContestEntry>,
    macros: Macros,
    mut log_adapter: LogAdapter,
    rig_txs: HashMap<RadioId, mpsc::Sender<logger_runtime::RigCmd>>,
    keyer_tx: Option<mpsc::Sender<logger_runtime::KeyerCmd>>,
    mut keyer_rx: Option<broadcast::Receiver<KeyerEvent>>,
    cw_echo_enabled: bool,
    cursor_style: crate::config::CursorStyle,
    call_history: std::sync::Arc<dyn CallHistoryLookup>,
    scp: std::sync::Arc<dyn ScpLookup>,
    mut rx: mpsc::Receiver<TerminalEvent>,
    initial_log_display: Vec<LogRow>,
    conn: crate::ConnectionStatus,
    so2r_tx: Option<mpsc::Sender<logger_runtime::So2rCmd>>,
    so2r_default_rx_mode: logger_core::So2rRxMode,
    mut condx_rx: Option<mpsc::Receiver<logger_runtime::CondXSnapshot>>,
    scoreboard: Option<(ScoreboardHandle, &'static str, String, CategoryConfig)>,
    theme: crate::theme::Theme,
) -> Result<()> {
    // RX mode is a runtime knob — the config provides an initial value, and
    // the operator can toggle it mid-session via the backtick keybinding.
    let mut so2r_default_rx_mode = so2r_default_rx_mode;

    // Event-loop latency instrumentation. Records samples unconditionally;
    // summary is emitted at debug level on shutdown (only visible with --debug).
    let mut perf = crate::perf::PerfStats::new();

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

    // Decompose the scoreboard tuple if present
    let (scoreboard_handle, scoreboard_cabrillo_id, scoreboard_call, scoreboard_category) =
        match scoreboard {
            Some((handle, cab_id, call, cat)) => (Some(handle), Some(cab_id), Some(call), Some(cat)),
            None => (None, None, None, None),
        };

    let initial_score = log_adapter.score_summary();
    let mut tui_state = TuiState {
        log_display: initial_log_display,
        score: initial_score,
        rigs: conn.rigs,
        keyer_configured: conn.keyer_configured,
        keyer_connected: conn.keyer_connected,
        dxfeed_configured: conn.dxfeed_configured,
        dxfeed_connected: conn.dxfeed_connected,
        so2r_configured: conn.so2r_configured,
        so2r_connected: conn.so2r_connected,
        scoreboard_configured: conn.scoreboard_configured,
        condx_configured: conn.condx_configured,
        bandmap_mode: conn.bandmap_mode,
        cw_echo_enabled,
        tx_radio: state.focused_radio,
        rx_mode: so2r_default_rx_mode,
        contest_name: contest.contest_name().to_string(),
        theme,
        ..Default::default()
    };

    // Initial OTRSP setup happens in `main.rs` before this function is called
    // — the event loop no longer holds `so2r_switch` directly.

    let mut render_interval = tokio::time::interval(Duration::from_millis(50)); // 20 FPS
    let mut timer_interval = tokio::time::interval(Duration::from_secs(1));

    // Track the last score epoch we shipped a scoreboard snapshot for.
    // Initialized to a sentinel so the first event always sends. See
    // `LogAdapter::score_epoch` for bump semantics.
    let mut last_sent_score_epoch: u64 = u64::MAX;

    let result = loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Some(TerminalEvent::OpenExportModal) => {
                        if tui_state.export_modal.is_none() {
                            tui_state.export_modal = Some(crate::ExportModal {
                                step: crate::ExportStep::SelectFormat,
                            });
                        }
                    }
                    Some(TerminalEvent::ToggleRxMode) => {
                        // Toggle OTRSP RX routing between mono and stereo.
                        // ReverseStereo is config-only and collapses to Stereo
                        // on its first toggle. No-op if OTRSP isn't configured
                        // (so2r_adapter::set_rx swallows the None switch).
                        so2r_default_rx_mode = match so2r_default_rx_mode {
                            logger_core::So2rRxMode::Mono => logger_core::So2rRxMode::Stereo,
                            logger_core::So2rRxMode::Stereo
                            | logger_core::So2rRxMode::ReverseStereo => {
                                logger_core::So2rRxMode::Mono
                            }
                        };
                        tui_state.rx_mode = so2r_default_rx_mode;
                        if let Some(tx) = so2r_tx.as_ref() {
                            let _ = tx.try_send(logger_runtime::So2rCmd::SetRx {
                                radio: state.focused_radio,
                                mode: so2r_default_rx_mode,
                            });
                        }
                    }
                    Some(TerminalEvent::App(app_event)) => {
                        // If the export modal is open, intercept keyboard events
                        if tui_state.export_modal.is_some() {
                            let key_code = match &app_event {
                                logger_core::AppEvent::KeyPress { key } => Some(match key {
                                    logger_core::Key::Enter => crossterm::event::KeyCode::Enter,
                                    logger_core::Key::Esc => crossterm::event::KeyCode::Esc,
                                    logger_core::Key::Backspace => crossterm::event::KeyCode::Backspace,
                                    logger_core::Key::Left => crossterm::event::KeyCode::Left,
                                    logger_core::Key::Right => crossterm::event::KeyCode::Right,
                                    logger_core::Key::Tab => crossterm::event::KeyCode::Tab,
                                    logger_core::Key::Space => crossterm::event::KeyCode::Char(' '),
                                    _ => continue,
                                }),
                                logger_core::AppEvent::TextInput { s } => {
                                    s.chars().next().map(crossterm::event::KeyCode::Char)
                                }
                                _ => None,
                            };
                            if let Some(kc) = key_code {
                                let records = log_adapter.ordered_records();
                                crate::ui::export_modal::handle_key(
                                    &mut tui_state.export_modal,
                                    kc,
                                    &records,
                                    &state.my_call,
                                    contest.contest_id(),
                                );
                                continue;
                            }
                            // Non-keyboard events (RigStatus, spots, etc.) fall through
                        }

                        // Hardware disconnect/error events update the status
                        // bar indicators and the error banner before the
                        // reducer sees them (the reducer treats them as no-ops).
                        match &app_event {
                            logger_core::AppEvent::RigDisconnected { radio } => {
                                tui_state.rigs.insert(*radio, false);
                                let label = if tui_state.rigs.len() > 1 {
                                    format!("RIG{radio}")
                                } else {
                                    "RIG".to_string()
                                };
                                tui_state.error_message = Some(format!("ERROR: {label} CONNECTION LOST"));
                            }
                            logger_core::AppEvent::KeyerDisconnected => {
                                tui_state.keyer_connected = false;
                                tui_state.error_message = Some("ERROR: KEYER CONNECTION LOST".to_string());
                            }
                            logger_core::AppEvent::KeyerError { message } => {
                                tui_state.error_message = Some(format!("KEYER: {message}"));
                            }
                            logger_core::AppEvent::So2rDisconnected => {
                                tui_state.so2r_connected = false;
                                tui_state.error_message = Some("ERROR: SO2R CONNECTION LOST".to_string());
                            }
                            logger_core::AppEvent::So2rError { message } => {
                                tui_state.error_message = Some(format!("SO2R: {message}"));
                            }
                            logger_core::AppEvent::PersistError { message } => {
                                tui_state.error_message = Some(format!("PERSIST: {message}"));
                            }
                            _ => {}
                        }

                        // Capture previous freq/mode so we can detect meaningful
                        // rig changes and skip expensive analytics when nothing moved.
                        let prev_freq_mode = state
                            .radios
                            .get(&state.focused_radio)
                            .map(|r| (r.freq_hz, r.mode.clone()));

                        let needs_analytics = needs_analytics_recompute(&app_event);

                        let rd_start = Instant::now();
                        let effects = reduce(
                            &mut state,
                            contest.as_ref(),
                            &macros,
                            &log_adapter,
                            &log_adapter,
                            call_history.as_ref(),
                            &log_adapter,
                            scp.as_ref(),
                            app_event,
                        );
                        if let Err(e) = dispatch_effects(
                            &effects,
                            &mut state,
                            &mut log_adapter,
                            &mut tui_state,
                            &rig_txs,
                            keyer_tx.as_ref(),
                            so2r_tx.as_ref(),
                            so2r_default_rx_mode,
                        ).await {
                            break Err(e);
                        }
                        perf.reduce_dispatch.record(rd_start.elapsed());

                        // Score is always cheap (cached read).
                        tui_state.score = log_adapter.score_summary();

                        // Update scoreboard snapshot — only rebuild the
                        // expensive `score_breakdown()` when the log
                        // adapter's score epoch has advanced since the
                        // last snapshot we sent. Most events (keystrokes,
                        // rig status updates, spot arrivals) don't touch
                        // the score state; on those, we skip entirely.
                        if let (Some(handle), Some(cab_id), Some(call), Some(cat)) = (
                            &scoreboard_handle,
                            scoreboard_cabrillo_id,
                            &scoreboard_call,
                            &scoreboard_category,
                        ) {
                            let current_epoch = log_adapter.score_epoch();
                            if current_epoch != last_sent_score_epoch {
                                let _ = handle.snapshot_tx.send(Some(ScoreboardSnapshot {
                                    cabrillo_id: cab_id,
                                    call: call.clone(),
                                    ops: call.clone(),
                                    category: cat.clone(),
                                    breakdown: log_adapter.score_breakdown(),
                                }));
                                last_sent_score_epoch = current_epoch;
                            }
                        }

                        // Analytics is expensive — only recompute when state
                        // that affects it has actually changed.
                        if needs_analytics {
                            let a_start = Instant::now();
                            recompute_analytics(&state, &log_adapter, &mut tui_state);
                            perf.analytics.record(a_start.elapsed());
                        } else {
                            // For RigStatus: recompute only if freq or mode changed
                            let cur_freq_mode = state
                                .radios
                                .get(&state.focused_radio)
                                .map(|r| (r.freq_hz, r.mode.clone()));
                            if cur_freq_mode != prev_freq_mode {
                                let a_start = Instant::now();
                                recompute_analytics(&state, &log_adapter, &mut tui_state);
                                perf.analytics.record(a_start.elapsed());
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
                let r_start = Instant::now();
                terminal.draw(|frame| {
                    ui::render(frame, &state, &tui_state);
                })?;
                perf.render.record(r_start.elapsed());
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
                    &log_adapter,
                    scp.as_ref(),
                    logger_core::AppEvent::TimerTick { now_ms },
                );
                if let Err(e) = dispatch_effects(
                    &effects,
                    &mut state,
                    &mut log_adapter,
                    &mut tui_state,
                    &rig_txs,
                    keyer_tx.as_ref(),
                    so2r_tx.as_ref(),
                    so2r_default_rx_mode,
                ).await {
                    break Err(e);
                }
                // Score is cheap; analytics skipped on timer ticks.
                tui_state.score = log_adapter.score_summary();
            }
            status = recv_scoreboard_status(&scoreboard_handle) => {
                tui_state.scoreboard_status = status;
            }
            snapshot = recv_condx(&mut condx_rx) => {
                tui_state.condx = Some(snapshot);
            }
        }
    };

    // Restore terminal
    terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen, cursor::Show, cursor::SetCursorStyle::DefaultUserShape)?;

    // Emit latency percentiles to the log (debug level — only shows with --debug)
    perf.log_summary();

    result
}

async fn dispatch_effects(
    effects: &[Effect],
    state: &mut AppState,
    log_adapter: &mut LogAdapter,
    tui_state: &mut TuiState,
    rig_txs: &HashMap<RadioId, mpsc::Sender<logger_runtime::RigCmd>>,
    keyer_tx: Option<&mpsc::Sender<logger_runtime::KeyerCmd>>,
    so2r_tx: Option<&mpsc::Sender<logger_runtime::So2rCmd>>,
    so2r_default_rx_mode: logger_core::So2rRxMode,
) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::CwSend { radio, text } => {
                // Update TUI state optimistically — the keyer task will
                // actually perform the (possibly cross-radio) send
                // asynchronously. If it needs to abort + switch OTRSP TX
                // + wait for the relay + send, that whole sequence happens
                // inside the keyer task (which holds the Arc<dyn So2rSwitch>
                // clone). The event loop just enqueues the command.
                if *radio != tui_state.tx_radio {
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
                if let Some(tx) = keyer_tx {
                    let _ = tx.try_send(logger_runtime::KeyerCmd::Send {
                        radio: *radio,
                        text: text.clone(),
                    });
                }
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
                // Non-blocking: enqueue a SetFrequency command for the rig's
                // dedicated control task. CI-V roundtrip latency no longer
                // blocks the event loop.
                if let Some(tx) = rig_txs.get(radio) {
                    let _ = tx.try_send(logger_runtime::RigCmd::SetFrequency {
                        hz: *freq_hz,
                    });
                }
            }
            Effect::CwAbort => {
                if let Some(tx) = keyer_tx {
                    let _ = tx.try_send(logger_runtime::KeyerCmd::Abort);
                }
            }
            Effect::UiClearEntry => {
                // State already reflects clear behavior in reducer
            }
            Effect::So2rFocusChanged { radio } => {
                // Entry focus changed: update RX audio to follow the operator's
                // attention. Do NOT change TX routing — that stays where CW is
                // currently being keyed. Non-blocking: the command is enqueued
                // to the so2r task which processes it asynchronously.
                if let Some(tx) = so2r_tx {
                    let _ = tx.try_send(logger_runtime::So2rCmd::SetRx {
                        radio: *radio,
                        mode: so2r_default_rx_mode,
                    });
                }
            }
            Effect::KeyerSetSpeed { wpm } => {
                if let Some(tx) = keyer_tx {
                    let _ = tx.try_send(logger_runtime::KeyerCmd::SetSpeed { wpm: *wpm });
                }
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
        | AppEvent::QuickLog
        | AppEvent::EsmTrigger => true,
        // Radio focus changes which band's analytics are shown
        AppEvent::FocusRadio { .. } | AppEvent::SwapRadios => true,
        // Bandmap cursor navigation / spot selection
        AppEvent::BandmapUp { .. }
        | AppEvent::BandmapDown { .. }
        | AppEvent::BandmapSelect { .. } => true,
        // RigStatus handled by freq/mode comparison in caller
        AppEvent::RigStatus { .. } => false,
        // Timer, disconnect, op-mode, operator changes don't affect analytics.
        // Hardware error/disconnect events are purely TUI status concerns.
        AppEvent::TimerTick { .. }
        | AppEvent::RigDisconnected { .. }
        | AppEvent::KeyerDisconnected
        | AppEvent::KeyerError { .. }
        | AppEvent::So2rDisconnected
        | AppEvent::So2rError { .. }
        | AppEvent::PersistError { .. }
        | AppEvent::SetOpMode { .. }
        | AppEvent::ToggleOpMode
        | AppEvent::SetOperator { .. }
        | AppEvent::CwSpeedAdjust { .. } => false,
    }
}

fn recompute_analytics(state: &AppState, log_adapter: &LogAdapter, tui_state: &mut TuiState) {
    // Each bandmap panel classifies spots against its *own* radio's
    // band+mode. Previously we only computed the focused radio's set,
    // which painted the unfocused radio's bandmap as if every spot
    // were un-worked.
    let mut cache = tui_state.bandmap_cache.borrow_mut();
    let mut worked: HashMap<RadioId, HashSet<String>> = HashMap::new();
    let mut mults: HashMap<RadioId, HashSet<String>> = HashMap::new();
    for (&radio_id, radio) in &state.radios {
        let wc = logger_runtime::compute_worked_calls(
            &state.bandmap,
            state.bandmap_version,
            &mut cache,
            radio.freq_hz,
            radio.mode.as_str(),
            log_adapter,
        );
        worked.insert(radio_id, wc.worked);
        mults.insert(radio_id, wc.mults);
    }

    // Avail still follows the focused radio's mode — it's a single
    // "what's unworked anywhere" panel, not per-radio.
    let focused_mode = state
        .radios
        .get(&state.focused_radio)
        .map(|r| r.mode.as_str())
        .unwrap_or("CW");
    let avail = logger_runtime::compute_avail(
        &state.bandmap,
        state.bandmap_version,
        &mut cache,
        focused_mode,
        log_adapter,
    );
    drop(cache);

    tui_state.worked_calls = worked;
    tui_state.mult_calls = mults;
    tui_state.avail = avail;

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

/// Awaits the next CondX snapshot, or pends forever when the feature
/// is disabled (so `tokio::select!` treats the arm as non-firing).
/// Returns `snapshot` rather than `Option<snapshot>` so the select arm
/// can assign directly without a None-branch in the handler.
async fn recv_condx(
    rx: &mut Option<mpsc::Receiver<logger_runtime::CondXSnapshot>>,
) -> logger_runtime::CondXSnapshot {
    match rx {
        Some(r) => match r.recv().await {
            Some(s) => s,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

async fn recv_scoreboard_status(handle: &Option<ScoreboardHandle>) -> ScoreboardStatus {
    match handle {
        Some(h) => {
            let mut rx = h.status_rx.clone();
            match rx.changed().await {
                Ok(()) => *rx.borrow(),
                Err(_) => std::future::pending().await,
            }
        }
        None => std::future::pending().await,
    }
}
