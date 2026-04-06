mod adapters;
mod config;
mod event_loop;
mod ui;

use std::collections::HashSet;
use std::fs::File;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use logger_core::AppEvent;
use logger_runtime::{AvailSummary, RateInfo, ScoreSummary};
use tokio::sync::mpsc;
use tracing::warn;
use logger_runtime::{Keyer, Rig};

use config::{Cli, load_config};
use ui::log_tail::LogRow;

#[derive(Default)]
pub struct TuiState {
    pub cw_history: Vec<String>,
    pub cw_echo: String,
    pub cw_echo_enabled: bool,
    pub cw_transmitting: bool,
    pub log_display: Vec<LogRow>,
    pub worked_calls: HashSet<String>,
    pub mult_calls: HashSet<String>,
    pub score: ScoreSummary,
    pub avail: AvailSummary,
    pub rate: RateInfo,
    pub error_message: Option<String>,
    pub rig_connected: bool,
    pub keyer_connected: bool,
    pub dxfeed_connected: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = File::create("clogger.log")?;
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    let config = load_config(&cli)?;

    // Bootstrap session (contest, state, log adapter, call history, SCP)
    let session = logger_runtime::bootstrap(logger_runtime::SessionConfig {
        contest_id: config.contest.clone(),
        my_call: config.my_call.clone(),
        my_zone: config.my_zone,
        rst_sent: config.rst_sent.clone(),
        my_name: config.my_name.clone(),
        my_xchg: config.my_xchg.clone(),
        macro_overrides: config.macros,
        default_cw_speed: config.rig.as_ref().and_then(|r| r.cw_speed)
            .or(config.keyer.as_ref().map(|k| k.speed_wpm))
            .unwrap_or(28),
        db_path: cli.db.as_ref().or(config.db_path.as_ref()).cloned(),
        call_history_path: cli.call_history.as_ref().or(config.call_history_file.as_ref()).cloned(),
        scp_path: cli.scp.as_ref().or(config.scp_file.as_ref()).cloned(),
    })?;

    // Two-channel bridge: hardware adapters send AppEvent, terminal sends TerminalEvent
    let (app_tx, mut app_rx) = mpsc::channel::<AppEvent>(256);
    let (tui_tx, tui_rx) = mpsc::channel::<adapters::terminal::TerminalEvent>(256);

    // Spawn terminal input reader
    adapters::terminal::spawn_terminal_reader(tui_tx.clone());

    // Optionally connect rig
    let mut rig_handle: Option<Arc<dyn Rig>> = None;
    let mut rig_connected = false;
    if let Some(rig_config) = &config.rig {
        match logger_runtime::spawn_rig_adapter(rig_config, app_tx.clone()).await {
            Ok(rig) => {
                rig_handle = Some(rig);
                rig_connected = true;
            }
            Err(e) => warn!("rig connection failed, continuing without: {e}"),
        }
    }

    // Optionally connect keyer (rig cw_speed overrides keyer speed_wpm)
    let mut keyer_connected = false;
    let rig_cw_speed = config.rig.as_ref().and_then(|r| r.cw_speed);
    let keyer: Option<Box<dyn Keyer>> = if let Some(keyer_config) = &config.keyer {
        match logger_runtime::connect_keyer(keyer_config, rig_cw_speed).await {
            Ok(k) => {
                keyer_connected = true;
                Some(k)
            }
            Err(e) => {
                warn!("keyer connection failed, continuing without: {e}");
                None
            }
        }
    } else {
        None
    };

    // Subscribe to keyer echo events if cw_echo is enabled
    let (keyer_rx, cw_echo_enabled) = match (&keyer, &config.keyer) {
        (Some(k), Some(kc)) if kc.cw_echo => (Some(k.subscribe()), true),
        _ => (None, false),
    };

    // Optionally connect dxfeed
    let mut dxfeed_connected = false;
    if let Some(dxfeed_config) = &config.dxfeed {
        match logger_runtime::spawn_dxfeed_adapter(dxfeed_config, app_tx.clone()).await {
            Ok(()) => { dxfeed_connected = true; }
            Err(e) => warn!("dxfeed connection failed, continuing without: {e}"),
        }
    }

    // Bridge: AppEvent → TerminalEvent::App
    let bridge_tx = tui_tx.clone();
    tokio::spawn(async move {
        while let Some(ev) = app_rx.recv().await {
            let _ = bridge_tx
                .send(adapters::terminal::TerminalEvent::App(ev))
                .await;
        }
    });

    // Rebuild log display from restored QSOs
    let initial_log_display = adapters::log::build_log_display(&session.log_adapter);

    // Run the event loop
    event_loop::run(
        session.state,
        session.contest,
        session.macros,
        session.log_adapter,
        rig_handle,
        keyer,
        keyer_rx,
        cw_echo_enabled,
        config.cursor_style,
        session.call_history,
        session.scp,
        tui_rx,
        initial_log_display,
        rig_connected,
        keyer_connected,
        dxfeed_connected,
    )
    .await
}
