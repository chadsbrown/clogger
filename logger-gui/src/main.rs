mod analytics;
mod bridge;
mod config;
mod keys;
mod mdi;
mod modals;
mod panes;
mod perf;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use clap::Parser;
use iced::{event, keyboard, Element, Event, Point, Size, Subscription, Theme};
use logger_core::{AppEvent, Effect, RadioId};
use logger_runtime::{CondXSnapshot, KeyerEvent, ScoreboardStatus, Session};

use bridge::AdapterHandles;
use panes::dxfeed::{DxActivityItem, DxActivityKind};

const DX_ACTIVITY_CAPACITY: usize = 256;

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
    pane_dxfeed: u32,
    pane_perf: u32,
    dx_activity: VecDeque<DxActivityItem>,
    perf: perf::PerfSampler,
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
    /// Clone of the `AppEvent` sender that adapter tasks publish to.
    /// Kept so on-demand spawns (e.g. future ad-hoc adapter restarts)
    /// can hand a fresh sender without replumbing.
    #[allow(dead_code)]
    app_tx: tokio::sync::mpsc::Sender<AppEvent>,
    /// Cached bandmap/available/rate pane data — recomputed in
    /// `update()` when inputs change rather than every render.
    analytics: analytics::PaneAnalytics,
    /// Operator's contest category — needed at Cabrillo export time.
    /// `None` when running demo or when the user omitted `[category]`.
    session_category: Option<logger_runtime::CategoryConfig>,
    /// Cabrillo header metadata — operator-supplied identity fields.
    session_cabrillo: logger_runtime::CabrilloConfig,
}

const FONT_SCALE_MIN: f32 = 0.6;
const FONT_SCALE_MAX: f32 = 2.0;
const FONT_SCALE_STEP: f32 = 0.1;

impl App {
    fn new(
        session: Session,
        app_tx: tokio::sync::mpsc::Sender<AppEvent>,
        session_category: Option<logger_runtime::CategoryConfig>,
        session_cabrillo: logger_runtime::CabrilloConfig,
    ) -> Self {
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
            workspace.add("DX Feed", Point::new(1180.0, 400.0), Size::new(320.0, 260.0));
            workspace.add("Performance", Point::new(1180.0, 680.0), Size::new(320.0, 220.0));
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
        let pane_dxfeed = workspace.id_by_title("DX Feed").unwrap_or_else(|| {
            workspace.add("DX Feed", Point::new(1180.0, 400.0), Size::new(320.0, 260.0))
        });
        let pane_perf = workspace.id_by_title("Performance").unwrap_or_else(|| {
            workspace.add("Performance", Point::new(1180.0, 680.0), Size::new(320.0, 220.0))
        });
        let pane_scp = workspace.id_by_title("SCP").unwrap_or_else(|| {
            workspace.add("SCP", Point::new(40.0, 800.0), Size::new(440.0, 160.0))
        });
        let tx_radio = session.state.focused_radio;
        let bandmap_zoom = std::collections::HashMap::new();
        let mut app = Self {
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
            pane_dxfeed,
            pane_perf,
            dx_activity: VecDeque::with_capacity(DX_ACTIVITY_CAPACITY),
            perf: perf::PerfSampler::new(),
            theme: resolve_saved_theme().unwrap_or(Theme::Dark),
            modal: None,
            error_banner: None,
            adapters_ready: false,
            font_scale: mdi::load_font_scale().unwrap_or(1.0).clamp(FONT_SCALE_MIN, FONT_SCALE_MAX),
            app_tx,
            analytics: analytics::PaneAnalytics::new(),
            session_category,
            session_cabrillo,
        };
        // Warm the cache so the first paint shows real bandmap/available/
        // rate data instead of blank panes that fill in only after the
        // first message lands.
        app.refresh_analytics();
        app
    }

    /// Refresh the cached pane analytics from the current session state.
    /// Called at the end of `update()` so subsequent `view()` calls read
    /// prepared data. Cheap when none of the tracked inputs changed.
    fn refresh_analytics(&mut self) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let show_r2 = self.show_r2();
        self.analytics.refresh(
            &self.session.state,
            &self.session.log_adapter,
            show_r2,
            now_ms,
        );
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
    SetTheme(Theme),
    ToggleSo2rRxMode,
    OpenModal(modals::Modal),
    CloseModal,
    /// User clicked "ADIF" in the Export modal.
    ExportPickAdif,
    /// User clicked "Cabrillo" in the Export modal.
    ExportPickCabrillo,
    /// User edited the export destination path.
    ExportPathChanged(String),
    /// User confirmed the export destination path.
    ExportConfirm,
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

fn default_export_path(state: &App, format: modals::export::ExportFormat) -> String {
    let default_ext = match format {
        modals::export::ExportFormat::Adif => "adi",
        modals::export::ExportFormat::Cabrillo => "log",
    };
    let default_name = format!(
        "{}-{}.{}",
        state.session.state.my_call.to_lowercase(),
        state.session.contest.contest_id(),
        default_ext,
    );

    std::env::current_dir()
        .map(|dir| dir.join(&default_name))
        .unwrap_or_else(|_| PathBuf::from(default_name))
        .display()
        .to_string()
}

fn path_from_export_input(input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Export path cannot be empty".into());
    }

