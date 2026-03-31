pub mod call_history;
pub mod config;
pub mod dxfeed_adapter;
pub mod keyer_adapter;
pub mod log_adapter;
pub mod rig_adapter;
pub mod scoring;
pub mod scp;

pub use call_history::CallHistoryDb;
pub use scp::ScpDb;
pub use config::{DxFeedConfig, DxFeedSourceConfig, KeyerConfig, RigConfig};
pub use dxfeed_adapter::spawn_dxfeed_adapter;
pub use keyer_adapter::{connect_keyer, send_cw};
pub use log_adapter::{LogAdapter, decode_exchange_pairs};
pub use rig_adapter::spawn_rig_adapter;
pub use scoring::{BandScore, ScoreSummary, scorer_for_contest};

// Re-export qsolog types needed by consumers of LogAdapter
pub use qsolog::qso::{ExchangeBlob, QsoRecord};
pub use qsolog::types::{Band, Mode};

// Re-export winkey::Keyer so TUI doesn't need a direct winkey dep
pub use winkey::Keyer;

// Re-export riglib types so TUI doesn't need a direct riglib dep
pub use riglib::Rig;
pub use riglib::ReceiverId;
