mod adapters;
mod config;
mod event_loop;
mod perf;
pub mod theme;
mod ui;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use logger_core::{AppEvent, RadioId, contest::BandmapCache};
use logger_runtime::{AvailSummary, ConfigValue, RateInfo, ScoreboardStatus, ScoreSummary};
use tokio::sync::mpsc;
use tracing::warn;
use logger_runtime::{Keyer, So2rSwitch};

use config::{Cli, load_config};
use ui::log_tail::LogRow;

pub use ui::export_modal::ExportStep;

pub struct ExportModal {
    pub step: ExportStep,
}

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
    /// Current SO2R RX audio routing mode. Mirrored from the event loop's
    /// `so2r_default_rx_mode` so the status panel can render L/R ear
    /// routing without reaching into the event loop. Rendered only when
    /// `so2r_configured` is true.
    pub rx_mode: logger_core::So2rRxMode,
    pub log_display: Vec<LogRow>,
    pub worked_calls: HashSet<String>,
    pub mult_calls: HashSet<String>,
    pub score: ScoreSummary,
    pub avail: AvailSummary,
    pub rate: RateInfo,
    pub error_message: Option<String>,
    /// Whether each hardware backend was *configured* in TOML (regardless of
    /// whether the connection succeeded). Used by the status bar to render a
    /// red "configured but not connected" indicator distinct from the empty
    /// "not configured at all" state.
    pub keyer_configured: bool,
    pub dxfeed_configured: bool,
    pub so2r_configured: bool,
    pub scoreboard_configured: bool,
    /// Per-radio rig connection state, keyed by configured radio_id.
    /// Empty when no rigs are configured. BTreeMap gives deterministic
    /// render order in the status bar (RIG1 before RIG2).
    pub rigs: BTreeMap<RadioId, bool>,
    pub keyer_connected: bool,
    pub dxfeed_connected: bool,
    pub so2r_connected: bool,
    pub scoreboard_status: ScoreboardStatus,
    pub export_modal: Option<ExportModal>,
    pub bandmap_mode: config::BandmapMode,
    /// Cached filtered bandmap spots keyed by (band, mode). Invalidated
    /// by `AppState.bandmap_version`. `RefCell` allows rendering (which
    /// only has `&TuiState`) to populate cache misses lazily.
    pub bandmap_cache: RefCell<BandmapCache>,
    pub theme: theme::Theme,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            echo_per_radio: HashMap::new(),
            cw_echo_enabled: false,
            cw_transmitting: false,
            tx_radio: 1,
            rx_mode: logger_core::So2rRxMode::Mono,
            log_display: Vec::new(),
            worked_calls: HashSet::new(),
            mult_calls: HashSet::new(),
            score: ScoreSummary::default(),
            avail: AvailSummary::default(),
            rate: RateInfo::default(),
            error_message: None,
            keyer_configured: false,
            dxfeed_configured: false,
            so2r_configured: false,
            rigs: BTreeMap::new(),
            keyer_connected: false,
            dxfeed_connected: false,
            so2r_connected: false,
            scoreboard_configured: false,
            scoreboard_status: ScoreboardStatus::Idle,
            export_modal: None,
            bandmap_mode: config::BandmapMode::Dual,
            bandmap_cache: RefCell::new(BandmapCache::new()),
            theme: theme::default_theme(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_file = File::create("clogger.log")?;
    let log_level = if cli.debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_max_level(log_level)
        .with_ansi(false)
        .init();

    let config = load_config(&cli)?;

    // Load theme early — bad theme_file or unknown name → fail before any
    // hardware adapters spawn, so the operator gets a clean startup error.
    let loaded_theme = theme::load(
        config.theme.as_deref(),
        config.theme_file.as_deref(),
    )?;

    // Two-channel bridge: hardware adapters send AppEvent, terminal sends TerminalEvent.
    // Created before bootstrap() so the persist task (spawned inside bootstrap when
    // db_path is set) has a channel to send PersistError events on.
    let (app_tx, mut app_rx) = mpsc::channel::<AppEvent>(256);
    let (tui_tx, tui_rx) = mpsc::channel::<adapters::terminal::TerminalEvent>(256);

    // Bootstrap session (contest, state, log adapter, call history, SCP)
    // Pull cw_speed from the first rig that has it set, fallback to keyer or default
    let default_cw_speed = config
        .rigs
        .iter()
        .find_map(|r| r.cw_speed)
        .or(config.keyer.as_ref().map(|k| k.speed_wpm))
        .unwrap_or(28);
    // Convert the typed [station] TOML table into contest-engine ConfigValues.
    // Bool/Int/String are supported; anything else (arrays, floats, datetimes)
    // is a hard error at load time — specs only accept these three types.
    let mut station_config: HashMap<String, ConfigValue> = HashMap::new();
    for (key, value) in &config.station {
        let cv = match value {
            toml::Value::Boolean(b) => ConfigValue::Bool(*b),
            toml::Value::Integer(i) => ConfigValue::Int(*i),
            toml::Value::String(s) => ConfigValue::Text(s.clone()),
            other => {
                anyhow::bail!(
                    "[station] key `{}` has unsupported TOML type `{}` — only bool, integer, and string are allowed",
                    key,
                    other.type_str()
                );
            }
        };
        station_config.insert(key.clone(), cv);
    }

    let session = logger_runtime::bootstrap(logger_runtime::SessionConfig {
        contest_id: config.contest.clone(),
        my_call: config.my_call.clone(),
        my_zone: config.my_zone,
        rst_sent: config.rst_sent.clone(),
        my_name: config.my_name.clone(),
        my_xchg: config.my_xchg.clone(),
        station_config,
        macro_overrides: config.macros,
        default_cw_speed,
        db_path: cli.db.as_ref().or(config.db_path.as_ref()).cloned(),
        call_history_path: cli.call_history.as_ref().or(config.call_history_file.as_ref()).cloned(),
        scp_path: cli.scp.as_ref().or(config.scp_file.as_ref()).cloned(),
        cty_path: cli.cty.as_ref().or(config.cty_file.as_ref()).cloned(),
        start_serial: None,
        show_passband_qrm: config.show_passband_qrm,
        esm_enabled: config.esm_enabled,
        block_dupes: config.block_dupes,
        app_tx: app_tx.clone(),
    })?;

    // Spawn rig adapters (one per configured rig, indexed by radio_id).
    // `rig_status` tracks per-radio connection state for the status bar:
    // every configured radio gets an entry, `true` if the adapter spawned
    // successfully and `false` if it failed. Each adapter returns an
    // mpsc sender for `RigCmd`s — the event loop never holds the rig
    // handle itself; control operations go through this channel to a
    // dedicated task that awaits on CI-V roundtrips.
    let mut rig_txs: HashMap<RadioId, mpsc::Sender<logger_runtime::RigCmd>> = HashMap::new();
    let mut rig_status: BTreeMap<RadioId, bool> = BTreeMap::new();
    for rig_config in &config.rigs {
        match logger_runtime::spawn_rig_adapter(rig_config, app_tx.clone()).await {
            Ok(rig_tx) => {
                rig_txs.insert(rig_config.radio_id, rig_tx);
                rig_status.insert(rig_config.radio_id, true);
            }
            Err(e) => {
                warn!("rig {} connection failed, continuing without: {e}", rig_config.radio_id);
                rig_status.insert(rig_config.radio_id, false);
            }
        }
    }

    // Spawn terminal input reader. Needs `has_second_rig` so it can suppress
    // R2-specific key bindings on single-rig setups; must happen after the
    // rig spawn loop so the rig roster is known. Keystrokes pressed before
    // this point stay buffered in the OS tty and arrive once the reader starts.
    let has_second_rig = rig_status.len() >= 2;
    adapters::terminal::spawn_terminal_reader(tui_tx.clone(), has_second_rig);

    // Optionally connect keyer (first rig's cw_speed overrides keyer speed_wpm)
    let keyer_configured = config.keyer.is_some();
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

    // Optionally connect dxfeed. Any error from `spawn_dxfeed_adapter`
    // here is a *config* problem (bad filter_file path/JSON, invalid
    // [dxfeed.skimmer_quality] values, builder validation) — not a
    // network failure. Cluster connection issues surface as runtime
    // events after spawn and don't come out of this Result. Fail fast
    // so the operator doesn't unknowingly run a contest without the
    // filtering they asked for.
    let dxfeed_configured = config.dxfeed.is_some();
    let mut dxfeed_connected = false;
    if let Some(dxfeed_config) = &config.dxfeed {
        logger_runtime::spawn_dxfeed_adapter(
            dxfeed_config,
            app_tx.clone(),
            Some(session.scp.clone()),
            session.cty.clone(),
        )
        .await
        .context("dxfeed config invalid — refusing to start")?;
        dxfeed_connected = true;
    }

    // Optionally connect OTRSP SO2R switch. The handle is shared between
    // multiple consumers via `Arc`: the event loop (for legacy direct set_tx
    // during cross-radio CwSend — Step 4 will move this into the keyer task)
    // and the so2r task (which owns SetRx commands). The underlying `otrsp`
    // crate serializes commands internally, so concurrent access is safe.
    let so2r_configured = config.so2r.is_some();
    let mut so2r_connected = false;
    let (so2r_switch, so2r_default_rx_mode): (Option<Arc<dyn So2rSwitch>>, logger_core::So2rRxMode) =
        if let Some(so2r_config) = &config.so2r {
            let mode = logger_runtime::so2r_adapter::parse_rx_mode(&so2r_config.default_rx_mode);
            match logger_runtime::so2r_adapter::connect_so2r(so2r_config).await {
                Ok(switch) => {
                    so2r_connected = true;
                    (Some(Arc::from(switch)), mode)
                }
                Err(e) => {
                    warn!("OTRSP connection failed, continuing without: {e}");
                    (None, mode)
                }
            }
        } else {
            (None, logger_core::So2rRxMode::Mono)
        };

    // Spawn the SO2R task if we have a switch. The task owns one clone of
    // the Arc and drains RX-routing commands from the channel we're about
    // to hand to the event loop.
    let so2r_tx = so2r_switch.as_ref().map(|s| {
        logger_runtime::spawn_so2r_task(Arc::clone(s), app_tx.clone())
    });

    // Initial OTRSP routing at startup: put the switch in a known state
    // before the event loop starts. Direct awaits here are fine — they
    // happen once at startup, not on the hot path. After this, all OTRSP
    // commands go through either the so2r task (RX) or the keyer task (TX).
    if let Some(switch) = so2r_switch.as_deref() {
        logger_runtime::so2r_adapter::set_tx(
            Some(switch),
            session.state.focused_radio,
        )
        .await;
        logger_runtime::so2r_adapter::set_rx(
            Some(switch),
            session.state.focused_radio,
            so2r_default_rx_mode,
        )
        .await;
    }

    // Spawn the keyer task. Transfers ownership of the `Box<dyn Keyer>`
    // and clones the so2r_switch Arc so the task can handle cross-radio
    // TX routing atomically (abort → set_tx → settle → send). The keyer
    // subscription (for CW echo events) was already taken above — it's a
    // broadcast receiver, separate from the command channel.
    let initial_tx_radio = session.state.focused_radio;
    let keyer_tx = keyer.map(|k| {
        logger_runtime::spawn_keyer_task(
            k,
            so2r_switch.as_ref().map(Arc::clone),
            initial_tx_radio,
            app_tx.clone(),
        )
    });

    // Optionally spawn scoreboard adapter
    let scoreboard_configured = !config.scoreboard.endpoints.is_empty();
    let scoreboard_handle = if scoreboard_configured {
        let category = config
            .category
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("[category] is required when [[scoreboard.endpoints]] is configured"))?;
        let cabrillo_id = session
            .contest
            .cabrillo_id(category.mode.to_category_mode())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "contest '{}' has no Cabrillo ID for mode '{}'",
                    config.contest,
                    category.mode.as_str()
                )
            })?;
        Some((
            logger_runtime::spawn_scoreboard_adapter(logger_runtime::ScoreboardConfig {
                endpoints: config.scoreboard.endpoints,
                interval_secs: config.scoreboard.interval_secs,
            }),
            cabrillo_id,
            config.my_call.clone(),
            category.clone(),
        ))
    } else {
        None
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
        rig_txs,
        keyer_tx,
        keyer_rx,
        cw_echo_enabled,
        config.cursor_style,
        session.call_history,
        session.scp,
        tui_rx,
        initial_log_display,
        ConnectionStatus {
            rigs: rig_status,
            keyer_configured,
            keyer_connected,
            dxfeed_configured,
            dxfeed_connected,
            so2r_configured,
            so2r_connected,
            scoreboard_configured,
            bandmap_mode: config.bandmap,
        },
        so2r_tx,
        so2r_default_rx_mode,
        scoreboard_handle,
        loaded_theme,
    )
    .await
}

pub struct ConnectionStatus {
    /// Per-radio rig connection state, keyed by configured radio_id.
    /// Empty when no rigs are configured.
    pub rigs: BTreeMap<RadioId, bool>,
    pub keyer_configured: bool,
    pub keyer_connected: bool,
    pub dxfeed_configured: bool,
    pub dxfeed_connected: bool,
    pub so2r_configured: bool,
    pub so2r_connected: bool,
    pub scoreboard_configured: bool,
    pub bandmap_mode: config::BandmapMode,
}
