mod bridge;
mod config;
mod keys;
mod mdi;
mod modals;
mod panes;

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use clap::Parser;
use iced::{event, keyboard, Element, Event, Point, Size, Subscription, Theme};
use logger_core::{AppEvent, Effect, RadioId};
use logger_runtime::{CondXSnapshot, KeyerEvent, RigConfig, ScoreboardStatus, Session};

use bridge::AdapterHandles;


/// Stash the parsed config between `main()` and `init()` (which iced calls
/// inside its tokio runtime).
static PENDING_CONFIG: OnceLock<Mutex<Option<config::Config>>> = OnceLock::new();

struct App {
    workspace: mdi::Workspace,
    session: Session,
    handles: AdapterHandles,
    /// CW echo buffer per radio, rendered inline under each radio's entry
    /// fields. When `cw_echo = true` in the keyer config, this is filled
    /// character-by-character from `KeyerEvent::CharacterSent`; otherwise
    /// `Effect::CwSend` pre-populates with the stripped macro text so the
    /// operator sees what's being keyed even without live echo.
    echo_per_radio: std::collections::HashMap<RadioId, String>,
    /// Latest CondX snapshot, when condx polling is enabled.
    condx: Option<CondXSnapshot>,
    /// Which radio the keyer is currently keying (independent of entry
    /// focus). Updated from `Effect::CwSend` in `observe_effects`.
    tx_radio: RadioId,
    /// True while the keyer reports a CW send in flight. Set optimistically
    /// on `Effect::CwSend`, cleared on `KeyerEvent::StatusChanged { busy: false }`.
    /// Drives the red TX border on the transmitting radio's entry section.
    cw_transmitting: bool,
    /// Per-radio bandmap zoom factor. Lives on the host rather than inside
    /// the Canvas's `Program::State` because iced's widget-tree diff ties
    /// canvas state to tree position — any pane click that bumps z-order
    /// rebuilds the tree and would otherwise reset zoom to 1.0.
    bandmap_zoom: std::collections::HashMap<RadioId, f32>,
    pane_entry: u32,
    pane_bandmap_r1: u32,
    pane_bandmap_r2: u32,
    pane_log: u32,
    pane_score: u32,
    pane_rate: u32,
    pane_avail: u32,
    pane_macros: u32,
    pane_so2r: u32,
    pane_condx: u32,
    pane_scp: u32,
    theme: Theme,
    modal: Option<modals::Modal>,
    /// Sticky error banner. Set by `AppEvent::*Error` / `*Disconnected`;
    /// cleared by the user (✕ on the banner).
    error_banner: Option<String>,
    /// False until `spawn_adapters` completes and `Message::AdaptersReady`
    /// fires. Subscriptions that depend on receivers stashed by
    /// `spawn_adapters` (condx, keyer echo, scoreboard status) are only
    /// registered after this flips — otherwise they'd activate before the
    /// receivers exist, see `None`, and park in `pending` forever.
    adapters_ready: bool,
    /// Global UI scale factor. Applied via `iced::application::scale_factor`,
    /// so every widget — text, padding, borders, the bandmap canvas — grows
    /// or shrinks together. Persisted alongside the layout.
    font_scale: f32,
    /// Rig configs kept around for auto-reconnect after
    /// `AppEvent::RigDisconnected`. The rig adapter itself doesn't retry
    /// (it just exits on disconnect), so the GUI supervises by re-spawning
    /// with the saved config on a back-off schedule.
    rig_configs: std::collections::HashMap<RadioId, RigConfig>,
    /// Clone of the `AppEvent` sender that adapter tasks publish to.
    /// Needed so `Message::RigReconnectAttempt` can hand a fresh sender
    /// to a re-spawned `spawn_rig_adapter` task.
    app_tx: tokio::sync::mpsc::Sender<AppEvent>,
}

const FONT_SCALE_MIN: f32 = 0.6;
const FONT_SCALE_MAX: f32 = 2.0;
const FONT_SCALE_STEP: f32 = 0.1;
/// Delay between rig-reconnect attempts. Long enough to not hammer a
/// stuck serial port; short enough that flipping a radio off/on doesn't
/// feel unresponsive.
const RIG_RECONNECT_DELAY: Duration = Duration::from_secs(5);

