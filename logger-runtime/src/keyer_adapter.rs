use tracing::{info, warn};
use winkey::{Keyer, WinKeyerBuilder};

use crate::config::KeyerConfig;

pub async fn connect_keyer(config: &KeyerConfig) -> anyhow::Result<Box<dyn Keyer>> {
    info!("connecting to WinKeyer on {}", config.port);
    let keyer = WinKeyerBuilder::new(&config.port)
        .speed(config.speed_wpm)
        .contest_spacing(config.contest_spacing)
        .build()
        .await?;
    info!("connected: {}", keyer.info().name);
    Ok(Box::new(keyer))
}

pub async fn send_cw(keyer: Option<&dyn Keyer>, text: &str) {
    if let Some(k) = keyer {
        if let Err(e) = k.send_message(text).await {
            warn!("keyer send failed: {e}");
        }
    }
}
