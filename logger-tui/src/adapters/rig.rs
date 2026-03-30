use logger_core::AppEvent;
use riglib::{Rig, RigEvent};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::adapters::terminal::TerminalEvent;
use crate::config::RigConfig;

struct LastRigState {
    freq_hz: u64,
    mode: String,
    is_ptt: bool,
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

pub async fn spawn_rig_adapter(
    config: &RigConfig,
    tx: mpsc::Sender<TerminalEvent>,
) -> anyhow::Result<Box<dyn Rig>> {
    let rig_def = riglib::find_rig(&config.model)
        .ok_or_else(|| anyhow::anyhow!("unknown rig model: {}", config.model))?;

    info!("connecting to {} on {}", rig_def.model_name, config.port);

    let needle = normalize(&config.model);

    let rig: Box<dyn Rig> = match rig_def.manufacturer {
        riglib::Manufacturer::Icom => {
            let model = riglib::icom::models::all_icom_models()
                .into_iter()
                .find(|m| normalize(m.name) == needle)
                .ok_or_else(|| anyhow::anyhow!("icom model not found: {}", config.model))?;
            let mut builder = riglib::icom::IcomBuilder::new(model).serial_port(&config.port);
            if let Some(baud) = config.baud_rate {
                builder = builder.baud_rate(baud);
            }
            Box::new(builder.build().await?)
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
            Box::new(builder.build().await?)
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
            Box::new(builder.build().await?)
        }
        riglib::Manufacturer::Kenwood => {
            let model = riglib::kenwood::models::all_kenwood_models()
                .into_iter()
                .find(|m| normalize(m.name) == needle)
                .ok_or_else(|| anyhow::anyhow!("kenwood model not found: {}", config.model))?;
            let mut builder =
                riglib::kenwood::KenwoodBuilder::new(model).serial_port(&config.port);
            if let Some(baud) = config.baud_rate {
                builder = builder.baud_rate(baud);
            }
            Box::new(builder.build().await?)
        }
        riglib::Manufacturer::FlexRadio => {
            let builder = riglib::flex::FlexRadioBuilder::new().host(&config.port);
            Box::new(builder.build().await?)
        }
    };

    // Initial poll
    let primary = rig.primary_receiver().await?;
    let freq_hz = rig.get_frequency(primary).await?;
    let mode = rig.get_mode(primary).await?;
    let is_ptt = rig.get_ptt().await.unwrap_or(false);

    let mode_str = map_mode(&mode);
    let radio_id = primary.index() + 1;

    let _ = tx
        .send(TerminalEvent::App(AppEvent::RigStatus {
            radio: radio_id,
            freq_hz,
            mode: mode_str.clone(),
            is_ptt,
        }))
        .await;

    // Subscribe and forward events
    let mut events = rig.subscribe()?;
    let mut last = LastRigState {
        freq_hz,
        mode: mode_str,
        is_ptt,
    };

    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(RigEvent::FrequencyChanged { receiver, freq_hz }) => {
                    last.freq_hz = freq_hz;
                    let radio = receiver.index() + 1;
                    let _ = tx
                        .send(TerminalEvent::App(AppEvent::RigStatus {
                            radio,
                            freq_hz: last.freq_hz,
                            mode: last.mode.clone(),
                            is_ptt: last.is_ptt,
                        }))
                        .await;
                }
                Ok(RigEvent::ModeChanged { receiver, mode }) => {
                    last.mode = map_mode(&mode);
                    let radio = receiver.index() + 1;
                    let _ = tx
                        .send(TerminalEvent::App(AppEvent::RigStatus {
                            radio,
                            freq_hz: last.freq_hz,
                            mode: last.mode.clone(),
                            is_ptt: last.is_ptt,
                        }))
                        .await;
                }
                Ok(RigEvent::PttChanged { on }) => {
                    last.is_ptt = on;
                    let _ = tx
                        .send(TerminalEvent::App(AppEvent::RigStatus {
                            radio: 1,
                            freq_hz: last.freq_hz,
                            mode: last.mode.clone(),
                            is_ptt: last.is_ptt,
                        }))
                        .await;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("rig event stream lagged, dropped {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    warn!("rig event stream closed");
                    break;
                }
            }
        }
    });

    Ok(rig)
}