impl App {
    fn new(session: Session, app_tx: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        let mut workspace = mdi::Workspace::default();
        mdi::load(&mut workspace);
        if workspace.panes.is_empty() {
            workspace.add("Entry", Point::new(40.0, 40.0), Size::new(440.0, 260.0));
            workspace.add("Bandmap R1", Point::new(500.0, 40.0), Size::new(340.0, 340.0));
            workspace.add("Bandmap R2", Point::new(860.0, 40.0), Size::new(340.0, 340.0));
            workspace.add("Log", Point::new(40.0, 320.0), Size::new(440.0, 240.0));
            workspace.add("Score", Point::new(500.0, 400.0), Size::new(320.0, 220.0));
            workspace.add("Rate", Point::new(840.0, 400.0), Size::new(320.0, 160.0));
            workspace.add("Available", Point::new(500.0, 640.0), Size::new(320.0, 220.0));
            workspace.add("Macros", Point::new(40.0, 580.0), Size::new(440.0, 200.0));
            workspace.add("SO2R", Point::new(840.0, 580.0), Size::new(320.0, 180.0));
            workspace.add("CondX", Point::new(1180.0, 40.0), Size::new(280.0, 340.0));
            workspace.add("SCP", Point::new(40.0, 800.0), Size::new(440.0, 160.0));
        }
        let pane_entry = workspace.id_by_title("Entry").unwrap_or_else(|| {
            workspace.add("Entry", Point::new(40.0, 40.0), Size::new(440.0, 260.0))
        });
        // Migration: older layouts used "Bandmap" for R1.
        let pane_bandmap_r1 = workspace
            .id_by_title("Bandmap R1")
            .or_else(|| workspace.id_by_title("Bandmap"))
            .unwrap_or_else(|| {
                workspace.add("Bandmap R1", Point::new(500.0, 40.0), Size::new(340.0, 340.0))
            });
        let pane_bandmap_r2 = workspace.id_by_title("Bandmap R2").unwrap_or_else(|| {
            workspace.add("Bandmap R2", Point::new(860.0, 40.0), Size::new(340.0, 340.0))
        });
        let pane_log = workspace.id_by_title("Log").unwrap_or_else(|| {
            workspace.add("Log", Point::new(40.0, 320.0), Size::new(440.0, 240.0))
        });
        let pane_score = workspace.id_by_title("Score").unwrap_or_else(|| {
            workspace.add("Score", Point::new(500.0, 400.0), Size::new(320.0, 220.0))
        });
        let pane_rate = workspace.id_by_title("Rate").unwrap_or_else(|| {
            workspace.add("Rate", Point::new(840.0, 400.0), Size::new(320.0, 160.0))
        });
        let pane_avail = workspace.id_by_title("Available").unwrap_or_else(|| {
            workspace.add("Available", Point::new(500.0, 640.0), Size::new(320.0, 220.0))
        });
        let pane_macros = workspace.id_by_title("Macros").unwrap_or_else(|| {
            workspace.add("Macros", Point::new(40.0, 580.0), Size::new(440.0, 200.0))
        });
        // Drop the old standalone "CW" pane if a previous layout had one —
        // echo now renders inline under each radio's entry.
        workspace.panes.retain(|p| p.title != "CW");
        let pane_so2r = workspace.id_by_title("SO2R").unwrap_or_else(|| {
            workspace.add("SO2R", Point::new(840.0, 580.0), Size::new(320.0, 180.0))
        });
        let pane_condx = workspace.id_by_title("CondX").unwrap_or_else(|| {
            workspace.add("CondX", Point::new(1180.0, 40.0), Size::new(280.0, 340.0))
        });
        let pane_scp = workspace.id_by_title("SCP").unwrap_or_else(|| {
            workspace.add("SCP", Point::new(40.0, 800.0), Size::new(440.0, 160.0))
        });
        let tx_radio = session.state.focused_radio;
        let bandmap_zoom = std::collections::HashMap::new();
        Self {
            workspace,
            session,
            handles: AdapterHandles::default(),
            echo_per_radio: std::collections::HashMap::new(),
            condx: None,
            tx_radio,
            cw_transmitting: false,
            bandmap_zoom,
            pane_entry,
            pane_bandmap_r1,
            pane_bandmap_r2,
            pane_log,
            pane_score,
            pane_rate,
            pane_avail,
            pane_macros,
            pane_so2r,
            pane_condx,
            pane_scp,
            theme: resolve_saved_theme().unwrap_or(Theme::Dark),
            modal: None,
            error_banner: None,
            adapters_ready: false,
            font_scale: mdi::load_font_scale().unwrap_or(1.0).clamp(FONT_SCALE_MIN, FONT_SCALE_MAX),
            rig_configs: std::collections::HashMap::new(),
            app_tx,
        }
    }

    /// Capture observable side effects before they hit `dispatch` so
    /// host-only state (like the inline CW echo buffers) reacts to them.
    fn observe_effects(&mut self, effects: &[Effect]) {
        for ef in effects {
            if let Effect::CwSend { radio, text } = ef {
                self.tx_radio = *radio;
                self.cw_transmitting = true;
                if self.handles.cw_echo_enabled {
                    // Live-echo mode: clear the buffer and let
                    // `KeyerEvent::CharacterSent` fill it as the keyer
                    // actually sends characters over the wire.
                    self.echo_per_radio.insert(*radio, String::new());
                } else {
                    // No live echo — pre-populate with the full stripped
                    // text so the operator still sees what's being keyed.
                    self.echo_per_radio
                        .insert(*radio, strip_speed_markers(text));
                }
            }
        }
    }

    fn show_r2(&self) -> bool {
        // Show the R2 entry when either a second rig is configured or the
        // demo session has been focused on R2 at some point. Simpler: always
        // show R2 when two radios are wired; otherwise show only on focus.
        self.handles.rig_status.len() >= 2 || self.session.state.focused_radio == 2
    }
}

