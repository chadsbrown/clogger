//! Lightweight event-loop instrumentation for performance work.
//!
//! Records recent samples of three latency metrics:
//!
//! - `reduce_dispatch` — time from receiving a `TerminalEvent::App` to
//!   finishing `reduce()` + `dispatch_effects()`. The keystroke hot path.
//! - `render` — time spent inside a single `terminal.draw(...)` call. The
//!   per-frame budget (20 Hz = 50 ms).
//! - `analytics` — time spent inside `recompute_analytics` when it fires.
//!
//! Each metric keeps a ring buffer of up to [`SAMPLE_CAPACITY`] samples so
//! old data falls off automatically. On shutdown, summary percentiles are
//! written to the `tracing` log at debug level — surface them by running
//! with `--debug`.
//!
//! Overhead is a single `Instant::now()` pair per event (~20-50 ns total)
//! plus one `VecDeque::push_back` / conditional `pop_front`. Safe to leave
//! always-on.

use std::collections::VecDeque;
use std::time::Duration;

use tracing::debug;

const SAMPLE_CAPACITY: usize = 512;

pub struct PerfStats {
    pub reduce_dispatch: RingSamples,
    pub render: RingSamples,
    pub analytics: RingSamples,
}

impl PerfStats {
    pub fn new() -> Self {
        Self {
            reduce_dispatch: RingSamples::new("reduce+dispatch"),
            render: RingSamples::new("render"),
            analytics: RingSamples::new("analytics"),
        }
    }

    /// Emit a summary of all three metrics to the tracing log at debug level.
    /// Visible only when the TUI is run with `--debug`. Intended to be called
    /// once during shutdown.
    pub fn log_summary(&self) {
        debug!("perf summary (last {} samples per metric):", SAMPLE_CAPACITY);
        self.reduce_dispatch.log_summary();
        self.render.log_summary();
        self.analytics.log_summary();
    }
}

pub struct RingSamples {
    label: &'static str,
    samples: VecDeque<Duration>,
}

impl RingSamples {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            samples: VecDeque::with_capacity(SAMPLE_CAPACITY),
        }
    }

    /// Record one elapsed duration. O(1) amortized — when the ring is full,
    /// the oldest sample is dropped.
    pub fn record(&mut self, d: Duration) {
        if self.samples.len() == SAMPLE_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(d);
    }

    fn log_summary(&self) {
        if self.samples.is_empty() {
            debug!("  {}: no samples", self.label);
            return;
        }
        let mut sorted: Vec<Duration> = self.samples.iter().copied().collect();
        sorted.sort();
        let n = sorted.len();
        let min = sorted[0];
        let max = sorted[n - 1];
        let sum: Duration = sorted.iter().sum();
        let mean = sum / n as u32;
        let p50 = sorted[n / 2];
        let p95 = sorted[(n * 95) / 100];
        let p99 = sorted[((n * 99) / 100).min(n - 1)];

        debug!(
            "  {}: n={} min={:?} mean={:?} p50={:?} p95={:?} p99={:?} max={:?}",
            self.label, n, min, mean, p50, p95, p99, max
        );
    }
}
