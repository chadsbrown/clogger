//! Temporary instrumentation used to diagnose typing lag in the entry
//! pane. NOT part of the long-term API — a grep-able marker comment
//! (`// PROFILING`) sits next to every call site so the whole set can be
//! ripped out in one pass.
//!
//! # Use
//!
//! Build normally, then:
//!
//! ```text
//! RUST_LOG=perf=debug cargo run -p logger-gui -- -c config.toml --contest cwt.toml
//! ```
//!
//! Or just `--debug` (the default filter enables this target at debug
//! level when `cli.debug` is set). Events look like:
//!
//! ```text
//! DEBUG perf: span=pane.entry elapsed_us=142
//! DEBUG perf: span=reducer.step elapsed_us=2184
//! DEBUG perf: span=view.total elapsed_us=9821
//! ```
//!
//! # Removal checklist
//!
//! Instrumented sites (search `// PROFILING` to confirm):
//!
//! - `src/main.rs` — `update(Message::Domain)` wraps `bridge::step` and
//!   `bridge::dispatch`; `update(Message::Tick)` wraps `bridge::step`;
//!   `view()` wraps the whole render
//! - `src/panes/entry.rs::view`
//! - `src/panes/bandmap.rs::view`
//! - `src/panes/log.rs::view`
//! - `src/panes/score.rs::view`
//! - `src/panes/rate.rs::view`
//! - `src/panes/available.rs::view`
//! - `src/panes/condx.rs::view`
//! - `src/panes/macros.rs::view`
//! - `src/panes/so2r.rs::view`
//!
//! To rip out:
//!
//! 1. Delete every line containing `// PROFILING` plus the `let _perf …`
//!    binding it marks (they're always paired on one line).
//! 2. Delete `mod perf;` from `src/main.rs`.
//! 3. Delete `perf=debug` from the default tracing filter in
//!    `src/main.rs::main()`.
//! 4. Delete this file.

use std::time::Instant;

pub struct Span {
    name: &'static str,
    start: Instant,
}

impl Span {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let elapsed_us = self.start.elapsed().as_micros() as u64;
        // Target distinct from our other tracing output so it's easy to
        // isolate with `RUST_LOG=perf=debug`.
        tracing::debug!(target: "perf", span = self.name, elapsed_us);
    }
}