#[derive(Debug, Clone)]
enum Message {
    Mdi(mdi::Message),
    Tick,
    Domain(AppEvent),
    /// Bandmap click. `call = Some(_)` when the click landed within snap
    /// distance of an actual spot — that path populates the entry box and
    /// dispatches a real `BandmapSelect`. `call = None` when clicking empty
    /// space — that path just tunes (synthesizes `RigStatus` if no rig).
    BandmapClicked {
        radio: RadioId,
        call: Option<String>,
        freq_hz: u64,
    },
    /// Scroll-wheel zoom on a bandmap canvas. Radio-keyed so R1 and R2
    /// retain independent zoom levels.
    BandmapZoom {
        radio: RadioId,
        zoom: f32,
    },
    /// Retry connecting to a rig after a prior `RigDisconnected`. Scheduled
    /// automatically by the `RigDisconnected` handler with a back-off; the
    /// Task::perform payload holds the result of `spawn_rig_adapter`.
    RigReconnected {
        radio: RadioId,
        result: Result<tokio::sync::mpsc::Sender<logger_runtime::RigCmd>, String>,
    },
    SetTheme(Theme),
    ToggleSo2rRxMode,
    OpenModal(modals::Modal),
    CloseModal,
    AdaptersReady(AdapterHandlesResult),
    KeyerEvent(KeyerEvent),
    CondxSnapshot(CondXSnapshot),
    ScoreboardStatus(ScoreboardStatus),
    RtcStatus(logger_runtime::rtc_adapter::RtcStatus),
    DismissError,
    FontScaleUp,
    FontScaleDown,
    FontScaleReset,
}

/// Wrapper because `Result<AdapterHandles, anyhow::Error>` isn't Clone
/// (anyhow::Error isn't Clone). We carry the error as a plain string.
#[derive(Debug, Clone)]
enum AdapterHandlesResult {
    Ok(std::sync::Arc<std::sync::Mutex<Option<AdapterHandles>>>),
    Err(String),
}

