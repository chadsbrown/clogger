mod adapters;
mod config;
mod event_loop;
mod ui;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use logger_core::{AppEvent, AppState, CallHistoryLookup, EntryState, EsmPolicy, NoCallHistory, NoScp, ScpLookup, contest_from_id};
use tokio::sync::mpsc;
use tracing::warn;
use logger_runtime::{Keyer, Rig, ScoreSummary};

use config::{Cli, load_config};
use ui::log_tail::LogRow;

#[derive(Default)]
pub struct TuiState {
    pub cw_history: Vec<String>,
    pub log_display: Vec<LogRow>,
    pub worked_calls: HashSet<String>,
    pub score: ScoreSummary,
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

    // Build contest + macros
    let contest = contest_from_id(&config.contest)
        .ok_or_else(|| anyhow::anyhow!("unknown contest: {}", config.contest))?;
    let macros = contest.default_macros();

    // Build initial state
    let mut my_exchange = HashMap::new();
    if let Some(name) = &config.my_name {
        my_exchange.insert("NAME".to_string(), name.clone());
    }
    if let Some(xchg) = &config.my_xchg {
        my_exchange.insert("XCHG".to_string(), xchg.clone());
    }

    // Build log adapter (scorer needs my_exchange before it's moved into state)
    let scorer = logger_runtime::scorer_for_contest(contest.as_ref(), config.my_zone, &my_exchange);

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
        my_exchange,
        esm_policy: EsmPolicy::default(),
        bandmap_cursor: None,
    };
    let db_path = cli.db.as_ref().or(config.db_path.as_ref());
    let log_adapter = if let Some(db_path) = db_path {
        logger_runtime::LogAdapter::open_db(scorer, db_path)?
    } else {
        logger_runtime::LogAdapter::new(scorer)
    };

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

    // Optionally connect keyer
    let mut keyer_connected = false;
    let keyer: Option<Box<dyn Keyer>> = if let Some(keyer_config) = &config.keyer {
        match logger_runtime::connect_keyer(keyer_config).await {
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

    // Load call history if configured (CLI flag overrides config)
    let ch_path = cli.call_history.as_ref().or(config.call_history_file.as_ref());
    let call_history: Box<dyn CallHistoryLookup> = if let Some(path) = ch_path {
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

    // Load SCP file if configured (CLI flag overrides config)
    let scp_path = cli.scp.as_ref().or(config.scp_file.as_ref());
    let scp: Box<dyn ScpLookup> = if let Some(path) = scp_path {
        match logger_runtime::ScpDb::load(path) {
            Ok(db) => Box::new(db),
            Err(e) => {
                warn!("SCP file load failed, continuing without: {e}");
                Box::new(NoScp)
            }
        }
    } else {
        Box::new(NoScp)
    };

    // Rebuild log display from restored QSOs
    let initial_log_display = adapters::log::build_log_display(&log_adapter);

    // Run the event loop
    event_loop::run(
        state,
        contest,
        macros,
        log_adapter,
        rig_handle,
        keyer,
        call_history,
        scp,
        tui_rx,
        initial_log_display,
        rig_connected,
        keyer_connected,
        dxfeed_connected,
    )
    .await
}
