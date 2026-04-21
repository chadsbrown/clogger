//! Glue between iced's event loop and the clogger reducer + hardware
//! adapters.
//!
//! Channel ownership: `init` creates the `app_tx`/`app_rx` pair, hands `app_tx`
//! to bootstrap and to every adapter spawn, and stashes `app_rx` in
//! `APP_RX` so the iced Subscription closure (which can't easily capture
//! move-only state) can pull it out on first activation.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use iced::{futures::sink::SinkExt, Subscription};
use logger_core::{reduce, AppEvent, Effect, RadioId};
use logger_runtime::{
    bootstrap, CondXSnapshot, KeyerCmd, KeyerEvent, MacroOverrides, RigCmd, ScoreboardStatus,
    Session, SessionConfig, So2rCmd, So2rSwitch,
};
use tokio::sync::{broadcast, mpsc, watch};

use crate::config::{AdapterBits, SessionBits};

/// AppEvent receiver, drained by the `adapter_events` subscription.
static APP_RX: OnceLock<Mutex<Option<mpsc::Receiver<AppEvent>>>> = OnceLock::new();
/// CondX snapshot receiver, drained by `condx_events` subscription. Only
/// populated when the user's config enables CondX polling.
static CONDX_RX: OnceLock<Mutex<Option<mpsc::Receiver<CondXSnapshot>>>> = OnceLock::new();
/// Keyer broadcast receiver, drained by `keyer_events` subscription. Only
/// populated when a keyer is configured AND `cw_echo = true`.
static KEYER_RX: OnceLock<Mutex<Option<broadcast::Receiver<KeyerEvent>>>> = OnceLock::new();
/// Scoreboard-status watch receiver, drained by `scoreboard_events`. Only
/// populated when `[[scoreboard.endpoints]]` is configured.
static SCOREBOARD_STATUS_RX: OnceLock<Mutex<Option<watch::Receiver<ScoreboardStatus>>>> =
    OnceLock::new();

/// RTC-status watch receiver, drained by `rtc_events`. Only populated
/// when `[rtc]` is enabled and the current contest has an `rtc_id`.
static RTC_STATUS_RX: OnceLock<
    Mutex<Option<watch::Receiver<logger_runtime::rtc_adapter::RtcStatus>>>,
> = OnceLock::new();

/// Make a fresh `AppEvent` channel and stash the receiver in `APP_RX`.
/// Called once from `init`.
pub fn make_app_channel() -> mpsc::Sender<AppEvent> {
    let (tx, rx) = mpsc::channel(256);
    APP_RX.get_or_init(|| Mutex::new(Some(rx)));
    tx
}

/// Boot a CWT demo session. Used when no config file is supplied.
pub fn bootstrap_demo(app_tx: mpsc::Sender<AppEvent>) -> Result<Session> {
    let cfg = SessionConfig {
        contest_id: "cwt".to_string(),
        my_call: "TEST".to_string(),
        my_zone: 5,
        rst_sent: "599".to_string(),
        my_name: Some("Test".to_string()),
        my_xchg: Some("TX".to_string()),
        station_config: HashMap::new(),
        macro_overrides: None::<MacroOverrides>,
        default_cw_speed: 30,
        db_path: None,
        call_history_path: None,
        scp_path: None,
        cty_path: None,
        start_serial: None,
        show_passband_qrm: false,
        bandmap_skip_worked: false,
        bandmap_high_at_top: false,
        esm_enabled: true,
        block_dupes: false,
        app_tx,
        scoreboard: None,
        rtc: None,
    };
    bootstrap(cfg)
}

/// Boot a real session from a parsed config.
pub fn bootstrap_from_session(bits: SessionBits, app_tx: mpsc::Sender<AppEvent>) -> Result<Session> {
    let station_config = crate::config::station_to_config_values(&bits.station)?;
    // Bootstrap owns scoreboard / RTC adapter lifecycle now; the GUI
    // passes pre-composed bundles (or None) instead of spawning them
    // itself later.
    let scoreboard = bits.scoreboard.clone();
    let rtc = bits.rtc.clone();
    let cfg = SessionConfig {
        contest_id: bits.contest,
        my_call: bits.my_call,
        my_zone: bits.my_zone,
        rst_sent: bits.rst_sent,
        my_name: bits.my_name,
        my_xchg: bits.my_xchg,
        station_config,
        macro_overrides: bits.macros,
        default_cw_speed: bits.default_cw_speed,
        db_path: bits.db_path,
        call_history_path: bits.call_history_file,
        scp_path: bits.scp_file,
        cty_path: bits.cty_file,
        start_serial: None,
        show_passband_qrm: bits.show_passband_qrm,
        bandmap_skip_worked: bits.bandmap_skip_worked,
        bandmap_high_at_top: bits.bandmap_high_at_top,
        esm_enabled: bits.esm_enabled,
        block_dupes: bits.block_dupes,
        app_tx,
        scoreboard,
        rtc,
    };
    bootstrap(cfg)
}