fn update(state: &mut App, msg: Message) -> iced::Task<Message> {
    // Rig reconnect scheduling is the only code path that needs to return
    // a `Task` from `update()`; every other arm is fine with `()` → none.
    // Capture the task in a local, run the match, then return at the end.
    let mut pending: iced::Task<Message> = iced::Task::none();

    match msg {
        Message::Mdi(m) => {
            state.workspace.update(m);
            mdi::save(
                &state.workspace,
                state.font_scale,
                &state.theme.to_string(),
            );
        }
        Message::Tick => {
            let now = chrono::Utc::now().timestamp_millis();
            let effects = {
                bridge::step(&mut state.session, AppEvent::TimerTick { now_ms: now })
            };
            state.observe_effects(&effects);
            {
                bridge::dispatch(&mut state.session, effects, &state.handles);
            }
            // Scoreboard/RTC pushes previously happened here. Now that
            // LogAdapter publishes snapshots directly to the
            // bootstrap-hosted adapters, there's nothing to do here.
        }
        Message::Domain(ev) => {
            // Modal swallows Esc so a key-press focused on modal dismissal
            // doesn't also ripple into the reducer (which would, for
            // instance, clear the entry box).
            if state.modal.is_some()
                && matches!(&ev, AppEvent::KeyPress { key: logger_core::Key::Esc })
            {
                state.modal = None;
                return iced::Task::none();
            }
            // Disconnect/error events flow through the reducer as no-ops,
            // but the GUI must surface them — update the status-bar
            // indicators and the error banner here, before dispatching.
            match &ev {
                AppEvent::RigDisconnected { radio } => {
                    state.handles.rig_status.insert(*radio, false);
                    state.error_banner = Some(format!("RIG{radio}: connection lost"));
                    // Schedule an automatic reconnect if we have the config
                    // saved. Adapter itself doesn't retry — the GUI supervises
                    // by re-spawning with the saved config on a back-off.
                    if let Some(cfg) = state.rig_configs.get(radio).cloned() {
                        pending = schedule_rig_reconnect(*radio, cfg, state.app_tx.clone());
                    }
                }
                AppEvent::KeyerDisconnected => {
                    state.handles.keyer_connected = false;
                    state.error_banner = Some("KEYER: connection lost".to_string());
                }
                AppEvent::KeyerError { message } => {
                    state.error_banner = Some(format!("KEYER: {message}"));
                }
                AppEvent::So2rDisconnected => {
                    state.handles.so2r_connected = false;
                    state.error_banner = Some("SO2R: connection lost".to_string());
                }
                AppEvent::So2rError { message } => {
                    state.error_banner = Some(format!("SO2R: {message}"));
                }
                AppEvent::PersistError { message } => {
                    state.error_banner = Some(format!("LOG: {message}"));
                }
                _ => {}
            }
            let effects = {
                bridge::step(&mut state.session, ev)
            };
            state.observe_effects(&effects);
            {
                bridge::dispatch(&mut state.session, effects, &state.handles);
            }
        }
        Message::OpenModal(m) => state.modal = Some(m),
        Message::CloseModal => state.modal = None,
        Message::SetTheme(t) => {
            state.theme = t;
            mdi::save(
                &state.workspace,
                state.font_scale,
                &state.theme.to_string(),
            );
        }
        Message::BandmapClicked { radio, call, freq_hz } => {
            // Bring entry focus to whichever radio's bandmap was clicked
            // (SO2R ergonomic — clicking a spot on R2 should also focus R2).
            if state.session.state.focused_radio != radio {
                let effects =
                    bridge::step(&mut state.session, AppEvent::FocusRadio { radio });
                state.observe_effects(&effects);
                bridge::dispatch(&mut state.session, effects, &state.handles);
            }

            if let Some(call) = call {
                // Click landed on a spot. Use the reducer's BandmapSelect
                // path: it sets the cursor, populates CALL, switches to S&P,
                // applies history autopopulate, and emits `Effect::RigSet`.
                let effects = bridge::step(
                    &mut state.session,
                    AppEvent::BandmapSelect {
                        radio,
                        call,
                        freq_hz,
                    },
                );
                state.observe_effects(&effects);
                bridge::dispatch(&mut state.session, effects, &state.handles);
                // If no real rig adapter, synthesize a RigStatus so the
                // bandmap cursor visibly tracks the click.
                if !state.handles.rig_txs.contains_key(&radio) {
                    synthesize_rig_status(state, radio, freq_hz);
                }
            } else {
                // Click in empty space — tune without touching the entry.
                if state.handles.rig_txs.contains_key(&radio) {
                    let ef = vec![Effect::RigSet { radio, freq_hz }];
                    bridge::dispatch(&mut state.session, ef, &state.handles);
                } else {
                    synthesize_rig_status(state, radio, freq_hz);
                }
            }
        }
        Message::BandmapZoom { radio, zoom } => {
            state.bandmap_zoom.insert(radio, zoom);
        }
        Message::ToggleSo2rRxMode => {
            use logger_core::So2rRxMode;
            // Mono → Stereo, Stereo|ReverseStereo → Mono (matches TUI).
            state.handles.so2r_default_rx_mode = match state.handles.so2r_default_rx_mode {
                So2rRxMode::Mono => So2rRxMode::Stereo,
                So2rRxMode::Stereo | So2rRxMode::ReverseStereo => So2rRxMode::Mono,
            };
            if let Some(tx) = &state.handles.so2r_tx {
                let _ = tx.try_send(logger_runtime::So2rCmd::SetRx {
                    radio: state.session.state.focused_radio,
                    mode: state.handles.so2r_default_rx_mode,
                });
            }
        }
        Message::AdaptersReady(result) => match result {
            AdapterHandlesResult::Ok(slot) => {
                if let Some(h) = slot.lock().ok().and_then(|mut g| g.take()) {
                    tracing::info!(
                        rigs = h.rig_status.len(),
                        keyer = h.keyer_connected,
                        dxfeed = h.dxfeed_connected,
                        so2r = h.so2r_connected,
                        condx = h.condx_configured,
                        scoreboard = h.scoreboard_configured,
                        "adapters ready"
                    );
                    state.handles = h;
                    // Flipping this flag re-runs `subscription()` and adds
                    // the condx/keyer/scoreboard streams now that their
                    // OnceLock receivers are populated.
                    state.adapters_ready = true;
                }
            }
            AdapterHandlesResult::Err(e) => {
                tracing::error!(err = %e, "adapter setup failed; running without hardware");
                state.error_banner = Some(format!("ADAPTERS: {e}"));
            }
        },
        Message::KeyerEvent(ev) => match ev {
            KeyerEvent::CharacterSent(c) => {
                // The keyer task routes TX to `tx_radio`; append the echoed
                // character to that radio's buffer. observe_effects already
                // cleared the buffer at the start of this send.
                state
                    .echo_per_radio
                    .entry(state.tx_radio)
                    .or_default()
                    .push(c);
            }
            KeyerEvent::Connected => {
                state.handles.keyer_connected = true;
            }
            KeyerEvent::Disconnected => {
                state.handles.keyer_connected = false;
            }
            KeyerEvent::StatusChanged(status) => {
                if !status.busy {
                    state.cw_transmitting = false;
                }
            }
            _ => {}
        },
        Message::CondxSnapshot(snap) => {
            tracing::debug!(sfi = ?snap.sfi, "condx snapshot");
            state.condx = Some(snap);
        }
        Message::ScoreboardStatus(s) => {
            state.handles.scoreboard_status = s;
            if matches!(s, ScoreboardStatus::Failing) {
                state.error_banner =
                    Some("SCOREBOARD: posting failing — check endpoints".to_string());
            }
        }
        Message::RtcStatus(s) => {
            state.handles.rtc_status = s;
            use logger_runtime::rtc_adapter::RtcStatus;
            match s {
                RtcStatus::Failing => {
                    state.error_banner =
                        Some("RTC: posting failing — check endpoint / credentials".to_string());
                }
                RtcStatus::Unsupported => {
                    state.error_banner = Some(
                        "RTC: server reports this contest as unsupported — contact admin"
                            .to_string(),
                    );
                }
                _ => {}
            }
        }
        Message::DismissError => {
            state.error_banner = None;
        }
        Message::FontScaleUp => {
            state.font_scale =
                (state.font_scale + FONT_SCALE_STEP).clamp(FONT_SCALE_MIN, FONT_SCALE_MAX);
            mdi::save(
                &state.workspace,
                state.font_scale,
                &state.theme.to_string(),
            );
        }
        Message::FontScaleDown => {
            state.font_scale =
                (state.font_scale - FONT_SCALE_STEP).clamp(FONT_SCALE_MIN, FONT_SCALE_MAX);
            mdi::save(
                &state.workspace,
                state.font_scale,
                &state.theme.to_string(),
            );
        }
        Message::RigReconnected { radio, result } => match result {
            Ok(cmd_tx) => {
                tracing::info!(radio, "rig reconnected");
                state.handles.rig_txs.insert(radio, cmd_tx);
                state.handles.rig_status.insert(radio, true);
                // Dismissing the stale "connection lost" banner is nice but
                // we leave it to the operator — they may want to see what
                // happened. It'll be replaced by any future error anyway.
            }
            Err(e) => {
                tracing::warn!(radio, err = %e, "rig reconnect failed; retrying");
                // Keep trying in the background. Same back-off delay.
                if let Some(cfg) = state.rig_configs.get(&radio).cloned() {
                    pending = schedule_rig_reconnect(radio, cfg, state.app_tx.clone());
                }
            }
        },
        Message::FontScaleReset => {
            state.font_scale = 1.0;
            mdi::save(
                &state.workspace,
                state.font_scale,
                &state.theme.to_string(),
            );
        }
    }

    pending
}

