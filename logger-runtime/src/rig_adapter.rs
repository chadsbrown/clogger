use std::sync::Arc;
use std::time::Duration;

use logger_core::AppEvent;
use riglib::{Rig, RigEvent};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::config::RigConfig;

/// Commands handled by the per-rig control task. Extend here as new
/// rig-control effects are added to the reducer.
#[derive(Debug, Clone)]
pub enum RigCmd {
    /// Tune the primary receiver to the given frequency (Hz).
    SetFrequency { hz: u64 },
}

struct LastRigState {
    freq_hz: u64,
    mode: String,
    is_ptt: bool,
    filter_width_hz: Option<u32>,
}

/// Initial-poll PTT query is retried this many times with PTT_RETRY_DELAY
/// between attempts before giving up and defaulting to PTT-off. The
/// connection itself is still considered established — a radio that refuses
/// to answer `get_ptt` on boot but responds to freq/mode reads is common
/// enough that this shouldn't block startup.
const PTT_STARTUP_RETRIES: u32 = 3;
const PTT_RETRY_DELAY: Duration = Duration::from_millis(200);

/// Reconnect backoff: start at 1s, double on each failure, cap at 30s.
/// Applies both to initial-connect failures and to mid-session disconnects.
const RECONNECT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

fn map_mode(mode: &riglib::Mode) -> String {
    match mode {
        riglib::Mode::CW | riglib::Mode::CWR => "CW".to_string(),
        riglib::Mode::USB | riglib::Mode::LSB => "SSB".to_string(),
        riglib::Mode::DataUSB
        | riglib::Mode::DataLSB
        | riglib::Mode::RTTY
        | riglib::Mode::RTTYR => "DIGITAL".to_string(),
        _ => "OTHER".to_string(),
    }
}

fn normalize(name: &str) -> String {
    name.to_lowercase().replace('-', "")
}

