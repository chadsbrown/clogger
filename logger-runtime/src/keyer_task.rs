//! Dedicated keyer task.
//!
//! Owns a `Box<dyn Keyer>` (the WinKey) plus an optional
//! `Arc<dyn So2rSwitch>` clone for cross-radio TX routing. Drains a bounded
//! mpsc channel of `KeyerCmd`s so the event loop never awaits on the
//! keyer's serial port. Processes commands strictly in order — `Send`
//! commands queue up behind each other, and the winkey crate's internal
//! `rt_tx` priority channel ensures `abort` still preempts a pending
//! `send_raw` within ~1 ms.
//!
//! ## Cross-radio CW atomicity
//!
//! The plan investigation found that `OtrspDevice::set_tx` returns after
//! the serial write completes (~5 ms) but the **physical relay takes
//! another 10–50 ms** to finish switching. If we split the keyer and
//! so2r into fully independent tasks with independent command channels,
//! a cross-radio CW-send sequence (`abort → set_tx → send`) has a race
//! window where the first dits can route through the previous radio.
//!
//! The fix is to keep the whole sequence inside a single task. This
//! keyer task holds its own `Arc<dyn So2rSwitch>` clone and executes:
//!
//! 1. `keyer.abort()` — stops any in-flight CW on the current radio
//! 2. `so2r.set_tx(new_radio)` — issues the OTRSP routing command
//! 3. `sleep(50 ms)` — waits for the relay to physically settle
//! 4. `keyer.send_raw(bytes)` — starts CW on the new radio
//!
//! The `so2r_task` module handles `set_rx` (focus-change RX routing)
//! separately. Both tasks share the same `Arc<dyn So2rSwitch>`; concurrent
//! access is safe because the `otrsp` crate serializes commands
//! internally via its own actor.

use std::sync::Arc;
use std::time::Duration;

use logger_core::{AppEvent, RadioId};
use otrsp::So2rSwitch;
use tokio::sync::mpsc;
use tracing::warn;
use winkey::{Keyer, message::build_contest_message};

use crate::so2r_adapter::radio_id_to_otrsp;

/// Delay after OTRSP `set_tx` before keying CW on the new radio. Covers
/// the physical relay propagation window on typical SO2R hardware
/// (YCCC SO2R+, microHAM, SO2RDuino). Empirically 10–50 ms; we use
/// 50 ms as a conservative margin.
const SO2R_TX_RELAY_SETTLE: Duration = Duration::from_millis(50);

/// Commands handled by the keyer task.
#[derive(Debug, Clone)]
pub enum KeyerCmd {
    /// Send a CW message, possibly after switching TX routing. The
    /// `radio` field is the target; if it differs from the keyer task's
    /// tracked `current_tx_radio`, the task performs the
    /// abort → set_tx → settle → send sequence.
    Send { radio: RadioId, text: String },
    /// Abort any in-flight CW transmission. Use the winkey crate's
    /// priority channel internally, so this preempts any `Send` currently
    /// running within ~1 ms.
    Abort,
}

/// Spawn the keyer task.
///
/// `initial_tx_radio` is the radio the keyer task assumes is already
/// routed for TX at startup — match what `main.rs` set via the initial
/// `so2r_adapter::set_tx` call so the first cross-radio send correctly
/// detects a mismatch and performs the switch.
pub fn spawn_keyer_task(
    keyer: Box<dyn Keyer>,
    so2r: Option<Arc<dyn So2rSwitch>>,
    initial_tx_radio: RadioId,
    err_tx: mpsc::Sender<AppEvent>,
) -> mpsc::Sender<KeyerCmd> {
    let (tx, rx) = mpsc::channel::<KeyerCmd>(32);
    tokio::spawn(keyer_task(keyer, so2r, initial_tx_radio, rx, err_tx));
    tx
}

async fn keyer_task(
    keyer: Box<dyn Keyer>,
    so2r: Option<Arc<dyn So2rSwitch>>,
    mut current_tx_radio: RadioId,
    mut rx: mpsc::Receiver<KeyerCmd>,
    err_tx: mpsc::Sender<AppEvent>,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            KeyerCmd::Send { radio, text } => {
                // Cross-radio switch: abort any in-flight CW, switch OTRSP
                // TX routing, wait for the relay to settle, then proceed.
                if radio != current_tx_radio {
                    if let Err(e) = keyer.abort().await {
                        warn!("keyer abort (pre-switch) failed: {e}");
                        let _ = err_tx
                            .send(AppEvent::KeyerError {
                                message: format!("{e}"),
                            })
                            .await;
                    }
                    if let Some(switch) = &so2r {
                        if let Err(e) = switch.set_tx(radio_id_to_otrsp(radio)).await {
                            warn!("OTRSP set_tx failed during cross-radio switch: {e}");
                            let _ = err_tx
                                .send(AppEvent::So2rError {
                                    message: format!("{e}"),
                                })
                                .await;
                        }
                        // Wait for the OTRSP relay to physically switch
                        // before keying CW on the new radio. See module
                        // doc comment for the rationale.
                        tokio::time::sleep(SO2R_TX_RELAY_SETTLE).await;
                    }
                    current_tx_radio = radio;
                }

                let bytes = build_contest_message(&text);
                if let Err(e) = keyer.send_raw(&bytes).await {
                    warn!("keyer send failed: {e}");
                    let _ = err_tx
                        .send(AppEvent::KeyerError {
                            message: format!("{e}"),
                        })
                        .await;
                }
            }
            KeyerCmd::Abort => {
                if let Err(e) = keyer.abort().await {
                    warn!("keyer abort failed: {e}");
                    let _ = err_tx
                        .send(AppEvent::KeyerError {
                            message: format!("{e}"),
                        })
                        .await;
                }
            }
        }
    }
    // Channel closed — event loop is shutting down.
}