/// Build a `Task` that sleeps for `RIG_RECONNECT_DELAY` then attempts
/// `spawn_rig_adapter` with the saved config. Producer of
/// `Message::RigReconnected`, whose handler either stores the fresh
/// `RigCmd` sender or schedules another retry on failure.
fn schedule_rig_reconnect(
    radio: RadioId,
    cfg: RigConfig,
    app_tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            tokio::time::sleep(RIG_RECONNECT_DELAY).await;
            let result = logger_runtime::spawn_rig_adapter(&cfg, app_tx)
                .await
                .map_err(|e| format!("{e:#}"));
            (radio, result)
        },
        |(radio, result)| Message::RigReconnected { radio, result },
    )
}

fn view(state: &App) -> Element<'_, Message> {
    let banner = state
        .error_banner
        .as_ref()
        .map(|msg| error_banner_view(msg.clone()))
        .unwrap_or_else(|| iced::widget::Space::new().height(0).into());
    let main = state.workspace.view(
        |pane| body_for(state, pane),
        // Stack the banner on top of the status bar at the bottom so critical
        // alerts are always visible regardless of pane z-order.
        iced::widget::column![banner, status_bar(state)].into(),
        Message::Mdi,
    );

    if let Some(modal) = state.modal {
        use iced::widget::{container, mouse_area, stack, Space};
        use iced::{Background, Color, Length};
        let backdrop: Element<Message> = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| iced::widget::container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
                    ..iced::widget::container::Style::default()
                }),
        )
        .on_press(Message::CloseModal)
        .into();
        let content: Element<Message> = match modal {
            modals::Modal::Help => modals::help::view(Message::CloseModal),
            modals::Modal::ThemePicker => {
                modals::theme::view(&state.theme, Message::SetTheme, Message::CloseModal)
            }
        };
        stack![main, backdrop, content].into()
    } else {
        main
    }
}