/// Spawn a rig adapter task and return the command channel.
///
/// The adapter is self-supervising: on connection failure or mid-session
/// disconnect it emits `AppEvent::RigDisconnected`, waits with exponential
/// backoff (1s → 2s → 4s → ... capped at 30s), and retries indefinitely
/// until the returned `cmd_tx` is dropped (which signals shutdown).
///
/// Commands sent to `cmd_tx` while the rig is offline are buffered in the
/// channel and dispatched as soon as reconnection succeeds; if the channel
/// buffer fills during a long outage, sends will block or fail per standard
/// mpsc semantics.
///
/// Only the initial model lookup is synchronous — a bogus model name in
/// TOML fails fast with `Err`. Every other error path is handled via
/// supervised reconnect.
///
/// # Limitation: one receiver per adapter
///
/// Each adapter instance is bound to a single `radio_id` from `RigConfig`,
/// and assumes the rig has exactly one receiver of interest (the primary).
/// All events from the rig — `FrequencyChanged`, `ModeChanged`, `PttChanged`,
/// `Disconnected` — are tagged with that `radio_id` regardless of which
/// receiver index they came from.
///
/// This is correct for the supported SO2R model: two physically separate rigs,
/// each with its own CAT connection, each spawning its own adapter with
/// `radio_id = 1` and `radio_id = 2` respectively.
///
/// **It is NOT correct for a single rig with multiple independent receivers**
/// (e.g., K3 + KRX3, FlexRadio with multiple slices, IC-7610 with sub-RX) where
/// you would want each receiver mapped to a different `radio_id`. In that case,
/// events from the sub-receiver would be misattributed to the main receiver's
/// `radio_id`, and `LastRigState` would cross-contaminate freq/mode between
/// receivers. Multi-receiver-per-rig support is a future enhancement: it would
/// require per-receiver `LastRigState` tracking and a config mapping from
/// `(rig, receiver_index)` to `radio_id`.
pub async fn spawn_rig_adapter(
    config: &RigConfig,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<mpsc::Sender<RigCmd>> {
    // Validate the model name upfront so TOML typos fail immediately
    // rather than disappearing into an infinite reconnect loop.
    riglib::find_rig(&config.model)
        .ok_or_else(|| anyhow::anyhow!("unknown rig model: {}", config.model))?;

    let (cmd_tx, cmd_rx) = mpsc::channel::<RigCmd>(32);
    let config = config.clone();
    tokio::spawn(rig_supervisor(config, tx, cmd_rx));
    Ok(cmd_tx)
}

/// Outer supervisor: owns the command channel across reconnect cycles.
/// Each iteration tries to bring up a full session (connect + subscribe +
/// poll + command dispatch). On failure or disconnect it sleeps with
/// backoff and tries again. Shutdown is signaled by the command sender
/// dropping, which closes `cmd_rx` and causes `run_session` to return
/// `SessionResult::Shutdown`.
async fn rig_supervisor(
    config: RigConfig,
    tx: mpsc::Sender<AppEvent>,
    mut cmd_rx: mpsc::Receiver<RigCmd>,
) {
    let radio_id = config.radio_id;
    let mut backoff = RECONNECT_INITIAL_DELAY;
    let mut first = true;

    loop {
        if !first {
            info!(radio = radio_id, "rig reconnecting in {:?}", backoff);
            tokio::time::sleep(backoff).await;
        }
        first = false;

        match run_session(&config, &tx, &mut cmd_rx).await {
            SessionResult::Shutdown => {
                info!(radio = radio_id, "rig adapter shutting down");
                return;
            }
            SessionResult::ConnectFailed(e) => {
                warn!(radio = radio_id, err = %e, "rig connect failed");
                let _ = tx
                    .send(AppEvent::RigDisconnected { radio: radio_id })
                    .await;
                backoff = (backoff * 2).min(RECONNECT_MAX_DELAY);
            }
            SessionResult::Disconnected(reason) => {
                warn!(radio = radio_id, "rig disconnected: {reason}");
                let _ = tx
                    .send(AppEvent::RigDisconnected { radio: radio_id })
                    .await;
                // Fresh disconnect — reset backoff so a long-stable rig
                // that briefly drops doesn't wait the full 30s next time.
                backoff = RECONNECT_INITIAL_DELAY;
            }
        }
    }
}

enum SessionResult {
    /// Command channel closed — caller dropped the adapter handle.
    Shutdown,
    /// Could not build or initialize the rig connection.
    ConnectFailed(anyhow::Error),
    /// Connection was established but went away mid-session.
    Disconnected(String),
}

/// One connection-lifetime session: build the rig, do the initial poll,
/// then run the subscribe/poll/control tasks until one of them signals
/// that the connection is gone (or the command channel closes).
async fn run_session(
    config: &RigConfig,
    tx: &mpsc::Sender<AppEvent>,
    cmd_rx: &mut mpsc::Receiver<RigCmd>,
) -> SessionResult {
    let radio_id = config.radio_id;
    let rig = match build_rig(config).await {
        Ok(r) => r,
        Err(e) => return SessionResult::ConnectFailed(e),
    };

    let primary = match rig.primary_receiver().await {
        Ok(p) => p,
        Err(e) => return SessionResult::ConnectFailed(anyhow::anyhow!("primary_receiver: {e}")),
    };
    let freq_hz = match rig.get_frequency(primary).await {
        Ok(f) => f,
        Err(e) => return SessionResult::ConnectFailed(anyhow::anyhow!("get_frequency: {e}")),
    };
    let mode = match rig.get_mode(primary).await {
        Ok(m) => m,
        Err(e) => return SessionResult::ConnectFailed(anyhow::anyhow!("get_mode: {e}")),
    };
    // PTT is less load-bearing for initial setup — many rigs return an
    // error on get_ptt() during the first few hundred ms after USB enum.
    // Retry a handful of times, then give up and assume PTT-off. This
    // doesn't fail the session; worst case the first UI paint shows PTT-off
    // until a genuine PttChanged event arrives.
    let is_ptt = startup_query_ptt(rig.as_ref()).await;
    let filter_width_hz = rig.get_passband(primary).await.ok().map(|pb| pb.hz());

    let mode_str = map_mode(&mode);

    let _ = tx
        .send(AppEvent::RigStatus {
            radio: radio_id,
            freq_hz,
            mode: mode_str.clone(),
            is_ptt,
            filter_width_hz,
        })
        .await;

    let (filter_tx, filter_rx) = watch::channel(filter_width_hz);

    let events = match rig.subscribe() {
        Ok(e) => e,
        Err(e) => return SessionResult::ConnectFailed(anyhow::anyhow!("subscribe: {e}")),
    };
    let last = LastRigState {
        freq_hz,
        mode: mode_str,
        is_ptt,
        filter_width_hz,
    };

    // Each sub-task reports its own exit reason. The first to report wins
    // — we discard the rest when we drop the JoinHandles on return.
    let (report_tx, mut report_rx) = mpsc::channel::<String>(4);

    let sub_handle = tokio::spawn(subscription_task(
        events,
        tx.clone(),
        filter_rx.clone(),
        radio_id,
        last,
        report_tx.clone(),
    ));

    let poll_handle = if !config.transceive {
        Some(tokio::spawn(poll_task(
            Arc::clone(&rig),
            primary,
            tx.clone(),
            filter_tx,
            radio_id,
            report_tx.clone(),
        )))
    } else {
        // No polling — but keep filter_tx alive so the subscription task's
        // filter_rx doesn't see a closed channel. A lightweight detached
        // task holds it for the session's duration.
        tokio::spawn(async move {
            let _hold = filter_tx;
            std::future::pending::<()>().await;
        });
        None
    };

    // Control task runs inline in this function: we select on cmd_rx and
    // the sub-task report channel. This way if the command channel closes
    // (shutdown) we can return Shutdown directly, without needing the
    // sub-tasks to cooperate.
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(RigCmd::SetFrequency { hz }) => {
                        if let Err(e) = rig.set_frequency(primary, hz).await {
                            warn!(radio = radio_id, "set_frequency failed: {e}");
                            // A command error is usually a disconnect in
                            // disguise. Let the sub-tasks confirm via their
                            // normal disconnect paths; we just log here.
                        }
                    }
                    None => {
                        // Shutdown: caller dropped cmd_tx.
                        sub_handle.abort();
                        if let Some(h) = poll_handle {
                            h.abort();
                        }
                        return SessionResult::Shutdown;
                    }
                }
            }
            reason = report_rx.recv() => {
                let reason = reason.unwrap_or_else(|| "sub-tasks exited".to_string());
                sub_handle.abort();
                if let Some(h) = poll_handle {
                    h.abort();
                }
                return SessionResult::Disconnected(reason);
            }
        }
    }
}

