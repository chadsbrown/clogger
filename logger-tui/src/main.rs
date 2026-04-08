mod adapters;
mod config;
mod event_loop;
mod ui;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use logger_core::{AppEvent, RadioId};
use logger_runtime::{AvailSummary, RateInfo, ScoreSummary};
use tokio::sync::mpsc;
use tracing::warn;
use logger_runtime::{Keyer, Rig, So2rSwitch};

use config::{Cli, load_config};
use ui::log_tail::LogRow;

pub struct TuiState {
    /// What to display in the CW line of each radio's entry.
    ///
    /// When live echo is enabled, this is empty at the start of a transmission
    /// and gets filled character-by-character as the keyer echoes them back.
    /// When live echo is disabled, this is set to the markers-stripped version
    /// of the sent text immediately on CwSend.
    ///
    /// Either way, this contains only printable CW characters — no `{NN}`
    /// speed control markers.
    pub echo_per_radio: HashMap<RadioId, String>,
    pub cw_echo_enabled: bool,
    pub cw_transmitting: bool,
    /// Which radio is currently routed for TX (independent of entry focus).
    /// Updated by dispatch_effects when CwSend is sent to a different radio.
    pub tx_radio: RadioId,
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

impl Default for TuiState {
    fn default() -> Self {
        Self {
            echo_per_radio: HashMap::new(),
            cw_echo_enabled: false,
            cw_transmitting: false,
            tx_radio: 1,
            log_display: Vec::new(),
            worked_calls: HashSet::new(),
            mult_calls: HashSet::new(),
            score: ScoreSummary::default(),
            avail: AvailSummary::default(),
            rate: RateInfo::default(),
            error_message: None,
            rig_connected: false,
            keyer_connected: false,
            dxfeed_connected: false,
        }
    }
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
    // Pull cw_speed from the first rig that has it set, fallback to keyer or default
    let default_cw_speed = config
        .rigs
        .iter()
        .find_map(|r| r.cw_speed)
        .or(config.keyer.as_ref().map(|k| k.speed_wpm))
        .unwrap_or(28);
    let session = logger_runtime::bootstrap(logger_runtime::SessionConfig {
        contest_id: config.contest.clone(),
        my_call: config.my_call.clone(),
        my_zone: config.my_zone,
        rst_sent: config.rst_sent.clone(),
        my_name: config.my_name.clone(),
        my_xchg: config.my_xchg.clone(),
        macro_overrides: config.macros,
        default_cw_speed,
        db_path: cli.db.as_ref().or(config.db_path.as_ref()).cloned(),
        call_history_path: cli.call_history.as_ref().or(config.call_history_file.as_ref()).cloned(),
        scp_path: cli.scp.as_ref().or(config.scp_file.as_ref()).cloned(),
        start_serial: None,
    })?;

    // Two-channel bridge: hardware adapters send AppEvent, terminal sends TerminalEvent
    let (app_tx, mut app_rx) = mpsc::channel::<AppEvent>(256);
    let (tui_tx, tui_rx) = mpsc::channel::<adapters::terminal::TerminalEvent>(256);

    // Spawn terminal input reader
    adapters::terminal::spawn_terminal_reader(tui_tx.clone());

    // Spawn rig adapters (one per configured rig, indexed by radio_id)
    let mut rigs: HashMap<RadioId, Arc<dyn Rig>> = HashMap::new();
    let mut rig_connected = false;
    for rig_config in &config.rigs {
        match logger_runtime::spawn_rig_adapter(rig_config, app_tx.clone()).await {
            Ok(rig) => {
                rigs.insert(rig_config.radio_id, rig);
                rig_connected = true;
            }
            Err(e) => warn!("rig {} connection failed, continuing without: {e}", rig_config.radio_id),
        }
    }

    // Optionally connect keyer (first rig's cw_speed overrides keyer speed_wpm)
    let mut keyer_connected = false;
    let rig_cw_speed = config.rigs.iter().find_map(|r| r.cw_speed);
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

    // Optionally connect OTRSP SO2R switch
    let (so2r_switch, so2r_default_rx_mode): (Option<Box<dyn So2rSwitch>>, logger_core::So2rRxMode) =
        if let Some(so2r_config) = &config.so2r {
            let mode = logger_runtime::so2r_adapter::parse_rx_mode(&so2r_config.default_rx_mode);
            match logger_runtime::so2r_adapter::connect_so2r(so2r_config).await {
                Ok(switch) => (Some(switch), mode),
                Err(e) => {
                    warn!("OTRSP connection failed, continuing without: {e}");
                    (None, mode)
                }
            }
        } else {
            (None, logger_core::So2rRxMode::Mono)
        };

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
        rigs,
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
        so2r_switch,
        so2r_default_rx_mode,
    )
    .await
}