fn error_banner_view(msg: String) -> Element<'static, Message> {
    use iced::widget::{button, container, row, text, Space};
    use iced::{Background, Border, Length};
    container(
        row![
            text("⚠").size(14.0),
            text(msg).size(12.0),
            Space::new().width(Length::Fill),
            button(text("✕").size(12.0))
                .padding([0, 8])
                .on_press(Message::DismissError)
                .style(|t: &Theme, _status| {
                    let pair = panes::style::error_banner_pair(t);
                    let mut overlay = pair.text;
                    overlay.a = 0.15;
                    iced::widget::button::Style {
                        background: Some(Background::Color(overlay)),
                        text_color: pair.text,
                        border: Border::default(),
                        ..iced::widget::button::Style::default()
                    }
                }),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([4, 12])
    .width(Length::Fill)
    .style(|t: &Theme| {
        let pair = panes::style::error_banner_pair(t);
        iced::widget::container::Style {
            background: Some(Background::Color(pair.color)),
            text_color: Some(pair.text),
            ..iced::widget::container::Style::default()
        }
    })
    .into()
}

fn body_for<'a>(state: &'a App, pane: &'a mdi::Pane) -> Element<'a, Message> {
    if pane.id == state.pane_entry {
        panes::entry::view(
            &state.session.state,
            state.show_r2(),
            &state.echo_per_radio,
            state.cw_transmitting.then_some(state.tx_radio),
        )
    } else if pane.id == state.pane_bandmap_r1 {
        let zoom = state.bandmap_zoom.get(&1).copied().unwrap_or(1.0);
        panes::bandmap::view(
            &state.session.state,
            &state.session.log_adapter,
            1,
            zoom,
            bandmap_r1_clicked,
            bandmap_r1_zoom,
        )
    } else if pane.id == state.pane_bandmap_r2 {
        let zoom = state.bandmap_zoom.get(&2).copied().unwrap_or(1.0);
        panes::bandmap::view(
            &state.session.state,
            &state.session.log_adapter,
            2,
            zoom,
            bandmap_r2_clicked,
            bandmap_r2_zoom,
        )
    } else if pane.id == state.pane_log {
        panes::log::view(&state.session.log_adapter)
    } else if pane.id == state.pane_score {
        panes::score::view(&state.session.log_adapter)
    } else if pane.id == state.pane_rate {
        panes::rate::view(&state.session.log_adapter)
    } else if pane.id == state.pane_avail {
        panes::available::view(&state.session.state, &state.session.log_adapter)
    } else if pane.id == state.pane_macros {
        panes::macros::view(&state.session.macros)
    } else if pane.id == state.pane_so2r {
        panes::so2r::view(
            state.session.state.focused_radio,
            state.tx_radio,
            &state.handles,
        )
    } else if pane.id == state.pane_condx {
        panes::condx::view(state.condx.as_ref())
    } else if pane.id == state.pane_scp {
        panes::scp::view(&state.session.state)
    } else {
        iced::widget::text(pane.body.clone()).into()
    }
}

fn bandmap_r1_clicked(call: Option<String>, freq: u64) -> Message {
    Message::BandmapClicked {
        radio: 1,
        call,
        freq_hz: freq,
    }
}

fn bandmap_r2_clicked(call: Option<String>, freq: u64) -> Message {
    Message::BandmapClicked {
        radio: 2,
        call,
        freq_hz: freq,
    }
}

fn bandmap_r1_zoom(zoom: f32) -> Message {
    Message::BandmapZoom { radio: 1, zoom }
}

fn bandmap_r2_zoom(zoom: f32) -> Message {
    Message::BandmapZoom { radio: 2, zoom }
}

/// Update in-memory radio state for clicks that hit no real rig adapter,
/// so the bandmap cursor moves visibly even without hardware. Mirrors
/// what a real `RigStatus` event from a rig task would do.
fn synthesize_rig_status(state: &mut App, radio: RadioId, freq_hz: u64) {
    let (mode, is_ptt, filter_width_hz) = state
        .session
        .state
        .radios
        .get(&radio)
        .map(|r| (r.mode.clone(), r.is_ptt, r.filter_width_hz))
        .unwrap_or_else(|| ("CW".to_string(), false, Some(500)));
    let ev = AppEvent::RigStatus {
        radio,
        freq_hz,
        mode,
        is_ptt,
        filter_width_hz,
    };
    let effects = bridge::step(&mut state.session, ev);
    state.observe_effects(&effects);
    bridge::dispatch(&mut state.session, effects, &state.handles);
}

fn status_bar(state: &App) -> Element<'_, Message> {
    use iced::widget::{container, row, text, Space};
    use iced::{Background, Border, Color, Length};

    let s = &state.session.state;
    let utc = chrono::Utc::now().format("%H:%M:%S UTC").to_string();
    let label = format!("{} • {}", s.my_call, state.session.contest.contest_name());

    let icon_btn_style = |t: &Theme, status: iced::widget::button::Status| {
        let pal = t.extended_palette().background.weak;
        let bg = match status {
            iced::widget::button::Status::Hovered => t.extended_palette().background.base.color,
            _ => pal.color,
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            text_color: pal.text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 2.0.into(),
            },
            ..iced::widget::button::Style::default()
        }
    };
    // Palette glyph — opens the theme picker modal. Single entry point
    // instead of the old binary dark/light toggle, since iced ships ~20
    // themes and we want them all reachable.
    let theme_btn = iced::widget::button(text("🎨").size(12.0))
        .padding([0, 6])
        .on_press(Message::OpenModal(modals::Modal::ThemePicker))
        .style(icon_btn_style);
    let help_btn = iced::widget::button(text("?").size(12.0))
        .padding([0, 6])
        .on_press(Message::OpenModal(modals::Modal::Help))
        .style(icon_btn_style);
    let font_down = iced::widget::button(text("A−").size(12.0))
        .padding([0, 6])
        .on_press(Message::FontScaleDown)
        .style(icon_btn_style);
    let font_up = iced::widget::button(text("A+").size(12.0))
        .padding([0, 6])
        .on_press(Message::FontScaleUp)
        .style(icon_btn_style);

    // RIG status: if any rig is configured, render one indicator per rig
    // (green=connected, red=configured-but-disconnected). Otherwise placeholder.
    let mut indicators: Vec<Element<Message>> = Vec::new();
    if state.handles.rig_status.is_empty() {
        indicators.push(indicator("RIG", IndicatorState::Idle));
    } else {
        for (radio, ok) in &state.handles.rig_status {
            indicators.push(indicator_owned(
                format!("R{radio}"),
                if *ok { IndicatorState::Ok } else { IndicatorState::Error },
            ));
        }
    }
    indicators.push(indicator(
        "KEY",
        if !state.handles.keyer_configured {
            IndicatorState::Idle
        } else if state.handles.keyer_connected {
            IndicatorState::Ok
        } else {
            IndicatorState::Error
        },
    ));
    indicators.push(indicator(
        "DXF",
        if !state.handles.dxfeed_configured {
            IndicatorState::Idle
        } else if state.handles.dxfeed_connected {
            IndicatorState::Ok
        } else {
            IndicatorState::Error
        },
    ));
    if state.handles.so2r_configured {
        indicators.push(indicator(
            "SO2R",
            if state.handles.so2r_connected {
                IndicatorState::Ok
            } else {
                IndicatorState::Error
            },
        ));
    }
    // Scoreboard upload status: gray Idle when not configured OR before
    // the first upload cycle fires; green on a successful post; red when
    // the adapter reports upload failures.
    indicators.push(indicator(
        "SCRBD",
        if !state.handles.scoreboard_configured {
            IndicatorState::Idle
        } else {
            match state.handles.scoreboard_status {
                ScoreboardStatus::Ok => IndicatorState::Ok,
                ScoreboardStatus::Failing => IndicatorState::Error,
                ScoreboardStatus::Idle => IndicatorState::Idle,
            }
        },
    ));

    // RTC upload status: same tri-state pattern as SCRBD. Hidden when
    // the adapter wasn't spawned (RTC disabled or contest has no
    // `rtc_id`) — there's no "disabled-but-relevant" state the way
    // scoreboard has, so we simply skip the badge.
    if state.handles.rtc_configured {
        use logger_runtime::rtc_adapter::RtcStatus;
        indicators.push(indicator(
            "RTC",
            match state.handles.rtc_status {
                RtcStatus::Ok => IndicatorState::Ok,
                RtcStatus::Failing | RtcStatus::Unsupported => IndicatorState::Error,
                RtcStatus::Idle | RtcStatus::NoChanges => IndicatorState::Idle,
            },
        ));
    }

    let mut bar = row![
        text(label).size(12.0),
        Space::new().width(Length::Fill),
        text(utc).size(12.0),
        Space::new().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::alignment::Vertical::Center);
    for ind in indicators {
        bar = bar.push(ind);
    }
    bar = bar
        .push(font_down)
        .push(font_up)
        .push(theme_btn)
        .push(help_btn);

    container(bar)
        .padding([4, 10])
        .width(Length::Fill)
        .height(Length::Fixed(24.0))
        .align_y(iced::alignment::Vertical::Center)
        .style(|t: &Theme| {
            let pal = t.extended_palette().background.strong;
            container::Style {
                background: Some(Background::Color(pal.color)),
                text_color: Some(pal.text),
                border: Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.4),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

#[derive(Clone, Copy)]
enum IndicatorState {
    Idle,
    Ok,
    Error,
}

fn indicator<'a>(label: &'static str, state: IndicatorState) -> Element<'a, Message> {
    indicator_owned(label.to_string(), state)
}

