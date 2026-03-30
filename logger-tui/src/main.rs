mod adapters;
mod config;
mod event_loop;
mod ui;

use std::collections::HashMap;

use anyhow::Result;
use clap::Parser;
use logger_core::{AppEvent, AppState, CallHistoryLookup, EntryState, EsmPolicy, NoCallHistory, contest_from_id};
use tokio::sync::mpsc;
use tracing::warn;
use logger_runtime::Keyer;

use config::{Cli, load_config};
use ui::log_tail::LogRow;

#[derive(Default)]
pub struct TuiState {
    pub cw_history: Vec<String>,
    pub log_display: Vec<LogRow>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();
    let config = load_config(&cli)?;

    // Build contest + macros
    let contest = contest_from_id(&config.contest)
        .ok_or_else(|| anyhow::anyhow!("unknown contest: {}", config.contest))?;
    let macros = contest.default_macros();

    // Build initial state
    let state = AppState {
        now_ms: chrono::Utc::now().timestamp_millis(),
        focused_radio: 1,
        active_operator: 1,
        radios: HashMap::new(),
        entry: EntryState::from_spec(&contest.form_spec()),
        bandmap: Vec::new(),
        last_logged: None,
        my_call: config.my_call.clone(),
        my_zone: config.my_zone,
        rst_sent: config.rst_sent.clone(),
        esm_policy: EsmPolicy::default(),
    };

    // Build log adapter
    let contest_id = contest.contest_id();
    let db_path = cli.db.as_ref().or(config.db_path.as_ref());
    let log_adapter = if let Some(db_path) = db_path {
        logger_runtime::LogAdapter::open_db(contest_id, config.my_zone, db_path)?
    } else {
        logger_runtime::LogAdapter::new(contest_id, config.my_zone)
    };

    // Two-channel bridge: hardware adapters send AppEvent, terminal sends TerminalEvent
    let (app_tx, mut app_rx) = mpsc::channel::<AppEvent>(256);
    let (tui_tx, tui_rx) = mpsc::channel::<adapters::terminal::TerminalEvent>(256);

    // Spawn terminal input reader
    adapters::terminal::spawn_terminal_reader(tui_tx.clone());

    // Optionally connect rig
    if let Some(rig_config) = &config.rig {
        match logger_runtime::spawn_rig_adapter(rig_config, app_tx.clone()).await {
            Ok(_rig) => {}
            Err(e) => warn!("rig connection failed, continuing without: {e}"),
        }
    }

    // Optionally connect keyer
    let keyer: Option<Box<dyn Keyer>> = if let Some(keyer_config) = &config.keyer {
        match logger_runtime::connect_keyer(keyer_config).await {
            Ok(k) => Some(k),
            Err(e) => {
                warn!("keyer connection failed, continuing without: {e}");
                None
            }
        }
    } else {
        None
    };

    // Optionally connect dxfeed
    if let Some(dxfeed_config) = &config.dxfeed {
        if let Err(e) = logger_runtime::spawn_dxfeed_adapter(dxfeed_config, app_tx.clone()).await {
            warn!("dxfeed connection failed, continuing without: {e}");
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

    // Load call history if configured
    let call_history: Box<dyn CallHistoryLookup> = if let Some(path) = &config.call_history_file {
        match logger_runtime::CallHistoryDb::load(path) {
            Ok(db) => Box::new(db),
            Err(e) => {
                warn!("call history load failed, continuing without: {e}");
                Box::new(NoCallHistory)
            }
        }
    } else {
        Box::new(NoCallHistory)
    };

    // Rebuild log display from restored QSOs
    let initial_log_display = adapters::log::build_log_display(&log_adapter);

    // Run the event loop
    event_loop::run(
        state,
        contest,
        macros,
        log_adapter,
        keyer,
        call_history,
        tui_rx,
        initial_log_display,
    )
    .await
}
