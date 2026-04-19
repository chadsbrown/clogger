//! CondX pane — solar/propagation snapshot from hamqsl. Shows SFI, A/K
//! indices, X-ray flare level, and per-band short-term ratings.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_runtime::CondXSnapshot;

use super::style;

pub fn view<'a, M: 'a>(condx: Option<&'a CondXSnapshot>) -> Element<'a, M> {
    let Some(snap) = condx else {
        return container(
            column![
                text("CondX").size(11.0).style(style::accent),
                Space::new().height(6),
                text("(no snapshot yet — enable [condx] in config)")
                    .size(12.0)
                    .style(style::very_muted),
            ]
            .spacing(0),
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

    let updated_text = text(format!("updated {}", snap.updated))
        .size(10.0)
        .style(style::very_muted);

    let mut body: Vec<Element<M>> = vec![
        text("CondX").size(11.0).style(style::accent).into(),
        Space::new().height(4).into(),
        line("SFI", sfi),
        line("Sunspots", sunspots),
        line("A / K", format!("{a_idx} / {k_idx}")),
        line("X-ray", snap.xray.clone()),
        line("Geomag", snap.geomag_field.clone()),
        line("S/N", snap.signal_noise.clone()),
    ];

    if !snap.conditions.is_empty() {
        body.push(Space::new().height(6).into());
        body.push(
            text("Band ratings (time — rating)")
                .size(11.0)
                .style(style::muted)
                .into(),
        );
        for band_cond in &snap.conditions {
            body.push(
                row![
                    text(band_cond.band.clone())
                        .size(12.0)
                        .style(style::body)
                        .width(Length::Fixed(80.0)),
                    text(band_cond.time.clone())
                        .size(12.0)
                        .style(style::muted)
                        .width(Length::Fixed(56.0)),
                    text(band_cond.rating.clone())
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