async fn build_rig(config: &RigConfig) -> anyhow::Result<Arc<dyn Rig>> {
    let rig_def = riglib::find_rig(&config.model)
        .ok_or_else(|| anyhow::anyhow!("unknown rig model: {}", config.model))?;
    info!("connecting to {} on {}", rig_def.model_name, config.port);
    let needle = normalize(&config.model);

    let rig: Arc<dyn Rig> = match rig_def.manufacturer {
        riglib::Manufacturer::Icom => {
            let model = riglib::icom::models::all_icom_models()
                .into_iter()
                .find(|m| normalize(m.name) == needle)
                .ok_or_else(|| anyhow::anyhow!("icom model not found: {}", config.model))?;
            let mut builder = riglib::icom::IcomBuilder::new(model)
                .serial_port(&config.port)
                .ai(config.transceive);
            if let Some(baud) = config.baud_rate {
                builder = builder.baud_rate(baud);
            }
            Arc::new(builder.build().await?) as Arc<dyn Rig>
        }
        riglib::Manufacturer::Yaesu => {
            let model = riglib::yaesu::models::all_yaesu_models()
                .into_iter()
                .find(|m| normalize(m.name) == needle)
                .ok_or_else(|| anyhow::anyhow!("yaesu model not found: {}", config.model))?;
            let mut builder = riglib::yaesu::YaesuBuilder::new(model)
                .serial_port(&config.port)
                .ai(config.transceive);
            if let Some(baud) = config.baud_rate {
                builder = builder.baud_rate(baud);
            }
            Arc::new(builder.build().await?) as Arc<dyn Rig>
        }
        riglib::Manufacturer::Elecraft => {
            let model = riglib::elecraft::models::all_elecraft_models()
                .into_iter()
                .find(|m| normalize(m.name) == needle)
                .ok_or_else(|| anyhow::anyhow!("elecraft model not found: {}", config.model))?;
            let mut builder = riglib::elecraft::ElecraftBuilder::new(model)
                .serial_port(&config.port)
                .ai(config.transceive);
            if let Some(baud) = config.baud_rate {
                builder = builder.baud_rate(baud);
            }
            Arc::new(builder.build().await?) as Arc<dyn Rig>
        }
        riglib::Manufacturer::Kenwood => {
            let model = riglib::kenwood::models::all_kenwood_models()
                .into_iter()
                .find(|m| normalize(m.name) == needle)
                .ok_or_else(|| anyhow::anyhow!("kenwood model not found: {}", config.model))?;
            let mut builder = riglib::kenwood::KenwoodBuilder::new(model)
                .serial_port(&config.port)
                .ai(config.transceive);
            if let Some(baud) = config.baud_rate {
                builder = builder.baud_rate(baud);
            }
            Arc::new(builder.build().await?) as Arc<dyn Rig>
        }
        riglib::Manufacturer::FlexRadio => {
            let builder = riglib::flex::FlexRadioBuilder::new().host(&config.port);
            Arc::new(builder.build().await?) as Arc<dyn Rig>
        }
    };
    Ok(rig)
}