fn indicator_owned<'a>(label: String, state: IndicatorState) -> Element<'a, Message> {
    use iced::widget::{container, text};
    use iced::{Border, Color, Theme};
    container(text(label).size(10.0))
        .padding([1, 6])
        .style(move |t: &Theme| {
            let pair = match state {
                IndicatorState::Idle => t.extended_palette().background.weak,
                IndicatorState::Ok => t.extended_palette().success.base,
                IndicatorState::Error => t.extended_palette().danger.base,
            };
            container::Style {
                background: Some(pair.color.into()),
                text_color: Some(pair.text),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 2.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn subscription(state: &App) -> Subscription<Message> {
    let mut subs: Vec<Subscription<Message>> = vec![
        state.workspace.subscription().map(Message::Mdi),
        iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick),
        bridge::adapter_events().map(Message::Domain),
        keyboard_subscription(),
    ];
    // Subscriptions that depend on adapter receivers are registered only
    // after `spawn_adapters` completes and populates them. Registering
    // them earlier would make the subscription closure activate against
    // empty `OnceLock`s, then park forever in `pending` — which is why
    // the CondX pane was stuck at "no snapshot yet" with config loaded.
    if state.adapters_ready {
        subs.push(bridge::condx_events().map(Message::CondxSnapshot));
        subs.push(bridge::keyer_events().map(Message::KeyerEvent));
        subs.push(bridge::scoreboard_events().map(Message::ScoreboardStatus));
        subs.push(bridge::rtc_events().map(Message::RtcStatus));
    }
    Subscription::batch(subs)
}

fn keyboard_subscription() -> Subscription<Message> {
    event::listen_with(|ev, _status, _id| match ev {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) => {
            // Ctrl-modifier shortcuts that should never reach the reducer.
            // Match on the Key (which is layout-aware and stable across
            // shift state) rather than the produced text.
            if (modifiers.control() || modifiers.command())
                && let keyboard::Key::Character(s) = &key {
                    match s.as_str() {
                        "+" | "=" => return Some(Message::FontScaleUp),
                        "-" | "_" => return Some(Message::FontScaleDown),
                        "0" => return Some(Message::FontScaleReset),
                        _ => {}
                    }
                }
            // Backtick → SO2R RX-mode toggle. Intercept before the reducer
            // sees it so the entry box doesn't collect a stray `.
            if text.as_deref() == Some("`") {
                return Some(Message::ToggleSo2rRxMode);
            }
            keys::translate(&key, modifiers, text.as_deref()).map(Message::Domain)
        }
        _ => None,
    })
}

fn init() -> (App, iced::Task<Message>) {
    let maybe_cfg = PENDING_CONFIG
        .get()
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()));

    let app_tx = bridge::make_app_channel();

    let (session, adapter_bits, rig_configs) = match maybe_cfg {
        Some(cfg) => {
            let (session_bits, adapter_bits) = cfg.into_parts();
            tracing::info!(
                contest = %session_bits.contest,
                my_call = %session_bits.my_call,
                "loaded config"
            );
            let s = bridge::bootstrap_from_session(session_bits, app_tx.clone())
                .expect("bootstrap_from_session failed");
            // Snapshot the rig configs before spawn_adapters consumes
            // `adapter_bits`; we need them for reconnect.
            let rig_configs: std::collections::HashMap<RadioId, RigConfig> = adapter_bits
                .rigs
                .iter()
                .cloned()
                .map(|c| (c.radio_id, c))
                .collect();
            (s, Some(adapter_bits), rig_configs)
        }
        None => {
            tracing::info!("no --config given; running demo session (CWT, callsign TEST)");
            let mut s = bridge::bootstrap_demo(app_tx.clone()).expect("bootstrap_demo failed");
            // Seed a few static spots so the bandmap isn't empty for someone
            // first opening the GUI. No periodic generator — those used to
            // run, and they were misleading.
            bridge::inject_demo_state(&mut s);
            (s, None, std::collections::HashMap::new())
        }
    };

    let scp = std::sync::Arc::clone(&session.scp);
    let cty = session.cty.clone();
    let initial_focused = session.state.focused_radio;
    // Pull the scoreboard / RTC status receivers off the session so
    // the GUI can drive its status-bar indicator. Bootstrap spawned
    // the adapters already; we just consume their reported status.
    let mut session = session;
    let scoreboard_status_rx = session.scoreboard_status_rx.take();
    let rtc_status_rx = session.rtc_status_rx.take();
    // Stash the status receivers into their static slots so the
    // iced subscriptions can drain them. Mirrors where the old spawn
    // code used to populate them inside `spawn_adapters`.
    if let Some(rx) = scoreboard_status_rx {
        let _ = bridge::set_scoreboard_status_rx(rx);
    }
    if let Some(rx) = rtc_status_rx {
        let _ = bridge::set_rtc_status_rx(rx);
    }
    let mut app = App::new(session, app_tx.clone());
    app.rig_configs = rig_configs;
    // Note: `rtc_configured` lives on AdapterHandles and is populated
    // by `spawn_adapters` from `AdapterBits.rtc_configured`. Setting
    // it here would be overwritten when `AdaptersReady` installs the
    // spawned handles — don't.

    let task = if let Some(bits) = adapter_bits {
        iced::Task::perform(
            async move {
                match bridge::spawn_adapters(bits, app_tx, scp, cty, initial_focused).await {
                    Ok(h) => AdapterHandlesResult::Ok(std::sync::Arc::new(std::sync::Mutex::new(
                        Some(h),
                    ))),
                    Err(e) => AdapterHandlesResult::Err(format!("{e:#}")),
                }
            },
            Message::AdaptersReady,
        )
    } else {
        iced::Task::none()
    };

    (app, task)
}

