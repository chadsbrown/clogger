//! Cached analytics for the bandmap, available, and rate panes.
//!
//! The bandmap pane filters spots and scans the log for dupe/mult state
//! per render; the available pane does the same across six bands; the
//! rate pane walks the whole log. At 10–12k QSOs this is meaningful
//! work to repeat on every keypress, spot event, keyer echo, and timer
//! tick.
//!
//! This module holds a `PaneAnalytics` struct on `App` that precomputes
//! those results in `update()` and skips the work when none of the
//! tracked inputs have changed (bandmap version, score epoch, per-radio
//! freq/mode, focused radio). The canvas/pane `view` fns read the
//! prepared results instead of recomputing.

use std::collections::HashSet;
use std::sync::Arc;

use logger_core::{
    AppState, DupeChecker, MultChecker, RadioId, Spot,
    contest::{BandmapCache, freq_to_band_label, normalize_mode},
};
use logger_runtime::{AvailSummary, LogAdapter, compute_avail};

/// Per-radio bandmap-pane inputs: the spot list (for rendering and
/// click-to-snap) and the precomputed worked/mult call sets.
///
/// Wrapped in `Arc` so the bandmap pane can hand them off to its
/// `BandmapProgram` via cheap pointer clones each render rather than
/// deep-cloning the spot vector and both hash sets every time.
pub struct RadioAnalytics {
    pub filtered_spots: Arc<Vec<Spot>>,
    pub worked: Arc<HashSet<String>>,
    pub mults: Arc<HashSet<String>>,
}

/// Rate-pane metrics. Computed by walking the log's record tail without
/// allocating a separate timestamp vector.
#[derive(Default)]
pub struct RateMetrics {
    pub r10_per_hour: f64,
    pub r60_per_hour: f64,
    pub last_5_per_hour: Option<f64>,
    pub secs_since_last: Option<u64>,
}

#[derive(Default)]
pub struct PaneAnalytics {
    bandmap_cache: BandmapCache,
    pub r1: Option<RadioAnalytics>,
    pub r2: Option<RadioAnalytics>,
    pub avail: AvailSummary,
    pub rate: RateMetrics,
    /// Wall-clock ms the rate metrics were last computed for. The rate
    /// pane re-renders on every 1 Hz tick because `secs_since_last`
    /// ticks up; we refresh rate when `now_ms / 1000` crosses to a new
    /// second even if nothing else changed.
    rate_now_sec: i64,
    last_bandmap_version: u64,
    last_score_epoch: u64,
    last_r1_freq: u64,
    last_r1_mode: &'static str,
    last_r2_freq: u64,
    last_r2_mode: &'static str,
    last_focused_radio: RadioId,
    last_show_r2: bool,
    fresh: bool,
}