/// Retry the initial PTT query a handful of times before giving up.
/// Returns `false` if every attempt errors; this lets a slow-to-boot radio
/// come up cleanly without blocking the adapter startup.
async fn startup_query_ptt(rig: &dyn Rig) -> bool {
    for attempt in 0..PTT_STARTUP_RETRIES {
        match rig.get_ptt().await {
            Ok(ptt) => return ptt,
            Err(e) => {
                if attempt + 1 < PTT_STARTUP_RETRIES {
                    warn!(
                        attempt = attempt + 1,
                        "get_ptt failed on startup: {e}; retrying"
                    );
                    tokio::time::sleep(PTT_RETRY_DELAY).await;
                } else {
                    warn!("get_ptt failed {PTT_STARTUP_RETRIES} times on startup; assuming off");
                }
            }
        }
    }
    false
}

async fn subscription_task(
    mut events: tokio::sync::broadcast::Receiver<RigEvent>,
    tx: mpsc::Sender<AppEvent>,
    filter_rx: watch::Receiver<Option<u32>>,
    radio_id: logger_core::RadioId,
    mut last: LastRigState,
    report_tx: mpsc::Sender<String>,
) {
    loop {
        match events.recv().await {
            Ok(RigEvent::FrequencyChanged { receiver: _, freq_hz }) => {
                last.freq_hz = freq_hz;
                last.filter_width_hz = *filter_rx.borrow();
                let _ = tx
                    .send(AppEvent::RigStatus {
                        radio: radio_id,
                        freq_hz: last.freq_hz,
                        mode: last.mode.clone(),
                        is_ptt: last.is_ptt,
                        filter_width_hz: last.filter_width_hz,
                    })
                    .await;
            }
            Ok(RigEvent::ModeChanged { receiver: _, mode }) => {
                last.mode = map_mode(&mode);
                last.filter_width_hz = *filter_rx.borrow();
                let _ = tx
                    .send(AppEvent::RigStatus {
                        radio: radio_id,
                        freq_hz: last.freq_hz,
                        mode: last.mode.clone(),
                        is_ptt: last.is_ptt,
                        filter_width_hz: last.filter_width_hz,
                    })
                    .await;
            }
            Ok(RigEvent::PttChanged { on }) => {
                last.is_ptt = on;
                last.filter_width_hz = *filter_rx.borrow();
                let _ = tx
                    .send(AppEvent::RigStatus {
                        radio: radio_id,
                        freq_hz: last.freq_hz,
                        mode: last.mode.clone(),
                        is_ptt: last.is_ptt,
                        filter_width_hz: last.filter_width_hz,
                    })
                    .await;
            }
            Ok(RigEvent::Disconnected) => {
                let _ = report_tx.send("subscription: Disconnected event".into()).await;
                return;
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!("rig event stream lagged, dropped {n} events");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                let _ = report_tx.send("subscription: stream closed".into()).await;
                return;
            }
        }
    }
}

/// Poll frequency, mode, and passband at 4 Hz (only when transceive is off).
/// On three consecutive failures, reports disconnect to the supervisor and
/// exits.
async fn poll_task(
    rig: Arc<dyn Rig>,
    primary: riglib::ReceiverId,
    _tx: mpsc::Sender<AppEvent>,
    filter_tx: watch::Sender<Option<u32>>,
    radio_id: logger_core::RadioId,
    report_tx: mpsc::Sender<String>,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    let mut consecutive_errors = 0u32;
    loop {
        interval.tick().await;
        let freq_ok = rig.get_frequency(primary).await.is_ok();
        let mode_ok = rig.get_mode(primary).await.is_ok();
        let new_filter = rig.get_passband(primary).await.ok().map(|pb| pb.hz());
        let _ = filter_tx.send(new_filter);
        if freq_ok && mode_ok {
            consecutive_errors = 0;
        } else {
            consecutive_errors += 1;
            warn!(radio = radio_id, "rig poll failed ({consecutive_errors} consecutive)");
            if consecutive_errors >= 3 {
                let _ = report_tx.send("poll: 3 consecutive errors".into()).await;
                return;
            }
        }
    }
}
