//! Rate pane — QSO cadence at a glance.
//!
//! Three different "rate" metrics, each useful at a different timescale:
//!
//! * **Last 10 min** — wall-clock window. Count of QSOs logged in the past
//!   10 minutes, projected to QSOs/hour. Reflects right-now activity; goes
//!   to zero immediately when you stop logging.
//! * **Last 60 min** — same idea, 60-minute window. Smoother; gives you the
//!   "actual hour" rate that's robust to short bursts and lulls.
//! * **Last 5 Qs** — pace over the most recent 5 QSOs. Computed as
//!   `5 / (now - timestamp_of_5th_from_end_in_hours)`. Includes the dead
//!   time since the last QSO, so the figure decays as you sit idle. `None`
//!   when fewer than 5 QSOs have been logged.
//! * **Since last Q** — wall-clock seconds since the most recent QSO.
//!
//! The runtime's `compute_rate` returns "minutes elapsed over last N QSOs"
//! which is a different (and complementary) metric; we deliberately don't
//! use it here.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_runtime::LogAdapter;

use super::style;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let timestamps: Vec<u64> = log
        .ordered_records()
        .iter()
        .filter(|r| !r.flags.is_void)
        .map(|r| r.ts_ms)
        .collect();

    let r10 = rate_in_window(&timestamps, now_ms, 10);
    let r60 = rate_in_window(&timestamps, now_ms, 60);
    let r5 = rate_over_last_n_qsos(&timestamps, now_ms, 5);
    let since = secs_since_last(&timestamps, now_ms);

    let line = |label: &'static str, value: String| -> Element<'a, M> {
        row![
            text(label)
                .size(style::TEXT_BODY)
                .style(style::muted)
                .width(Length::Fixed(110.0)),
            text(value).size(style::TEXT_VALUE).style(style::body),
        ]
        .spacing(4)
        .into()
    };

    container(
        column![
            line("Last 10 min", fmt_rate(Some(r10))),
            Space::new().height(2),
            line("Last 60 min", fmt_rate(Some(r60))),
            Space::new().height(2),
            line("Last 5 Qs", fmt_rate(r5)),
            Space::new().height(2),
            line("Since last Q", fmt_since(since)),
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// QSOs logged within the last `window_min` minutes, projected to a
/// per-hour rate. Always defined (zero when no QSOs in window). Window
/// boundaries are inclusive of `now_ms - window_min * 60s` and earlier
/// QSOs are excluded.
fn rate_in_window(timestamps: &[u64], now_ms: u64, window_min: u64) -> f64 {
    let cutoff = now_ms.saturating_sub(window_min * 60_000);
    let count = timestamps.iter().filter(|&&t| t >= cutoff).count();
    count as f64 * 60.0 / window_min as f64
}

/// Pace over the most recent `n` QSOs: `n / (now - 5th-from-end) * 60`.
/// Includes the gap since the last QSO so the figure decays during a lull.
/// `None` when fewer than `n` QSOs are logged or the elapsed window is zero.
fn rate_over_last_n_qsos(timestamps: &[u64], now_ms: u64, n: usize) -> Option<f64> {
    if timestamps.len() < n {
        return None;
    }
    let anchor = timestamps[timestamps.len() - n];
    let elapsed_min = (now_ms.saturating_sub(anchor)) as f64 / 60_000.0;
    if elapsed_min <= 0.0 {
        return None;
    }
    Some(n as f64 / elapsed_min * 60.0)
}

fn secs_since_last(timestamps: &[u64], now_ms: u64) -> Option<u64> {
    timestamps.last().map(|&t| now_ms.saturating_sub(t) / 1000)
}

fn fmt_rate(r: Option<f64>) -> String {
    match r {
        None => "—".to_string(),
        Some(r) => format!("{:.0}/hr", r),
    }
}

fn fmt_since(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        Some(s) => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `now` we can do clean math against. 2024-01-01 00:00:00 UTC = 1704067200_000.
    const NOW: u64 = 1_704_067_200_000;
    const MIN: u64 = 60_000;

    #[test]
    fn windowed_rate_zero_when_empty() {
        assert_eq!(rate_in_window(&[], NOW, 10), 0.0);
        assert_eq!(rate_in_window(&[], NOW, 60), 0.0);
    }

    #[test]
    fn windowed_rate_skips_qsos_outside_window() {
        // Five QSOs all 30 min ago — none in the 10-min window, all in 60-min.
        let ts: Vec<u64> = (0..5).map(|i| NOW - 30 * MIN - i as u64 * MIN).collect();
        assert_eq!(rate_in_window(&ts, NOW, 10), 0.0);
        // 5 in 60 min = 5/hr.
        assert_eq!(rate_in_window(&ts, NOW, 60), 5.0);
    }

    #[test]
    fn windowed_rate_projects_count_to_hourly() {
        // 5 QSOs in the last 10 minutes → 30/hr.
        let ts = vec![NOW - MIN, NOW - 3 * MIN, NOW - 5 * MIN, NOW - 7 * MIN, NOW - 9 * MIN];
        assert_eq!(rate_in_window(&ts, NOW, 10), 30.0);
        // Same set in a 60-min window: 5 in 60 min → 5/hr.
        assert_eq!(rate_in_window(&ts, NOW, 60), 5.0);
    }

    #[test]
    fn last_n_qsos_none_when_fewer_than_n() {
        assert_eq!(rate_over_last_n_qsos(&[NOW - 5 * MIN], NOW, 5), None);
        let ts = vec![NOW - 10 * MIN, NOW - 5 * MIN, NOW - MIN];
        assert_eq!(rate_over_last_n_qsos(&ts, NOW, 5), None);
    }

    #[test]
    fn last_n_qsos_pace_includes_idle_gap() {
        // 5 QSOs in a burst 5 minutes long, ending 30 minutes ago. Anchor
        // (5th-from-end) is 35 min ago; 5 / 35min * 60 = 8.57/hr.
        let burst_end = NOW - 30 * MIN;
        let ts = vec![
            burst_end - 5 * MIN, // 5th-from-end (anchor)
            burst_end - 4 * MIN,
            burst_end - 3 * MIN,
            burst_end - MIN,
            burst_end,
        ];
        let r = rate_over_last_n_qsos(&ts, NOW, 5).unwrap();
        // Expected: 5 / 35 * 60 ≈ 8.571
        assert!((r - 8.571).abs() < 0.01, "got {r}");
    }

    #[test]
    fn last_n_qsos_fast_recent_pace() {
        // 5 QSOs in the last 5 min, most recent = now → anchor is 5 min ago,
        // rate = 5 / 5min * 60 = 60/hr.
        let ts = vec![NOW - 5 * MIN, NOW - 4 * MIN, NOW - 3 * MIN, NOW - 2 * MIN, NOW - MIN];
        let r = rate_over_last_n_qsos(&ts, NOW, 5).unwrap();
        // Anchor is 5 min ago, so 5 / 5 * 60 = 60.
        assert!((r - 60.0).abs() < 0.01, "got {r}");
    }

    /// User's reported scenario: last QSO 30 min ago. The wall-clock
    /// window-based rates should be zero, NOT 300+ as the previous bug
    /// produced. The last-5-QSOs metric reports the true (decaying) pace.
    #[test]
    fn user_scenario_last_qso_30min_ago_shows_zero_recent_rate() {
        let burst_end = NOW - 30 * MIN;
        let ts: Vec<u64> = (0..10)
            .map(|i| burst_end - i as u64 * MIN)
            .rev()
            .collect();
        assert_eq!(rate_in_window(&ts, NOW, 10), 0.0); // nothing recent
        // Last 60 min: 0 (burst was at -30..-39, all outside the 60-min window? -39 is inside)
        // Actually: -30, -31, -32, ..., -39 — all within 60 min. So count = 10, rate = 10.
        assert_eq!(rate_in_window(&ts, NOW, 60), 10.0);
        // Last 5 QSOs: anchor = 5th-from-end = -34 min, elapsed 34 min.
        // Rate = 5/34*60 ≈ 8.82
        let r5 = rate_over_last_n_qsos(&ts, NOW, 5).unwrap();
        assert!((r5 - 8.823).abs() < 0.01, "got {r5}");
        // Since last Q: 30 min.
        assert_eq!(secs_since_last(&ts, NOW), Some(30 * 60));
    }

    #[test]
    fn fmt_rate_rounds() {
        assert_eq!(fmt_rate(None), "—");
        assert_eq!(fmt_rate(Some(0.0)), "0/hr");
        assert_eq!(fmt_rate(Some(8.6)), "9/hr");
        assert_eq!(fmt_rate(Some(60.4)), "60/hr");
        assert_eq!(fmt_rate(Some(60.6)), "61/hr");
    }

    #[test]
    fn fmt_since_brackets() {
        assert_eq!(fmt_since(None), "—");
        assert_eq!(fmt_since(Some(0)), "0s");
        assert_eq!(fmt_since(Some(45)), "45s");
        assert_eq!(fmt_since(Some(90)), "1m30s");
        assert_eq!(fmt_since(Some(1800)), "30m00s");
        assert_eq!(fmt_since(Some(3660)), "1h01m");
    }
}
