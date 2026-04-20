//! CondX pane — solar/propagation snapshot from hamqsl. Shows SFI, A/K
//! indices, X-ray flare level, and per-band short-term ratings.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_runtime::CondXSnapshot;

use super::style;

pub fn view<'a, M: 'a>(condx: Option<&'a CondXSnapshot>) -> Element<'a, M> {
    let Some(snap) = condx else {
        return container(
            text("(no snapshot yet — enable [condx] in config)")
                .size(12.0)
                .style(style::very_muted),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    };

    let line = |label: &'static str, value: String| -> Element<'a, M> {
        row![
            text(label)
                .size(12.0)
                .style(style::muted)
                .width(Length::Fixed(96.0)),
            text(value).size(13.0).style(style::body),
        ]
        .spacing(4)
        .into()
    };

    let sfi = snap
        .sfi
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".to_string());
    let a_idx = snap
        .a_index
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".to_string());
    let k_idx = snap
        .k_index
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".to_string());
    let sunspots = snap
        .sunspots
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".to_string());

    // hamqsl emits timestamps as "<time>Z <date> GMT"; rewrite to UTC
    // since that's the standard amateur-radio convention.
    let updated_text = text(format!("updated {}", snap.updated.replace("GMT", "UTC")))
        .size(10.0)
        .style(style::very_muted);

    let mut body: Vec<Element<M>> = vec![
        line("SFI", sfi),
        line("Sunspots", sunspots),
        line("A / K", format!("{a_idx} / {k_idx}")),
        line("X-ray", snap.xray.clone()),
        line("Geomag", snap.geomag_field.clone()),
        line("S/N", snap.signal_noise.clone()),
    ];

    if !snap.conditions.is_empty() {
        body.push(Space::new().height(6).into());
        // Header row: band / day / night. Day and night ratings share one
        // line per band — hamqsl emits them as two separate `BandCondition`
        // entries tagged `"day"` / `"night"` for the same band label, so
        // group them before rendering.
        body.push(
            row![
                text("Band")
                    .size(11.0)
                    .style(style::muted)
                    .width(Length::Fixed(80.0)),
                text("Day")
                    .size(11.0)
                    .style(style::muted)
                    .width(Length::Fill),
                text("Night")
                    .size(11.0)
                    .style(style::muted)
                    .width(Length::Fill),
            ]
            .spacing(4)
            .into(),
        );

        let grouped = group_by_band(&snap.conditions);
        for (band, day, night) in grouped {
            body.push(
                row![
                    text(band)
                        .size(12.0)
                        .style(style::body)
                        .width(Length::Fixed(80.0)),
                    text(day.unwrap_or_else(|| "—".to_string()))
                        .size(12.0)
                        .style(style::body)
                        .width(Length::Fill),
                    text(night.unwrap_or_else(|| "—".to_string()))
                        .size(12.0)
                        .style(style::body)
                        .width(Length::Fill),
                ]
                .spacing(4)
                .into(),
            );
        }
    }

    body.push(Space::new().height(6).into());
    body.push(updated_text.into());

    container(column(body).spacing(0))
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Collapse the flat `[BandCondition]` list from hamqsl (which repeats
/// each band label twice — once for `time="day"`, once for `time="night"`)
/// into one `(band, day_rating, night_rating)` per band, preserving the
/// first-seen band order so the display matches the feed.
fn group_by_band(conds: &[logger_runtime::BandCondition]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut out: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for c in conds {
        let is_day = c.time.eq_ignore_ascii_case("day");
        if let Some(entry) = out.iter_mut().find(|(b, _, _)| b == &c.band) {
            if is_day {
                entry.1 = Some(c.rating.clone());
            } else {
                entry.2 = Some(c.rating.clone());
            }
        } else if is_day {
            out.push((c.band.clone(), Some(c.rating.clone()), None));
        } else {
            out.push((c.band.clone(), None, Some(c.rating.clone())));
        }
    }
    out
}