    if trimmed == "~" || trimmed.starts_with("~/") {
        let home = dirs::home_dir().ok_or_else(|| "Cannot resolve home directory".to_string())?;
        if trimmed == "~" {
            Ok(home)
        } else {
            Ok(home.join(&trimmed[2..]))
        }
    } else {
        Ok(PathBuf::from(trimmed))
    }
}

fn open_export_path_step(state: &mut App, format: modals::export::ExportFormat) {
    state.modal = Some(modals::Modal::Export(modals::export::ExportState {
        step: modals::export::ExportStep::ChoosePath {
            format,
            path: default_export_path(state, format),
        },
    }));
}

fn run_export_to_path(
    state: &mut App,
    format: modals::export::ExportFormat,
    path_input: String,
) {
    let path = match path_from_export_input(&path_input) {
        Ok(path) => path,
        Err(message) => {
            state.modal = Some(modals::Modal::Export(modals::export::ExportState {
                step: modals::export::ExportStep::Result {
                    message,
                    is_error: true,
                },
            }));
            return;
        },
    };

    let records = state.session.log_adapter.ordered_records();
    let my_call = state.session.state.my_call.clone();
    let result = match format {
        modals::export::ExportFormat::Adif => {
            let contest_id = state.session.contest.contest_id().to_string();
            logger_runtime::adif_export::export_adif(
                &records,
                &my_call,
                &contest_id,
                &path,
            )
        }
        modals::export::ExportFormat::Cabrillo => {
            let category = state
                .session_category
                .as_ref()
                .expect("category presence checked at Pick");
            let claimed_score = state.session.log_adapter.score_summary().claimed_score;
            logger_runtime::cabrillo_export::export_cabrillo(
                &records,
                state.session.contest.as_ref(),
                category,
                &state.session_cabrillo,
                &my_call,
                claimed_score,
                &path,
            )
        }
    };
    let step = match result {
        Ok(count) => modals::export::ExportStep::Result {
            message: format!("Exported {count} QSOs to {}", path.display()),
            is_error: false,
        },
        Err(e) => modals::export::ExportStep::Result {
            message: format!("Export failed: {e}"),
            is_error: true,
        },
    };
    state.modal = Some(modals::Modal::Export(modals::export::ExportState { step }));
}

fn update(state: &mut App, msg: Message) -> iced::Task<Message> {
    // Most arms don't need to return a Task; capture any async follow-up
    // in `pending` and return at the end.
    let pending: iced::Task<Message> = iced::Task::none();

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
            state.perf.sample();
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
            state.record_dx_activity(&ev);
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
                    // Adapter supervises its own reconnect with back-off;
                    // it'll emit fresh RigStatus events once it recovers.
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
        Message::ExportPickAdif => {
            open_export_path_step(state, modals::export::ExportFormat::Adif);
        }
        Message::ExportPickCabrillo => {
            // Cabrillo export needs a [category] section; without it we'd
            // emit an unsubmittable file. Surface that as a Result error
            // instead of asking for a destination.
            if state.session_category.is_none() {
                state.modal = Some(modals::Modal::Export(modals::export::ExportState {
                    step: modals::export::ExportStep::Result {
                        message: "Cabrillo export requires a [category] section in config.toml"
                            .into(),
                        is_error: true,
                    },
                }));
                return iced::Task::none();
            }
            open_export_path_step(state, modals::export::ExportFormat::Cabrillo);
        }
        Message::ExportPathChanged(next_path) => {
            if let Some(modals::Modal::Export(modals::export::ExportState {
                step: modals::export::ExportStep::ChoosePath { path, .. },
            })) = &mut state.modal
            {
                *path = next_path;
            }
        }
        Message::ExportConfirm => {
            let Some((format, path)) = state.modal.as_ref().and_then(|modal| match modal {
                modals::Modal::Export(modals::export::ExportState {
                    step: modals::export::ExportStep::ChoosePath { format, path },
                }) => Some((*format, path.clone())),
                _ => None,
            }) else {
                return iced::Task::none();
            };

            run_export_to_path(state, format, path);
        }
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
            if let Some(tx) = &state.handles.so2r_tx
                && let Err(e) = tx.try_send(logger_runtime::So2rCmd::SetRx {
                    radio: state.session.state.focused_radio,
                    mode: state.handles.so2r_default_rx_mode,
                })
            {
                tracing::warn!(
                    radio = state.session.state.focused_radio,
                    err = %e,
                    "ToggleSo2rRxMode dropped: so2r channel full or closed"
                );
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
        Message::FontScaleReset => {
            state.font_scale = 1.0;
            mdi::save(
                &state.workspace,
                state.font_scale,
                &state.theme.to_string(),
            );
        }
    }

    // Refresh cached pane analytics (bandmap worked/mults, available,
    // rate) once per message. No-op when none of the tracked inputs
    // changed; otherwise runs the work here rather than every render.
    state.refresh_analytics();

    pending
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

    if let Some(modal) = &state.modal {
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
            modals::Modal::Export(s) => modals::export::view(
                s,
                Message::ExportPickAdif,
                Message::ExportPickCabrillo,
                Message::ExportPathChanged,
                Message::ExportConfirm,
                Message::CloseModal,
            ),
        };
        stack![main, backdrop, content].into()
    } else {
        main
    }
}