/// Inject demo state so the bandmap/entry/log panes have something to render
/// before any real adapters exist. Only called in demo mode.
pub fn inject_demo_state(session: &mut Session) {
    use logger_core::state::Spot;
    let effects = step(
        session,
        AppEvent::RigStatus {
            radio: 1,
            freq_hz: 14_040_000,
            mode: "CW".to_string(),
            is_ptt: false,
            filter_width_hz: Some(500),
        },
    );
    dispatch_log_only(session, effects);

    for spot in [
        ("K1ABC", 14_025_000),
        ("W2DEF", 14_038_500),
        ("VE3GHI", 14_055_000),
        ("DL4JKL", 14_065_000),
        ("JA1MNO", 14_080_000),
    ] {
        let effects = step(
            session,
            AppEvent::SpotReceived {
                spot: Spot {
                    call: spot.0.to_string(),
                    freq_hz: spot.1,
                    mode: "CW".to_string(),
                },
            },
        );
        dispatch_log_only(session, effects);
    }
}

/// Run a single reducer pass.
pub fn step(session: &mut Session, event: AppEvent) -> Vec<Effect> {
    reduce(
        &mut session.state,
        session.contest.as_ref(),
        &session.macros,
        &session.log_adapter,
        &session.log_adapter,
        session.call_history.as_ref(),
        &session.log_adapter,
        session.scp.as_ref(),
        event,
    )
}

/// Senders + flags returned by `spawn_adapters`. Stored on `App` so
/// `dispatch_effects` can route effects to the right hardware task.
pub struct AdapterHandles {
    pub rig_txs: HashMap<RadioId, mpsc::Sender<RigCmd>>,
    pub rig_status: BTreeMap<RadioId, bool>,
    pub keyer_tx: Option<mpsc::Sender<KeyerCmd>>,
    pub keyer_configured: bool,
    pub keyer_connected: bool,
    pub cw_echo_enabled: bool,
    pub so2r_tx: Option<mpsc::Sender<So2rCmd>>,
    pub so2r_configured: bool,
    pub so2r_connected: bool,
    pub so2r_default_rx_mode: logger_core::So2rRxMode,
    pub dxfeed_configured: bool,
    pub dxfeed_connected: bool,
    pub condx_configured: bool,
    pub scoreboard_configured: bool,
    /// Last known scoreboard-adapter status. Updated by the scoreboard
    /// subscription; drives the SCRBD status-bar indicator.
    pub scoreboard_status: ScoreboardStatus,
    /// `true` when RTC is enabled AND the contest maps to an `rtc_id`
    /// (i.e. the adapter actually spawned). Drives the RTC badge.
    pub rtc_configured: bool,
    /// Last known RTC-adapter status. Updated by the RTC subscription.
    pub rtc_status: logger_runtime::rtc_adapter::RtcStatus,
}

impl std::fmt::Debug for AdapterHandles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdapterHandles")
            .field("rig_status", &self.rig_status)
            .field("keyer_connected", &self.keyer_connected)
            .field("so2r_connected", &self.so2r_connected)
            .field("dxfeed_connected", &self.dxfeed_connected)
            .field("condx_configured", &self.condx_configured)
            .field("scoreboard_configured", &self.scoreboard_configured)
            .field("scoreboard_status", &self.scoreboard_status)
            .finish()
    }
}

impl Default for AdapterHandles {
    fn default() -> Self {
        Self {
            rig_txs: HashMap::new(),
            rig_status: BTreeMap::new(),
            keyer_tx: None,
            keyer_configured: false,
            keyer_connected: false,
            cw_echo_enabled: false,
            so2r_tx: None,
            so2r_configured: false,
            so2r_connected: false,
            so2r_default_rx_mode: logger_core::So2rRxMode::Mono,
            dxfeed_configured: false,
            dxfeed_connected: false,
            condx_configured: false,
            scoreboard_configured: false,
            scoreboard_status: ScoreboardStatus::Idle,
            rtc_configured: false,
            rtc_status: logger_runtime::rtc_adapter::RtcStatus::Idle,
        }
    }
}

