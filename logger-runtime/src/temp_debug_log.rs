//! TEMP-DEBUG (remove-me).
//!
//! Scratch append-to-file logger used during the initial RTC rollout so
//! we can inspect the full XML bodies posted to scoreboard and RTC
//! endpoints without drowning the tracing output. Writes to a file in
//! the current working directory.
//!
//! # Removal
//!
//! When the RTC adapter is known-stable:
//!   1. Delete this file (`logger-runtime/src/temp_debug_log.rs`).
//!   2. Drop the `pub mod temp_debug_log;` line from `lib.rs`.
//!   3. Remove the two `temp_debug_log::append(...)` call sites
//!      (grep for `temp_debug_log` or `TEMP-DEBUG`).
//!
//! All three edits are self-contained; nothing else depends on this
//! module.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Append `body` to `path`, preceded by a UTC timestamp separator.
/// All errors are silently swallowed — this is debug scaffolding and
/// the adapters must never fail because of a log write.
pub fn append(path: impl AsRef<Path>, body: &str) {
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let line = format!("=== {ts} ===\n{body}\n");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