fn theme_for(state: &App) -> Theme {
    state.theme.clone()
}

/// Strip `{NN}` WPM-change markers from macro-expanded CW text so the
/// inline echo line shows what the operator hears, not the keyer protocol.
/// Prosigns like `<AR>` use angle brackets and are preserved.
fn strip_speed_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_marker = false;
    for c in s.chars() {
        match c {
            '{' => in_marker = true,
            '}' if in_marker => in_marker = false,
            _ if !in_marker => out.push(c),
            _ => {}
        }
    }
    out
}

/// Look up the saved theme by name against `iced::Theme::ALL`. Returns
/// `None` on first run (no saved theme) OR if the saved name no longer
/// exists in this iced version (we gracefully fall back to Dark).
fn resolve_saved_theme() -> Option<Theme> {
    let name = mdi::load_theme_name()?;
    Theme::ALL.iter().find(|t| t.to_string() == name).cloned()
}

fn main() -> anyhow::Result<()> {
    let cli = config::Cli::parse();

    let default_level = if cli.debug { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!(
            "{default_level},\
             wgpu_hal=error,\
             wgpu_core=error,\
             naga=error,\
             iced_wgpu=warn,\
             iced_winit=warn,\
             cosmic_text=error"
        ))
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let maybe_cfg = config::load(&cli)?;
    PENDING_CONFIG.get_or_init(|| Mutex::new(None));
    *PENDING_CONFIG.get().unwrap().lock().unwrap() = maybe_cfg;

    // Restore saved OS-window geometry before iced brings up the window,
    // so the operator's preferred size and position are honored on first
    // paint (instead of flashing at a default size then jumping).
    let saved_window = mdi::load_window_state();
    match mdi::layout_path() {
        Some(p) => tracing::info!(path = %p.display(), "gui layout file"),
        None => tracing::warn!(
            "no platform config directory found; layout will not persist this session"
        ),
    }
    let window_settings = iced::window::Settings {
        size: saved_window
            .map(|w| iced::Size::new(w.w.max(400.0), w.h.max(300.0)))
            .unwrap_or_else(|| iced::Size::new(1280.0, 820.0)),
        position: saved_window
            .map(|w| iced::window::Position::Specific(iced::Point::new(w.x, w.y)))
            .unwrap_or(iced::window::Position::Default),
        ..Default::default()
    };

    iced::application(init, update, view)
        .title(|_state: &App| "Clogger".to_string())
        .subscription(subscription)
        .theme(theme_for)
        .scale_factor(scale_factor_for)
        .window(window_settings)
        .run()
        .map_err(|e| anyhow::anyhow!("iced exited: {e}"))
}

fn scale_factor_for(state: &App) -> f32 {
    state.font_scale
}