/// Stash the scoreboard-status watch receiver into the static slot
/// that `scoreboard_events` drains from. Called once at startup with
/// the receiver taken off `Session`. Returns `Err(())` if already
/// initialized — harmless in practice but kept as a signal so callers
/// notice duplicate init.
pub fn set_scoreboard_status_rx(rx: watch::Receiver<ScoreboardStatus>) -> std::result::Result<(), ()> {
    SCOREBOARD_STATUS_RX
        .set(Mutex::new(Some(rx)))
        .map_err(|_| ())
}

/// RTC analogue of `set_scoreboard_status_rx`.
pub fn set_rtc_status_rx(
    rx: watch::Receiver<logger_runtime::rtc_adapter::RtcStatus>,
) -> std::result::Result<(), ()> {
    RTC_STATUS_RX.set(Mutex::new(Some(rx))).map_err(|_| ())
}

/// Spawn every configured hardware adapter. Mirrors `logger-tui/src/main.rs`'s
/// adapter setup section. Receivers for keyer-echo and condx are stashed in
/// `KEYER_RX`/`CONDX_RX` for the dedicated subscriptions to consume. The
/// scoreboard and RTC uploaders no longer spawn here — they're owned by
/// `logger_runtime::bootstrap`. We just record that scoreboard was
/// configured for the footer badge.
pub async fn spawn_adapters(
    bits: AdapterBits,
    app_tx: mpsc::Sender<AppEvent>,
    scp: Arc<dyn logger_core::ScpLookup>,
    cty: Option<Arc<logger_runtime::CtyDb>>,
    initial_focused_radio: RadioId,
) -> Result<AdapterHandles> {
    let mut handles = AdapterHandles::default();

    // --- Rigs ---
    for rig_config in &bits.rigs {
        match logger_runtime::spawn_rig_adapter(rig_config, app_tx.clone()).await {
            Ok(rig_tx) => {
                handles.rig_txs.insert(rig_config.radio_id, rig_tx);
                handles.rig_status.insert(rig_config.radio_id, true);
            }
            Err(e) => {
                tracing::warn!(
                    radio = rig_config.radio_id,
                    err = %e,
                    "rig adapter failed to start"
                );
                handles.rig_status.insert(rig_config.radio_id, false);
            }
        }
    }

    // --- Keyer ---
    handles.keyer_configured = bits.keyer.is_some();
    let rig_cw_speed = bits.rigs.iter().find_map(|r| r.cw_speed);
    let keyer = if let Some(keyer_config) = &bits.keyer {
        match logger_runtime::connect_keyer(keyer_config, rig_cw_speed).await {
            Ok(k) => {
                handles.keyer_connected = true;
                if keyer_config.cw_echo {
                    handles.cw_echo_enabled = true;
                    KEYER_RX.get_or_init(|| Mutex::new(Some(k.subscribe())));
                }
                Some(k)
            }
            Err(e) => {
                tracing::warn!(err = %e, "keyer connection failed");
                None
            }
        }
    } else {
        None
    };

    // --- DX feed (cluster) ---
    handles.dxfeed_configured = bits.dxfeed.is_some();
    if let Some(dxfeed_config) = &bits.dxfeed {
        logger_runtime::spawn_dxfeed_adapter(
            dxfeed_config,
            app_tx.clone(),
            Some(scp),
            cty.clone(),
        )
        .await
        .context("dxfeed config invalid — refusing to start")?;
        handles.dxfeed_connected = true;
    }

    // --- SO2R (OTRSP) ---
    handles.so2r_configured = bits.so2r.is_some();
    let (so2r_switch, so2r_default_rx_mode): (Option<Arc<dyn So2rSwitch>>, _) = if let Some(
        so2r_config,
    ) = &bits.so2r
    {
        let mode = logger_runtime::so2r_adapter::parse_rx_mode(&so2r_config.default_rx_mode);
        match logger_runtime::so2r_adapter::connect_so2r(so2r_config).await {
            Ok(switch) => {
                handles.so2r_connected = true;
                (Some(Arc::from(switch)), mode)
            }
            Err(e) => {
                tracing::warn!(err = %e, "OTRSP connection failed");
                (None, mode)
            }
        }
    } else {
        (None, logger_core::So2rRxMode::Mono)
    };
    handles.so2r_default_rx_mode = so2r_default_rx_mode;
    handles.so2r_tx = so2r_switch
        .as_ref()
        .map(|s| logger_runtime::spawn_so2r_task(Arc::clone(s), app_tx.clone()));

    // Initial OTRSP routing (one-time at startup, before any keyer activity).
    if let Some(switch) = so2r_switch.as_deref() {
        logger_runtime::so2r_adapter::set_tx(Some(switch), initial_focused_radio).await;
        logger_runtime::so2r_adapter::set_rx(
            Some(switch),
            initial_focused_radio,
            so2r_default_rx_mode,
        )
        .await;
    }

    // --- Keyer task (after SO2R, since the task wants the so2r_switch Arc) ---
    handles.keyer_tx = keyer.map(|k| {
        logger_runtime::spawn_keyer_task(
            k,
            so2r_switch.as_ref().map(Arc::clone),
            initial_focused_radio,
            app_tx.clone(),
        )
    });

    // --- CondX (solar/propagation polling) ---
    handles.condx_configured = bits.condx.enabled;
    if let Some(rx) = logger_runtime::spawn_condx_adapter(bits.condx) {
        CONDX_RX.get_or_init(|| Mutex::new(Some(rx)));
    }

    // Scoreboard / RTC adapters were spawned inside `bootstrap` from
    // the pre-composed bundles on `SessionBits`. All we do here is
    // record that scoreboard was configured so the status footer
    // shows the SCRBD badge. The status receiver is taken from
    // `Session.scoreboard_status_rx` by `init()` and stashed in
    // `SCOREBOARD_STATUS_RX` there (same place this code used to
    // populate).
    handles.scoreboard_configured = bits.scoreboard_configured;

    Ok(handles)
}

