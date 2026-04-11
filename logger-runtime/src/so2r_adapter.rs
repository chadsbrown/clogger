use otrsp::{OtrspBuilder, Radio, RxMode, So2rSwitch};
use tracing::{info, warn};

use crate::config::So2rConfig;
use logger_core::{RadioId, So2rRxMode};

pub async fn connect_so2r(config: &So2rConfig) -> anyhow::Result<Box<dyn So2rSwitch>> {
    info!("connecting to OTRSP device on {}", config.port);
    let device = OtrspBuilder::new(&config.port).build().await?;
    info!("connected: {}", device.info().name);
    Ok(Box::new(device))
}

pub fn parse_rx_mode(s: &str) -> So2rRxMode {
    match s.to_ascii_lowercase().as_str() {
        "stereo" => So2rRxMode::Stereo,
        "reverse_stereo" | "reverse-stereo" => So2rRxMode::ReverseStereo,
        _ => So2rRxMode::Mono,
    }
}

pub(crate) fn radio_id_to_otrsp(radio: RadioId) -> Radio {
    if radio == 1 { Radio::Radio1 } else { Radio::Radio2 }
}

pub(crate) fn rx_mode_to_otrsp(mode: So2rRxMode) -> RxMode {
    match mode {
        So2rRxMode::Mono => RxMode::Mono,
        So2rRxMode::Stereo => RxMode::Stereo,
        So2rRxMode::ReverseStereo => RxMode::ReverseStereo,
    }
}

/// Set the SO2R switch's TX routing for the given radio.
pub async fn set_tx(switch: Option<&dyn So2rSwitch>, radio: RadioId) {
    if let Some(s) = switch {
        if let Err(e) = s.set_tx(radio_id_to_otrsp(radio)).await {
            warn!("OTRSP set_tx failed: {e}");
        }
    }
}

/// Set the SO2R switch's RX audio routing.
pub async fn set_rx(switch: Option<&dyn So2rSwitch>, radio: RadioId, mode: So2rRxMode) {
    if let Some(s) = switch {
        if let Err(e) = s.set_rx(radio_id_to_otrsp(radio), rx_mode_to_otrsp(mode)).await {
            warn!("OTRSP set_rx failed: {e}");
        }
    }
}
