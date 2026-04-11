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

/// Spawn a rig adapter task and return a handle to the rig.
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
            let mut builder = riglib::icom::IcomBuilder::new(model).serial_port(&config.port);
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
            let mut builder = riglib::yaesu::YaesuBuilder::new(model).serial_port(&config.port);
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
            let mut builder =
                riglib::elecraft::ElecraftBuilder::new(model).serial_port(&config.port);
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
            let mut builder = riglib::kenwood::KenwoodBuilder::new(model).serial_port(&config.port);
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

    // Initial poll
    let primary = rig.primary_receiver().await?;
    let freq_hz = rig.get_frequency(primary).await?;
    let mode = rig.get_mode(primary).await?;
    let is_ptt = rig.get_ptt().await.unwrap_or(false);
    let filter_width_hz = rig.get_passband(primary).await.ok().map(|pb| pb.hz());

    let mode_str = map_mode(&mode);
    // Each rig adapter is tied to a single radio_id (configured in TOML).
    // This allows two physical rigs (each reporting receiver index 0) to be
    // distinguished as Radio 1 and Radio 2.
    let radio_id = config.radio_id;

    let _ = tx
        .send(AppEvent::RigStatus {
            radio: radio_id,
            freq_hz,
            mode: mode_str.clone(),
            is_ptt,
            filter_width_hz,
        })
        .await;

    // The poll task updates filter_width_hz via a watch channel; the
    // subscription task reads it when forwarding RigStatus events.
    let (filter_tx, filter_rx) = watch::channel(filter_width_hz);

    // Subscribe and forward events
    let mut events = rig.subscribe()?;
    let mut last = LastRigState {
        freq_hz,
        mode: mode_str,
        is_ptt,
        filter_width_hz,
    };

    let event_tx = tx.clone();
    tokio::spawn(async move {
        let filter_rx = filter_rx;
        loop {
            match events.recv().await {
                Ok(RigEvent::FrequencyChanged { receiver: _, freq_hz }) => {
                    last.freq_hz = freq_hz;
                    last.filter_width_hz = *filter_rx.borrow();
                    let _ = event_tx
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
                    let _ = event_tx
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
                    let _ = event_tx
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
                    warn!("rig disconnected");
                    let _ = event_tx
                        .send(AppEvent::RigDisconnected { radio: radio_id })
                        .await;
                    break;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("rig event stream lagged, dropped {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    warn!("rig event stream closed");
                    let _ = event_tx
                        .send(AppEvent::RigDisconnected { radio: radio_id })
                        .await;
                    break;
                }
            }
        }
    });

    // Poll frequency, mode, and passband at 4 Hz — get_frequency()/get_mode()
    // emit events into the broadcast channel, which the subscription task
    // forwards.  get_passband() doesn't emit broadcast events, so we update
    // the watch channel and let the subscription task pick it up on the next
    // broadcast event.
    let poll_rig = Arc::clone(&rig);
    let poll_tx = tx.clone();
    tokio::spawn(async move {
        // DIAGNOSTIC: filter_tx moved in so the watch channel's sender half
        // stays alive for filter_rx readers. Its `.send()` is disabled
        // below as part of the CI-V bus contention test (see commented
        // get_passband block).
        let _filter_tx = filter_tx;
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        let mut consecutive_errors = 0u32;
        loop {
            interval.tick().await;
            let freq_ok = poll_rig.get_frequency(primary).await.is_ok();
            let mode_ok = poll_rig.get_mode(primary).await.is_ok();
            // DIAGNOSTIC: get_passband polling disabled to test whether
            // CI-V traffic for filter width reads is what's making the
            // IC-7610 ATT and FIL buttons feel unresponsive. Revert this
            // block (and the `_filter_tx` rebind above) to restore normal
            // filter-width tracking. The watch channel keeps whatever
            // value get_passband returned at startup (line 132), so the
            // reducer will still receive a reasonable filter_width_hz on
            // the first RigStatus after a freq/mode change.
            // let new_filter = poll_rig
            //     .get_passband(primary)
            //     .await
            //     .ok()
            //     .map(|pb| pb.hz());
            // let _ = _filter_tx.send(new_filter);
            if freq_ok && mode_ok {
                consecutive_errors = 0;
            } else {
                consecutive_errors += 1;
                warn!("rig poll failed ({consecutive_errors} consecutive)");
                if consecutive_errors >= 3 {
                    warn!("rig poll: too many consecutive errors, stopping");
                    let _ = poll_tx
                        .send(AppEvent::RigDisconnected { radio: radio_id })
                        .await;
                    break;
                }
            }
        }
    });

    // Control task: drains RigCmd from an mpsc channel and calls the
    // corresponding Rig methods. Shares `Arc<dyn Rig>` with the poll and
    // subscription tasks — the underlying riglib implementation serializes
    // commands internally via its own actor, so concurrent access is safe.
    // This is what makes hardware operations non-blocking from the event
    // loop's perspective: the event loop does `rig_tx.try_send(cmd)` and
    // returns in nanoseconds; the control task picks it up and awaits on
    // the CI-V roundtrip without blocking the UI.
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<RigCmd>(32);
    let control_rig = Arc::clone(&rig);
    let control_err_tx = tx;
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                RigCmd::SetFrequency { hz } => {
                    if let Err(e) = control_rig.set_frequency(primary, hz).await {
                        warn!("rig {} set_frequency failed: {e}", radio_id);
                        // Transient errors get surfaced to the UI as
                        // RigError-style events could be added here; for
                        // now, mirror the existing log-and-continue behavior.
                        let _ = control_err_tx
                            .send(AppEvent::RigDisconnected { radio: radio_id })
                            .await;
                    }
                }
            }
        }
    });

    Ok(cmd_tx)
}
