pub mod adif_export;
pub mod analytics;
pub mod bootstrap;
pub mod call_history;
pub mod condx_adapter;
pub mod config;
pub mod contest_history;
pub mod cty;
pub mod dxfeed_adapter;
pub mod dxfeed_enrichment;
pub mod dxfeed_entity;
pub mod keyer_adapter;
pub mod keyer_task;
pub mod log_adapter;
pub mod persist_task;
pub mod rig_adapter;
pub mod scoring;
pub mod scp;
pub mod scoreboard_adapter;
pub mod rtc_adapter;
pub mod so2r_adapter;
pub mod so2r_task;

pub use call_history::CallHistoryDb;
pub use condx_adapter::{BandCondition, CondXConfig, CondXSnapshot, spawn_condx_adapter};
pub use cty::CtyDb;
pub use scp::ScpDb;
pub use config::{
    CabrilloConfig, CategoryConfig, CategoryConfigMode, ScoreboardEndpoint,
    DxFeedConfig, DxFeedSourceConfig, KeyerConfig, RigConfig, RtcConfig, So2rConfig,
};
pub use dxfeed_adapter::spawn_dxfeed_adapter;
pub use keyer_adapter::{abort_cw, connect_keyer, send_cw};
pub use keyer_task::{KeyerCmd, spawn_keyer_task};
pub use log_adapter::{LogAdapter, LogSnapshot, decode_exchange_pairs};
pub use persist_task::spawn_persist_task;
pub use rig_adapter::{RigCmd, spawn_rig_adapter};
pub use so2r_task::{So2rCmd, spawn_so2r_task};
pub use analytics::{AvailSummary, RateInfo, WorkedCalls, compute_avail, compute_rate, compute_worked_calls};
pub use bootstrap::{MacroOverrides, Session, SessionConfig, bootstrap};
pub use scoring::{BandScore, BreakdownRow, ScoreBreakdown, ScoreSummary, scorer_for_contest};

/// Typed config value consumed by contest-engine specs. Re-exported so
/// frontends (TUI, CLI) can build `station_config` maps without a direct
/// contest-engine dep.
pub use contest_engine::spec::Value as ConfigValue;
pub use scoreboard_adapter::{
    ScoreboardConfig, ScoreboardSnapshot, ScoreboardStatus, spawn_scoreboard_adapter,
};

// Re-export qsolog types needed by consumers of LogAdapter
pub use qsolog::qso::{ExchangeBlob, QsoRecord};
pub use qsolog::types::{Band, Mode};

// Re-export winkey types so TUI doesn't need a direct winkey dep
pub use winkey::Keyer;
pub use winkey::KeyerEvent;

// Re-export riglib types so TUI doesn't need a direct riglib dep
pub use riglib::Rig;
pub use riglib::ReceiverId;

// Re-export otrsp types so TUI doesn't need a direct otrsp dep
pub use otrsp::So2rSwitch;
pub use otrsp::Radio as OtrspRadio;
