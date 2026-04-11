//! Dedicated SO2R OTRSP task.
//!
//! Owns a shared `Arc<dyn So2rSwitch>` and drains a bounded mpsc channel of
//! RX-routing commands. The event loop sends commands via `try_send` without
//! awaiting, so OTRSP serial latency never blocks UI responsiveness.
//!
//! **Scope note:** this task only handles `set_rx` (RX audio routing on focus
//! change / mono↔stereo toggle). `set_tx` is owned by the keyer task because
//! it's part of the atomic cross-radio CW-send sequence
//! (abort → set_tx → settle → send), and splitting the steps across two
//! tasks would create a race with OTRSP's 10–50 ms relay-settle delay.
//! The keyer task holds its own `Arc<dyn So2rSwitch>` clone for that
//! purpose; concurrent access with this task is safe because the `otrsp`
//! crate serializes commands internally via its own actor.

use std::sync::Arc;

use logger_core::{AppEvent, RadioId, So2rRxMode};
use otrsp::So2rSwitch;
use tokio::sync::mpsc;
use tracing::warn;

use crate::so2r_adapter::{radio_id_to_otrsp, rx_mode_to_otrsp};

/// Commands handled by the SO2R task.
#[derive(Debug, Clone)]
pub enum So2rCmd {
    /// Route RX audio to the given radio with the given mono/stereo mode.
    SetRx { radio: RadioId, mode: So2rRxMode },
}

/// Spawn the SO2R task.
///
/// Returns the sender the event loop uses to enqueue commands. The task
/// runs until its command channel closes (i.e., the sender is dropped),
/// at which point it exits. Dropping the task does not reset OTRSP state
/// on the device — the operator's last routing setting is preserved.
pub fn spawn_so2r_task(
    switch: Arc<dyn So2rSwitch>,
    err_tx: mpsc::Sender<AppEvent>,
) -> mpsc::Sender<So2rCmd> {
    let (tx, rx) = mpsc::channel::<So2rCmd>(32);
    tokio::spawn(so2r_task(switch, rx, err_tx));
    tx
}

async fn so2r_task(
    switch: Arc<dyn So2rSwitch>,
    mut rx: mpsc::Receiver<So2rCmd>,
    err_tx: mpsc::Sender<AppEvent>,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            So2rCmd::SetRx { radio, mode } => {
                if let Err(e) = switch
                    .set_rx(radio_id_to_otrsp(radio), rx_mode_to_otrsp(mode))
                    .await
                {
                    warn!("OTRSP set_rx failed: {e}");
                    let _ = err_tx
                        .send(AppEvent::So2rError {
                            message: format!("{e}"),
                        })
                        .await;
                }
            }
        }
    }
}
