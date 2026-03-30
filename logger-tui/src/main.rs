mod adapters;
mod config;
mod event_loop;
mod ui;

use std::collections::HashMap;

use anyhow::Result;
use clap::Parser;
use logger_core::{
    AppState, EntryState, EsmPolicy, contest_from_id,
};
use tokio::sync::mpsc;
use tracing::warn;
use winkey::Keyer;

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
        adapters::log::LogAdapter::open_db(contest_id, config.my_zone, db_path)?
    } else {
        adapters::log::LogAdapter::new(contest_id, config.my_zone)
    };

    // Event channel
    let (tx, rx) = mpsc::channel::<adapters::terminal::TerminalEvent>(256);

    // Spawn terminal input reader
    adapters::terminal::spawn_terminal_reader(tx.clone());

    // Optionally connect rig
    if let Some(rig_config) = &config.rig {
        match adapters::rig::spawn_rig_adapter(rig_config, tx.clone()).await {
            Ok(_rig) => {}
            Err(e) => warn!("rig connection failed, continuing without: {e}"),
        }
    }

    // Optionally connect keyer
    let keyer: Option<Box<dyn Keyer>> = if let Some(keyer_config) = &config.keyer {
        match adapters::keyer::connect_keyer(keyer_config).await {
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
        if let Err(e) = adapters::dxfeed::spawn_dxfeed_adapter(dxfeed_config, tx.clone()).await {
            warn!("dxfeed connection failed, continuing without: {e}");
        }
    }

    // Rebuild log display from restored QSOs
    let initial_log_display = adapters::log::build_log_display(&log_adapter);

    // Run the event loop
    event_loop::run(state, contest, macros, log_adapter, keyer, rx, initial_log_display).await
}
