//! Rate pane — last 10 min, last 100 min, projected hourly, time since
//! last QSO. Computed on demand from `LogAdapter`.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_runtime::{compute_rate, LogAdapter};

use super::style;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let info = compute_rate(log, now_ms);

    let line = |label: &'static str, value: String| -> Element<'a, M> {
        row![
            text(label)
                .size(12)
                .style(style::muted)
                .width(Length::Fixed(110.0)),
            text(value).size(14).style(style::body),
        ]
        .spacing(4)
        .into()
    };

    let last_10 = info
        .last_10_minutes
        .map(|r| format!("{:.1}/hr", r))
        .unwrap_or_else(|| "—".to_string());
    let last_100 = info
        .last_100_minutes
        .map(|r| format!("{:.1}/hr", r))
        .unwrap_or_else(|| "—".to_string());
    let since = info
        .secs_since_last_qso
        .map(|s| format_hms(s))
        .unwrap_or_else(|| "—".to_string());

    container(
        column![
            line("Last 10 min", last_10),
            Space::with_height(2),
            line("Last 100 min", last_100),
            Space::with_height(2),
            line("Projected/hr", format!("{}", info.rate_per_hour)),
            Space::with_height(2),
            line("Since last Q", since),
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn format_hms(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}
