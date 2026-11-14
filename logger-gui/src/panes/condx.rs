//! CondX pane: solar/propagation snapshot from hamqsl. Shows SFI, A/K
//! indices, X-ray flare level, and per-band short-term ratings.
//!
//! The six solar stats share one raised tile in a 3×2 grid, and band
//! conditions are a flat Band / Day / Night table. Ratings use a
//! traffic-light scale (good is success, fair is warning, poor is
//! danger) instead of the earlier mix of green, accent blue and two
//! shades of gray, which made "poor" look like "no data".

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Theme};
use logger_runtime::CondXSnapshot;

use super::style;
use super::widgets::{self, Tone};

const COL_BAND_W: f32 = 72.0;
const COL_SPACING: f32 = style::SP_3;

pub fn view<'a, M: 'a>(condx: Option<&'a CondXSnapshot>) -> Element<'a, M> {
    let Some(snap) = condx else {
        return widgets::pane_body(widgets::empty_state(
            "No snapshot yet. Enable [condx] in the config.",
        ));
    };

    let opt = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string());

    let stats = widgets::tile(
        column![
            row![
                stat_cell("SFI", opt(snap.sfi)),
                stat_cell("Sunspots", opt(snap.sunspots)),
                stat_cell("A / K", format!("{} / {}", opt(snap.a_index), opt(snap.k_index))),
            ]
            .spacing(style::SP_3),
            row![
                stat_cell("X-ray", snap.xray.clone()),
                stat_cell("Geomag", snap.geomag_field.clone()),
                stat_cell("S/N", snap.signal_noise.clone()),
            ]
            .spacing(style::SP_3),
        ]
        .spacing(style::SP_3),
        None,
        false,
    );

    let mut body: Vec<Element<M>> = vec![stats.into()];

    if !snap.conditions.is_empty() {
        let mut rows: Vec<Element<M>> = Vec::new();
        for (i, (band, day, night)) in group_by_band(&snap.conditions).into_iter().enumerate() {
            rows.push(widgets::table_row(
                row![
                    text(band)
                        .size(style::TEXT_BODY)
                        .font(style::FONT_MONO)
                        .style(style::body)
                        .width(Length::Fixed(COL_BAND_W)),
                    rating_text(day.unwrap_or_else(|| "—".to_string())),
                    rating_text(night.unwrap_or_else(|| "—".to_string())),
                ]
                .spacing(COL_SPACING)
                .align_y(iced::alignment::Vertical::Center),
                i % 2 == 1,
                None,
            ));
        }
        body.push(
            column![
                widgets::table_header(
                    vec![
                        widgets::col_header("Band", Length::Fixed(COL_BAND_W)),
                        widgets::col_header("Day", Length::FillPortion(1)),
                        widgets::col_header("Night", Length::FillPortion(1)),
                    ],
                    COL_SPACING,
                ),
                column(rows),
            ]
            .into(),
        );
    }

    // hamqsl emits timestamps as "<time>Z <date> GMT"; rewrite to UTC
    // since that's the standard amateur-radio convention.
    body.push(
        text(format!("Updated {}", snap.updated.replace("GMT", "UTC")))
            .size(style::TEXT_TINY)
            .style(style::very_muted)
            .into(),
    );

    widgets::pane_body(column(body).spacing(style::SP_4))
}

fn stat_cell<'a, M: 'a>(label: &str, value: String) -> Element<'a, M> {
    container(widgets::stat(label, value, style::TEXT_BODY, None))
        .width(Length::FillPortion(1))
        .into()
}

fn rating_text<'a, M: 'a>(value: String) -> Element<'a, M> {
    let tone = rating_tone(&value);
    text(value)
        .size(style::TEXT_BODY)
        .font(style::FONT_UI_MEDIUM)
        .style(move |t: &Theme| text::Style {
            color: Some(match tone {
                Some(tone) => widgets::tone_text(t, tone),
                None => style::muted_color(t),
            }),
        })
        .width(Length::FillPortion(1))
        .into()
}

fn rating_tone(value: &str) -> Option<Tone> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("excellent") || lower.contains("good") {
        Some(Tone::Success)
    } else if lower.contains("fair") {
        Some(Tone::Warning)
    } else if lower.contains("poor") {
        Some(Tone::Danger)
    } else {
        None
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