impl PaneAnalytics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recompute cached results when any tracked input has changed.
    /// Cheap when nothing moved.
    pub fn refresh(
        &mut self,
        state: &AppState,
        log: &LogAdapter,
        show_r2: bool,
        now_ms: i64,
    ) {
        let r1 = state.radios.get(&1);
        let r1_freq = r1.map(|r| r.freq_hz).unwrap_or(0);
        let r1_mode_raw = r1.map(|r| r.mode.as_str()).unwrap_or("CW");
        let r1_mode = normalize_mode(r1_mode_raw);

        let r2 = state.radios.get(&2);
        let r2_freq = r2.map(|r| r.freq_hz).unwrap_or(0);
        let r2_mode_raw = r2.map(|r| r.mode.as_str()).unwrap_or("CW");
        let r2_mode = normalize_mode(r2_mode_raw);

        let focused = state.focused_radio;
        let bandmap_version = state.bandmap_version;
        let score_epoch = log.score_epoch();
        let rate_now_sec = now_ms / 1000;

        let pane_inputs_changed = !self.fresh
            || self.last_bandmap_version != bandmap_version
            || self.last_score_epoch != score_epoch
            || self.last_r1_freq != r1_freq
            || self.last_r1_mode != r1_mode
            || self.last_r2_freq != r2_freq
            || self.last_r2_mode != r2_mode
            || self.last_focused_radio != focused
            || self.last_show_r2 != show_r2;

        let rate_stale = !self.fresh
            || self.last_score_epoch != score_epoch
            || self.rate_now_sec != rate_now_sec;

        if !pane_inputs_changed && !rate_stale {
            return;
        }

        if pane_inputs_changed {
            self.r1 = if r1_freq > 0 {
                Some(compute_radio(
                    &mut self.bandmap_cache,
                    state,
                    log,
                    r1_freq,
                    r1_mode,
                ))
            } else {
                None
            };

            self.r2 = if show_r2 && r2_freq > 0 {
                Some(compute_radio(
                    &mut self.bandmap_cache,
                    state,
                    log,
                    r2_freq,
                    r2_mode,
                ))
            } else {
                None
            };

            let focused_mode = state
                .radios
                .get(&focused)
                .map(|r| r.mode.as_str())
                .unwrap_or("CW");
            self.avail = compute_avail(
                &state.bandmap,
                bandmap_version,
                &mut self.bandmap_cache,
                focused_mode,
                log,
            );
        }

        if rate_stale {
            let now_ms_u = now_ms.max(0) as u64;
            self.rate = compute_rate_metrics(log, now_ms_u);
            self.rate_now_sec = rate_now_sec;
        }

        self.last_bandmap_version = bandmap_version;
        self.last_score_epoch = score_epoch;
        self.last_r1_freq = r1_freq;
        self.last_r1_mode = r1_mode;
        self.last_r2_freq = r2_freq;
        self.last_r2_mode = r2_mode;
        self.last_focused_radio = focused;
        self.last_show_r2 = show_r2;
        self.fresh = true;
    }
}

fn compute_radio(
    cache: &mut BandmapCache,
    state: &AppState,
    log: &LogAdapter,
    freq_hz: u64,
    mode: &'static str,
) -> RadioAnalytics {
    let band = freq_to_band_label(freq_hz);
    let spots: Vec<Spot> = cache
        .get_or_build(&state.bandmap, state.bandmap_version, band, mode)
        .to_vec();

    let mut worked = HashSet::new();
    let mut mults = HashSet::new();
    for spot in &spots {
        let call_norm = spot.call.to_ascii_uppercase();
        if log.is_dupe(&call_norm, band, mode) {
            worked.insert(spot.call.clone());
        } else if log.is_new_mult(&call_norm, band, mode) {
            mults.insert(spot.call.clone());
        }
    }
    RadioAnalytics {
        filtered_spots: Arc::new(spots),
        worked: Arc::new(worked),
        mults: Arc::new(mults),
    }
}

fn compute_rate_metrics(log: &LogAdapter, now_ms: u64) -> RateMetrics {
    let ts_iter = log
        .records()
        .iter()
        .filter(|r| !r.flags.is_void)
        .map(|r| r.ts_ms);
    compute_rate_metrics_from_iter(ts_iter, now_ms)
}

