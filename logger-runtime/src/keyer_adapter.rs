use tracing::{info, warn};
use winkey::{Keyer, WinKeyerBuilder, message::build_contest_message};

use crate::config::KeyerConfig;

pub async fn connect_keyer(config: &KeyerConfig, speed_override: Option<u8>) -> anyhow::Result<Box<dyn Keyer>> {
    let speed = speed_override.unwrap_or(config.speed_wpm);
    info!(
        "connecting to WinKeyer on {} at {speed} WPM, sidetone={}",
        config.port, config.sidetone
    );
    let keyer = WinKeyerBuilder::new(&config.port)
        .speed(speed)
        .contest_spacing(config.contest_spacing)
        .sidetone_enabled(config.sidetone)
        .build()
        .await?;
    info!("connected: {}", keyer.info().name);
    Ok(Box::new(keyer))
}

pub async fn send_cw(keyer: Option<&dyn Keyer>, text: &str) {
    if let Some(k) = keyer {
        let bytes = build_contest_message(text);
        if let Err(e) = k.send_raw(&bytes).await {
            warn!("keyer send failed: {e}");
        }
    }
}

pub async fn abort_cw(keyer: Option<&dyn Keyer>) {
    if let Some(k) = keyer
        && let Err(e) = k.abort().await {
            warn!("keyer abort failed: {e}");
        }
}
