//! Rate pane — QSO cadence at a glance.
//!
//! Four metrics at different timescales, computed once per message in
//! `analytics::compute_rate_metrics` and read here as prepared values:
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

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};

use crate::analytics::RateMetrics;

use super::style;

pub fn view<'a, M: 'a>(metrics: &'a RateMetrics) -> Element<'a, M> {
    container(
        column![
            row![
                metric_card(
                    "10 min",
                    fmt_rate(Some(metrics.r10_per_hour)),
                    fmt_count(metrics.r10_count, "in 10m"),
                    true,
                ),
                metric_card(
                    "60 min",
                    fmt_rate(Some(metrics.r60_per_hour)),
                    fmt_count(metrics.r60_count, "in 60m"),
                    false,
                ),
            ]
            .spacing(8),
            row![
                metric_card(
                    "Last 5 Qs",
                    fmt_rate(metrics.last_5_per_hour),
                    "Recent run cadence".to_string(),
                    false,
                ),
                metric_card(
                    "Idle",
                    fmt_since(metrics.secs_since_last),
                    "Since the last QSO".to_string(),
                    false,
                ),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn metric_card<'a, M: 'a>(
    label: &'static str,
    value: String,
    caption: String,
    emphasize: bool,
) -> Element<'a, M> {
    container(
        column![
            text(label).size(style::TEXT_TINY).style(move |t: &Theme| {
                if emphasize {
                    style::accent(t)
                } else {
                    style::muted(t)
                }
            }),
            text(value)
                .size(style::TEXT_DISPLAY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| {
                    if emphasize {
                        iced::widget::text::Style {
                            color: Some(t.extended_palette().primary.weak.text),
                        }
                    } else {
                        style::body(t)
                    }
                }),
            text(caption)
                .size(style::TEXT_TINY)
                .style(move |t: &Theme| {
                    if emphasize {
                        iced::widget::text::Style {
                            color: Some(t.extended_palette().primary.base.color),
                        }
                    } else {
                        style::very_muted(t)
                    }
                }),
        ]
        .spacing(2),
    )
    .padding([8, 10])
    .width(Length::FillPortion(1))
    .height(Length::FillPortion(1))
    .style(move |t: &Theme| {
        if emphasize {
            style::accent_card_style(t)
        } else {
            style::card_style(t)
        }
    })
    .into()
}

fn fmt_count(count: u64, suffix: &'static str) -> String {
    match count {
        1 => format!("1 QSO {suffix}"),
        n => format!("{n} QSOs {suffix}"),
    }
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