/// Critical-alert banner above the status bar. It is the one place a
/// full danger fill is right: it must interrupt.
fn error_banner_view(msg: String) -> Element<'static, Message> {
    use iced::widget::{button, container, row, text, Space};
    use iced::{Background, Border, Length};
    use panes::style::{self, Icon};
    container(
        row![
            style::icon(Icon::Alert, style::TEXT_BODY),
            text(msg).size(style::TEXT_LABEL).font(style::FONT_UI_MEDIUM),
            Space::new().width(Length::Fill),
            button(style::icon(Icon::X, style::TEXT_LABEL))
                .padding(style::SP_2)
                .on_press(Message::DismissError)
                .style(|t: &Theme, status| {
                    let pair = style::error_banner_pair(t);
                    let overlay = style::with_alpha(
                        pair.text,
                        if matches!(status, iced::widget::button::Status::Hovered) {
                            0.25
                        } else {
                            0.12
                        },
                    );
                    iced::widget::button::Style {
                        background: Some(Background::Color(overlay)),
                        text_color: pair.text,
                        border: Border {
                            radius: style::RADIUS_SM.into(),
                            ..Border::default()
                        },
                        ..iced::widget::button::Style::default()
                    }
                }),
        ]
        .spacing(style::SP_3)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([style::SP_2, style::SP_4])
    .width(Length::Fill)
    .style(|t: &Theme| {
        let pair = style::error_banner_pair(t);
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
        let cursor_hz = state.session.state.radios.get(&1).map(|r| r.freq_hz).unwrap_or(0);
        panes::bandmap::view(
            &state.session.state,
            state.analytics.r1.as_ref(),
            cursor_hz,
            zoom,
            bandmap_r1_clicked,
            bandmap_r1_zoom,
        )
    } else if pane.id == state.pane_bandmap_r2 {
        let zoom = state.bandmap_zoom.get(&2).copied().unwrap_or(1.0);
        let cursor_hz = state.session.state.radios.get(&2).map(|r| r.freq_hz).unwrap_or(0);
        panes::bandmap::view(
            &state.session.state,
            state.analytics.r2.as_ref(),
            cursor_hz,
            zoom,
            bandmap_r2_clicked,
            bandmap_r2_zoom,
        )
    } else if pane.id == state.pane_log {
        panes::log::view(&state.session.log_adapter)
    } else if pane.id == state.pane_score {
        panes::score::view(&state.session.log_adapter)
    } else if pane.id == state.pane_rate {
        panes::rate::view(&state.analytics.rate)
    } else if pane.id == state.pane_avail {
        panes::available::view(&state.analytics.avail)
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
    } else if pane.id == state.pane_dxfeed {
        panes::dxfeed::view(&state.dx_activity)
    } else if pane.id == state.pane_perf {
        panes::perf::view(state.perf.snapshot())
    } else if pane.id == state.pane_scp {
        panes::scp::view(&state.session.state)
    } else {
        iced::widget::text(pane.body.clone()).into()
    }
}

impl App {
    fn record_dx_activity(&mut self, ev: &AppEvent) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let item = match ev {
            AppEvent::SpotReceived { spot } => Some(DxActivityItem {
                ts_ms: now_ms,
                kind: DxActivityKind::Spot,
                call: spot.call.clone(),
                freq_hz: Some(spot.freq_hz),
                mode: Some(spot.mode.clone()),
            }),
            AppEvent::SpotWithdrawn { call } => {
                let existing = self
                    .session
                    .state
                    .bandmap
                    .iter()
                    .find(|spot| spot.call.eq_ignore_ascii_case(call));
                Some(DxActivityItem {
                    ts_ms: now_ms,
                    kind: DxActivityKind::Withdraw,
                    call: call.clone(),
                    freq_hz: existing.map(|spot| spot.freq_hz),
                    mode: existing.map(|spot| spot.mode.clone()),
                })
            }
            _ => None,
        };

        if let Some(item) = item {
            if self.dx_activity.len() == DX_ACTIVITY_CAPACITY {
                self.dx_activity.pop_front();
            }
            self.dx_activity.push_back(item);
        }
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

/// Bottom status bar, laid out in three zones:
/// * left: station call (semibold) and contest name
/// * center: UTC clock in mono, which contesters check constantly
/// * right: connection dots, a divider, then icon buttons
///
/// Earlier versions used solid-fill indicator pills and mixed an emoji
/// (🎨) with Unicode glyphs for buttons, so rendering depended on the
/// system's emoji font. Every icon now comes from the bundled Lucide
/// subset.
fn status_bar(state: &App) -> Element<'_, Message> {
    use iced::widget::{column, container, row, text, Space};
    use iced::{Background, Length};
    use panes::style::{self, Icon};
    use panes::widgets::{self, ButtonVariant, DotState};

    let s = &state.session.state;
    let utc = chrono::Utc::now().format("%H:%M:%S").to_string();

    let icon_btn = |i: Icon, msg: Message| -> Element<'_, Message> {
        widgets::icon_button(i, style::TEXT_BODY + 1.0, ButtonVariant::Ghost, msg)
            .padding(3)
            .into()
    };

    // RIG status: one dot per configured rig (green connected, red
    // configured but disconnected), or a single idle placeholder.
    let mut dots: Vec<Element<Message>> = Vec::new();
    if state.handles.rig_status.is_empty() {
        dots.push(widgets::status_dot("RIG", DotState::Idle));
    } else {
        for (radio, ok) in &state.handles.rig_status {
            dots.push(widgets::status_dot(
                format!("R{radio}"),
                if *ok { DotState::Ok } else { DotState::Error },
            ));
        }
    }
    let tri = |configured: bool, connected: bool| {
        if !configured {
            DotState::Idle
        } else if connected {
            DotState::Ok
        } else {
            DotState::Error
        }
    };
    dots.push(widgets::status_dot(
        "KEY",
        tri(state.handles.keyer_configured, state.handles.keyer_connected),
    ));
    dots.push(widgets::status_dot(
        "DXF",
        tri(state.handles.dxfeed_configured, state.handles.dxfeed_connected),
    ));
    if state.handles.so2r_configured {
        dots.push(widgets::status_dot(
            "SO2R",
            tri(true, state.handles.so2r_connected),
        ));
    }
    // Scoreboard upload status: idle when not configured OR before the
    // first upload cycle fires; ok on a successful post; error when the
    // adapter reports upload failures.
    dots.push(widgets::status_dot(
        "SCRBD",
        if !state.handles.scoreboard_configured {
            DotState::Idle
        } else {
            match state.handles.scoreboard_status {
                ScoreboardStatus::Ok => DotState::Ok,
                ScoreboardStatus::Failing => DotState::Error,
                ScoreboardStatus::Idle => DotState::Idle,
            }
        },
    ));
    // RTC upload status: same tri-state pattern as SCRBD. Hidden when the
    // adapter wasn't spawned (RTC disabled or contest has no `rtc_id`),
    // since there's no "disabled-but-relevant" state the way scoreboard
    // has.
    if state.handles.rtc_configured {
        use logger_runtime::rtc_adapter::RtcStatus;
        dots.push(widgets::status_dot(
            "RTC",
            match state.handles.rtc_status {
                RtcStatus::Ok => DotState::Ok,
                RtcStatus::Failing | RtcStatus::Unsupported => DotState::Error,
                RtcStatus::Idle | RtcStatus::NoChanges => DotState::Idle,
            },
        ));
    }

    let left = row![
        text(s.my_call.clone())
            .size(style::TEXT_LABEL)
            .font(style::FONT_UI_SEMIBOLD)
            .style(style::body),
        text(state.session.contest.contest_name().to_string())
            .size(style::TEXT_LABEL)
            .style(style::muted),
    ]
    .spacing(style::SP_3)
    .align_y(iced::alignment::Vertical::Center);

    let center = row![
        text(utc)
            .size(style::TEXT_LABEL)
            .font(style::FONT_MONO)
            .style(style::body),
        text("UTC")
            .size(style::TEXT_TINY)
            .font(style::FONT_UI_SEMIBOLD)
            .style(style::very_muted),
    ]
    .spacing(style::SP_2)
    .align_y(iced::alignment::Vertical::Center);

    let divider = container(Space::new().width(1).height(14))
        .width(1)
        .height(14)
        .style(|t: &Theme| iced::widget::container::Style {
            background: Some(Background::Color(style::border_color(t))),
            ..iced::widget::container::Style::default()
        });

    let right = row![
        row(dots)
            .spacing(style::SP_4)
            .align_y(iced::alignment::Vertical::Center),
        divider,
        row![
            icon_btn(Icon::TextSmaller, Message::FontScaleDown),
            icon_btn(Icon::TextLarger, Message::FontScaleUp),
            icon_btn(
                Icon::Download,
                Message::OpenModal(modals::Modal::Export(
                    modals::export::ExportState::default(),
                )),
            ),
            icon_btn(Icon::Palette, Message::OpenModal(modals::Modal::ThemePicker)),
            icon_btn(Icon::Help, Message::OpenModal(modals::Modal::Help)),
        ]
        .spacing(style::SP_1)
        .align_y(iced::alignment::Vertical::Center),
    ]
    .spacing(style::SP_4)
    .align_y(iced::alignment::Vertical::Center);

    // Left and right zones get equal fill portions so the clock sits at
    // the true center of the window, whatever their content widths.
    let bar = row![
        container(left).width(Length::FillPortion(1)),
        center,
        container(right)
            .width(Length::FillPortion(1))
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .align_y(iced::alignment::Vertical::Center);

    column![
        widgets::hairline(),
        container(bar)
            .padding([0.0, style::SP_4])
            .width(Length::Fill)
            .height(Length::Fixed(STATUS_BAR_H))
            .align_y(iced::alignment::Vertical::Center)
            .style(|t: &Theme| iced::widget::container::Style {
                background: Some(Background::Color(style::surface_pane(t))),
                text_color: Some(style::text_color(t)),
                ..iced::widget::container::Style::default()
            }),
    ]
    .into()
}

/// Status-bar height. It was 24px, which forced 10px text and cramped
/// buttons. 30px fits 12px text and 15px icons comfortably.
const STATUS_BAR_H: f32 = 30.0;

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
    event::listen_with(|ev, status, _id| {
        if matches!(status, event::Status::Captured) {
            return None;
        }

        match ev {
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
                    && let keyboard::Key::Character(s) = &key
                {
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
        }
    })
}

fn init() -> (App, iced::Task<Message>) {
    let maybe_cfg = PENDING_CONFIG
        .get()
        .and_then(|m| m.lock().ok().and_then(|mut g| g.take()));

    let app_tx = bridge::make_app_channel();

    let (session, adapter_bits, session_category, session_cabrillo) = match maybe_cfg {
        Some(cfg) => {
            let (session_bits, adapter_bits) = cfg.into_parts();
            tracing::info!(
                contest = %session_bits.contest,
                my_call = %session_bits.my_call,
                "loaded config"
            );
            // Cloned out before bootstrap consumes session_bits — both
            // are kept on App for export-time access.
            let category = session_bits.category.clone();
            let cabrillo = session_bits.cabrillo.clone();
            let s = bridge::bootstrap_from_session(session_bits, app_tx.clone())
                .expect("bootstrap_from_session failed");
            (s, Some(adapter_bits), category, cabrillo)
        }
        None => {
            tracing::info!("no --config given; running demo session (CWT, callsign TEST)");
            let mut s = bridge::bootstrap_demo(app_tx.clone()).expect("bootstrap_demo failed");
            // Seed a few static spots so the bandmap isn't empty for someone
            // first opening the GUI. No periodic generator — those used to
            // run, and they were misleading.
            bridge::inject_demo_state(&mut s);
            (s, None, None, logger_runtime::CabrilloConfig::default())
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
    let app = App::new(session, app_tx.clone(), session_category, session_cabrillo);
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

    let mut app = iced::application(init, update, view)
        .title(|_state: &App| "Clogger".to_string());
    // Bundled fonts (Inter, JetBrains Mono, Lucide). See
    // panes/style.rs and docs/gui-visual-design.md for why they're
    // embedded rather than taken from the system.
    for bytes in panes::style::FONT_FILES {
        app = app.font(*bytes);
    }
    app.default_font(panes::style::FONT_UI)
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