/// Dispatch effects against whatever adapters are wired. Hardware-bound
/// effects (CwSend/CwAbort/RigSet/KeyerSetSpeed/So2rFocusChanged) `try_send`
/// into the matching adapter's command channel; LogInsert writes to the
/// log adapter; Beep is currently silent (audio deferred); UI-flavored
/// effects are handled by the reducer's state changes.
pub fn dispatch(session: &mut Session, effects: Vec<Effect>, handles: &AdapterHandles) {
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    for ef in effects {
        match ef {
            Effect::LogInsert { draft } => {
                let radio = session.state.focused_radio as u32;
                let operator = session.state.active_operator as u32;
                match session.log_adapter.insert(draft, now_ms, radio, operator) {
                    Ok(id) => {
                        session.state.last_logged = Some(id);
                        tracing::debug!(qso_id = ?id, "logged QSO");
                    }
                    Err(e) => tracing::warn!(?e, "log insert failed"),
                }
            }
            Effect::CwSend { radio, text } => {
                if let Some(tx) = &handles.keyer_tx {
                    let _ = tx.try_send(KeyerCmd::Send { radio, text });
                } else {
                    tracing::debug!(?radio, "CwSend with no keyer wired");
                }
            }
            Effect::CwAbort => {
                if let Some(tx) = &handles.keyer_tx {
                    let _ = tx.try_send(KeyerCmd::Abort);
                }
            }
            Effect::KeyerSetSpeed { wpm } => {
                if let Some(tx) = &handles.keyer_tx {
                    let _ = tx.try_send(KeyerCmd::SetSpeed { wpm });
                }
            }
            Effect::RigSet { radio, freq_hz } => {
                if let Some(tx) = handles.rig_txs.get(&radio) {
                    let _ = tx.try_send(RigCmd::SetFrequency { hz: freq_hz });
                } else {
                    tracing::debug!(?radio, "RigSet with no rig wired");
                }
            }
            Effect::So2rFocusChanged { radio } => {
                if let Some(tx) = &handles.so2r_tx {
                    let _ = tx.try_send(So2rCmd::SetRx {
                        radio,
                        mode: handles.so2r_default_rx_mode,
                    });
                }
            }
            Effect::Beep { kind: _ } => {
                // Audio deferred to post-MVP. For now: silent.
            }
            Effect::UiSetFocus { field_id } => {
                if let Some(idx) = session
                    .state
                    .focused_entry()
                    .fields
                    .iter()
                    .position(|f| f.field_id == field_id)
                {
                    session.state.focused_entry_mut().focus = idx;
                }
            }
            Effect::UiClearEntry => {
                // Reducer already clears entry state; no UI-side cleanup needed.
            }
        }
    }
}

/// Demo-mode dispatch: only `LogInsert` matters; everything else is silent.
fn dispatch_log_only(session: &mut Session, effects: Vec<Effect>) {
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    for ef in effects {
        if let Effect::LogInsert { draft } = ef {
            let _ = session.log_adapter.insert(
                draft,
                now_ms,
                session.state.focused_radio as u32,
                session.state.active_operator as u32,
            );
        }
    }
}

