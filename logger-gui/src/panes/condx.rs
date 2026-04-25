//! CondX pane — solar/propagation snapshot from hamqsl. Shows SFI, A/K
//! indices, X-ray flare level, and per-band short-term ratings.

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};
use logger_runtime::CondXSnapshot;

use super::style;

pub fn view<'a, M: 'a>(condx: Option<&'a CondXSnapshot>) -> Element<'a, M> {
    let Some(snap) = condx else {
        return container(
            container(
                text("(no snapshot yet — enable [condx] in config)")
                    .size(style::TEXT_BODY)
                    .style(style::very_muted),
            )
            .padding([8, 10])
            .style(style::card_style),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
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
        .size(style::TEXT_TINY)
        .style(style::very_muted);

    let mut body: Vec<Element<M>> = vec![
        row![
            stat_chip("SFI", sfi),
            stat_chip("Sunspots", sunspots),
            stat_chip("A / K", format!("{a_idx} / {k_idx}")),
        ]
        .spacing(8)
        .into(),
        row![
            stat_chip("X-ray", snap.xray.clone()),
            stat_chip("Geomag", snap.geomag_field.clone()),
            stat_chip("S/N", snap.signal_noise.clone()),
        ]
        .spacing(8)
        .into(),
    ];

    if !snap.conditions.is_empty() {
        let grouped = group_by_band(&snap.conditions);
        for (band, day, night) in grouped {
            body.push(band_condition_row(
                &band,
                day.unwrap_or_else(|| "—".to_string()),
                night.unwrap_or_else(|| "—".to_string()),
            ));
        }
    }

    body.push(updated_text.into());

    container(column(body).spacing(6))
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn stat_chip<'a, M: 'a>(label: &'static str, value: String) -> Element<'a, M> {
    container(
        column![
            text(label).size(style::TEXT_TINY).style(style::muted),
            text(value)
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(style::body),
        ]
        .spacing(1),
    )
    .padding([4, 8])
    .width(Length::FillPortion(1))
    .style(style::card_style)
    .into()
}

fn band_condition_row<'a, M: 'a>(band: &str, day: String, night: String) -> Element<'a, M> {
    container(
        row![
            text(band.to_string())
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(style::body)
                .width(Length::Fixed(56.0)),
            condition_text("Day", day),
            condition_text("Night", night),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([3, 4])
    .style(style::card_style)
    .into()
}

fn condition_text<'a, M: 'a>(label: &'static str, value: String) -> Element<'a, M> {
    let value_for_style = value.clone();
    row![
        text(label).size(style::TEXT_TINY).style(style::muted),
        text(value)
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(move |t: &Theme| condition_text_style(t, &value_for_style)),
    ]
    .spacing(4)
    .width(Length::FillPortion(1))
    .into()
}

fn condition_text_style(t: &Theme, value: &str) -> iced::widget::text::Style {
    let lower = value.to_ascii_lowercase();
    if lower.contains("excellent") {
        iced::widget::text::Style {
            color: Some(style::new_spot_color(t)),
        }
    } else if lower.contains("good") {
        iced::widget::text::Style {
            color: Some(style::accent_text_color(t)),
        }
    } else if lower.contains("fair") {
        iced::widget::text::Style {
            color: Some(style::muted_color(t)),
        }
    } else if lower.contains("poor") {
        iced::widget::text::Style {
            color: Some(style::very_muted_color(t)),
        }
    } else {
        style::body(t)
    }
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
