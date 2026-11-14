//! Performance pane: process and host resource usage for the running GUI.

use iced::widget::{Space, column, container, progress_bar, row, text};
use iced::{Border, Element, Length, Theme};

use crate::perf::PerfSnapshot;

use super::style;
use super::widgets::{self, Tone};

pub fn view<'a, M: 'a>(snapshot: &'a PerfSnapshot) -> Element<'a, M> {
    let error: Element<M> = snapshot
        .error
        .as_ref()
        .map(|message| {
            text(message.clone())
                .size(style::TEXT_TINY)
                .style(style::danger)
                .into()
        })
        .unwrap_or_else(|| Space::new().height(0).into());

    widgets::pane_body(
        column![
            row![
                metric_tile(
                    "Proc CPU",
                    fmt_percent(snapshot.process_cpu_percent),
                    "Clogger process".to_string(),
                    snapshot.process_cpu_percent,
                    true,
                ),
                metric_tile(
                    "RSS",
                    fmt_bytes(snapshot.rss_bytes),
                    format!("virt {}", fmt_bytes(snapshot.virtual_bytes)),
                    None,
                    false,
                ),
            ]
            .spacing(style::SP_3),
            row![
                metric_tile(
                    "Host CPU",
                    fmt_percent(snapshot.system_cpu_percent),
                    "Total system".to_string(),
                    snapshot.system_cpu_percent,
                    false,
                ),
                metric_tile(
                    "Host mem",
                    fmt_percent(snapshot.system_memory_used_percent),
                    fmt_memory_pair(
                        snapshot.system_memory_used_bytes,
                        snapshot.system_memory_total_bytes,
                    ),
                    snapshot.system_memory_used_percent,
                    false,
                ),
            ]
            .spacing(style::SP_3),
            details_row(snapshot),
            error,
        ]
        .spacing(style::SP_3),
    )
}

fn metric_tile<'a, M: 'a>(
    label: &'static str,
    value: String,
    caption: String,
    bar_percent: Option<f32>,
    emphasize: bool,
) -> Element<'a, M> {
    let bar: Element<M> = match bar_percent {
        Some(percent) => progress_bar(0.0..=100.0, percent.clamp(0.0, 100.0))
            .girth(4.0)
            .style(move |t: &Theme| progress_bar::Style {
                background: style::surface_sunken(t).into(),
                bar: if emphasize {
                    style::accent_color(t)
                } else {
                    style::muted_color(t)
                }
                .into(),
                border: Border {
                    radius: 2.0.into(),
                    ..Border::default()
                },
            })
            .into(),
        None => Space::new().height(4).into(),
    };

    widgets::tile(
        column![
            widgets::stat(label, value, style::TEXT_HEADER, None),
            bar,
            text(caption).size(style::TEXT_TINY).style(style::very_muted),
        ]
        .spacing(style::SP_2),
        emphasize.then_some(Tone::Accent),
        false,
    )
    .width(Length::FillPortion(1))
    .into()
}

fn details_row<'a, M: 'a>(snapshot: &PerfSnapshot) -> Element<'a, M> {
    row![
        detail("Threads", fmt_u64(snapshot.thread_count)),
        detail("FD", fmt_u64(snapshot.fd_count)),
        detail("Up", fmt_duration(snapshot.uptime)),
        detail("Samples", snapshot.sample_count.to_string()),
    ]
    .spacing(style::SP_3)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn detail<'a, M: 'a>(label: &'static str, value: String) -> Element<'a, M> {
    container(widgets::stat(label, value, style::TEXT_BODY, None))
        .width(Length::FillPortion(1))
        .into()
}

fn fmt_percent(value: Option<f32>) -> String {
    match value {
        Some(value) if value < 10.0 => format!("{value:.1}%"),
        Some(value) => format!("{value:.0}%"),
        None => "-".to_string(),
    }
}

fn fmt_bytes(value: Option<u64>) -> String {
    let Some(bytes) = value else {
        return "-".to_string();
    };
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1}G", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.0}M", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.0}K", bytes / KIB)
    } else {
        format!("{bytes:.0}B")
    }
}

fn fmt_memory_pair(used: Option<u64>, total: Option<u64>) -> String {
    match (used, total) {
        (Some(used), Some(total)) => {
            format!("{} / {}", fmt_bytes(Some(used)), fmt_bytes(Some(total)))
        }
        _ => "used / total".to_string(),
    }
}

fn fmt_u64(value: Option<u64>) -> String {
    value
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn fmt_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_formatting() {
        assert_eq!(fmt_percent(None), "-");
        assert_eq!(fmt_percent(Some(1.25)), "1.2%");
        assert_eq!(fmt_percent(Some(10.4)), "10%");
    }

    #[test]
    fn byte_formatting() {
        assert_eq!(fmt_bytes(None), "-");
        assert_eq!(fmt_bytes(Some(512)), "512B");
        assert_eq!(fmt_bytes(Some(1024 * 512)), "512K");
        assert_eq!(fmt_bytes(Some(1024 * 1024 * 42)), "42M");
    }
}