/// Adapter-events subscription. Drains `app_rx` — i.e. every event coming
/// from a hardware adapter (rig status, dxfeed spots, keyer/so2r errors,
/// persist errors). Same shape regardless of whether real adapters are
/// wired; the channel just stays empty when nothing is producing.
pub fn adapter_events() -> Subscription<AppEvent> {
    // iced 0.14's `Subscription::run` takes a `fn` pointer (not a closure)
    // as the stream builder, so each subscription needs a top-level fn
    // that produces its own stream. The fn pointer's identity serves as
    // the subscription id; iced re-uses the same subscription across
    // renders as long as we keep passing the same fn.
    Subscription::run(adapter_events_stream)
}

fn adapter_events_stream() -> impl iced::futures::Stream<Item = AppEvent> {
    iced::stream::channel(64, async |mut output| {
        let Some(rx_slot) = APP_RX.get() else {
            tracing::warn!("APP_RX not initialized");
            std::future::pending::<()>().await;
            return;
        };
        let Some(mut rx) = rx_slot.lock().ok().and_then(|mut g| g.take()) else {
            tracing::warn!("APP_RX already taken; subscription idle");
            std::future::pending::<()>().await;
            return;
        };
        while let Some(ev) = rx.recv().await {
            if output.send(ev).await.is_err() {
                break;
            }
        }
    })
}

/// CondX snapshot subscription. Only emits when the user's config enables
/// CondX polling AND `spawn_adapters` has stashed the receiver.
pub fn condx_events() -> Subscription<CondXSnapshot> {
    Subscription::run(condx_events_stream)
}

fn condx_events_stream() -> impl iced::futures::Stream<Item = CondXSnapshot> {
    iced::stream::channel(8, async |mut output| {
        let Some(rx_slot) = CONDX_RX.get() else {
            std::future::pending::<()>().await;
            return;
        };
        let Some(mut rx) = rx_slot.lock().ok().and_then(|mut g| g.take()) else {
            std::future::pending::<()>().await;
            return;
        };
        while let Some(snap) = rx.recv().await {
            if output.send(snap).await.is_err() {
                break;
            }
        }
    })
}

/// Scoreboard-status subscription. Emits on every status transition
/// (Idle → Ok / Failing). Only populated when the scoreboard adapter is
/// spawned.
pub fn scoreboard_events() -> Subscription<ScoreboardStatus> {
    Subscription::run(scoreboard_events_stream)
}

/// RTC-status subscription. Mirrors scoreboard_events. Emits on every
/// status change; pending forever when the RTC adapter wasn't spawned
/// (so the iced runtime doesn't poll anything).
pub fn rtc_events() -> Subscription<logger_runtime::rtc_adapter::RtcStatus> {
    Subscription::run(rtc_events_stream)
}

fn rtc_events_stream() -> impl iced::futures::Stream<Item = logger_runtime::rtc_adapter::RtcStatus>
{
    iced::stream::channel(8, async |mut output| {
        let Some(rx_slot) = RTC_STATUS_RX.get() else {
            std::future::pending::<()>().await;
            return;
        };
        let Some(mut rx) = rx_slot.lock().ok().and_then(|mut g| g.take()) else {
            std::future::pending::<()>().await;
            return;
        };
        let initial = *rx.borrow();
        if output.send(initial).await.is_err() {
            return;
        }
        while rx.changed().await.is_ok() {
            let v = *rx.borrow();
            if output.send(v).await.is_err() {
                break;
            }
        }
    })
}

fn scoreboard_events_stream() -> impl iced::futures::Stream<Item = ScoreboardStatus> {
    iced::stream::channel(8, async |mut output| {
        let Some(rx_slot) = SCOREBOARD_STATUS_RX.get() else {
            std::future::pending::<()>().await;
            return;
        };
        let Some(mut rx) = rx_slot.lock().ok().and_then(|mut g| g.take()) else {
            std::future::pending::<()>().await;
            return;
        };
        // Emit the current value on start, then on every change.
        let initial = *rx.borrow();
        if output.send(initial).await.is_err() {
            return;
        }
        while rx.changed().await.is_ok() {
            let v = *rx.borrow();
            if output.send(v).await.is_err() {
                break;
            }
        }
    })
}

/// Keyer-echo subscription. Only emits when a keyer is configured AND
/// `cw_echo = true` in the keyer config.
pub fn keyer_events() -> Subscription<KeyerEvent> {
    Subscription::run(keyer_events_stream)
}

fn keyer_events_stream() -> impl iced::futures::Stream<Item = KeyerEvent> {
    iced::stream::channel(64, async |mut output| {
        let Some(rx_slot) = KEYER_RX.get() else {
            std::future::pending::<()>().await;
            return;
        };
        let Some(mut rx) = rx_slot.lock().ok().and_then(|mut g| g.take()) else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if output.send(ev).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "keyer broadcast lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