/// Walk non-void QSO timestamps (in insertion order, i.e. ascending)
/// backward to extract the metrics the rate pane needs, without
/// allocating a timestamps vector. Inserts are monotonic
/// non-decreasing by `ts_ms`, so we can early-exit as soon as we cross
/// the 60-min cutoff and have already captured the 5th-from-end
/// timestamp.
fn compute_rate_metrics_from_iter<I>(ts_ascending: I, now_ms: u64) -> RateMetrics
where
    I: DoubleEndedIterator<Item = u64>,
{
    let cutoff_10 = now_ms.saturating_sub(10 * 60_000);
    let cutoff_60 = now_ms.saturating_sub(60 * 60_000);

    let mut r10_count = 0u64;
    let mut r60_count = 0u64;
    let mut last_ts: Option<u64> = None;
    let mut fifth_from_end: Option<u64> = None;
    let mut seen_from_end = 0u64;

    for ts in ts_ascending.rev() {
        if last_ts.is_none() {
            last_ts = Some(ts);
        }
        seen_from_end += 1;
        if seen_from_end == 5 {
            fifth_from_end = Some(ts);
        }
        if ts >= cutoff_60 {
            r60_count += 1;
            if ts >= cutoff_10 {
                r10_count += 1;
            }
        } else if seen_from_end >= 5 {
            break;
        }
    }

    let r10_per_hour = r10_count as f64 * 60.0 / 10.0;
    let r60_per_hour = r60_count as f64 * 60.0 / 60.0;
    let last_5_per_hour = fifth_from_end.and_then(|anchor| {
        let elapsed_min = now_ms.saturating_sub(anchor) as f64 / 60_000.0;
        if elapsed_min <= 0.0 {
            None
        } else {
            Some(5.0 / elapsed_min * 60.0)
        }
    });
    let secs_since_last = last_ts.map(|t| now_ms.saturating_sub(t) / 1000);

    RateMetrics {
        r10_per_hour,
        r60_per_hour,
        last_5_per_hour,
        secs_since_last,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_704_067_200_000;
    const MIN: u64 = 60_000;

    fn metrics(ts_asc: &[u64], now_ms: u64) -> RateMetrics {
        compute_rate_metrics_from_iter(ts_asc.iter().copied(), now_ms)
    }

    #[test]
    fn empty_log() {
        let m = metrics(&[], NOW);
        assert_eq!(m.r10_per_hour, 0.0);
        assert_eq!(m.r60_per_hour, 0.0);
        assert!(m.last_5_per_hour.is_none());
        assert!(m.secs_since_last.is_none());
    }

    #[test]
    fn windowed_rates_exclude_stale_qsos() {
        let ts: Vec<u64> = (0..5).map(|i| NOW - 30 * MIN - i as u64 * MIN).rev().collect();
        let m = metrics(&ts, NOW);
        assert_eq!(m.r10_per_hour, 0.0);
        assert_eq!(m.r60_per_hour, 5.0);
    }

    #[test]
    fn windowed_rates_project_hourly() {
        let ts = vec![NOW - 9 * MIN, NOW - 7 * MIN, NOW - 5 * MIN, NOW - 3 * MIN, NOW - MIN];
        let m = metrics(&ts, NOW);
        assert_eq!(m.r10_per_hour, 30.0);
        assert_eq!(m.r60_per_hour, 5.0);
    }

    #[test]
    fn last_5_includes_idle_gap() {
        let burst_end = NOW - 30 * MIN;
        let ts = vec![
            burst_end - 5 * MIN,
            burst_end - 4 * MIN,
            burst_end - 3 * MIN,
            burst_end - MIN,
            burst_end,
        ];
        let m = metrics(&ts, NOW);
        let r5 = m.last_5_per_hour.unwrap();
        assert!((r5 - 8.571).abs() < 0.01, "got {r5}");
    }

    #[test]
    fn last_5_needs_five_qsos() {
        let ts = vec![NOW - 10 * MIN, NOW - 5 * MIN, NOW - MIN];
        assert!(metrics(&ts, NOW).last_5_per_hour.is_none());
    }

    #[test]
    fn user_scenario_last_qso_30min_ago() {
        let burst_end = NOW - 30 * MIN;
        let ts: Vec<u64> = (0..10).map(|i| burst_end - i as u64 * MIN).rev().collect();
        let m = metrics(&ts, NOW);
        assert_eq!(m.r10_per_hour, 0.0);
        assert_eq!(m.r60_per_hour, 10.0);
        let r5 = m.last_5_per_hour.unwrap();
        assert!((r5 - 8.823).abs() < 0.01, "got {r5}");
        assert_eq!(m.secs_since_last, Some(30 * 60));
    }
}
