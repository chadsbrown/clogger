//! Available pane: per-band counts of unworked spotted callsigns and
//! still-needed multipliers.
//!
//! The headline totals are shown as stats, followed by a flat table. The
//! band(s) that lead in calls or mults get an accent bar, which answers
//! "where should I switch to?" at a glance. Earlier versions also added
//! "Q"/"M" leader badges and an accent border, which said the same thing
//! three times.

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Theme};
use logger_runtime::AvailSummary;

use super::style;
use super::widgets::{self, Tone};

const COL_BAND_W: f32 = 48.0;
const COL_CALLS_W: f32 = 64.0;
const COL_SPACING: f32 = style::SP_3;

pub fn view<'a, M: 'a>(info: &'a AvailSummary) -> Element<'a, M> {
    let max_qsos = info.by_band.iter().map(|(_, qsos, _)| *qsos).max().unwrap_or(0);
    let max_mults = info.by_band.iter().map(|(_, _, mults)| *mults).max().unwrap_or(0);

    let mut rows: Vec<Element<M>> = Vec::new();
    for (band, qsos, mults) in &info.by_band {
        if *qsos == 0 && *mults == 0 {
            continue;
        }
        let leader =
            (*qsos == max_qsos && max_qsos > 0) || (*mults == max_mults && max_mults > 0);
        let stripe = rows.len() % 2 == 1;
        rows.push(widgets::table_row(
            available_cells(band, *qsos, *mults, leader),
            stripe,
            leader.then_some(Tone::Accent),
        ));
    }

    let table: Element<M> = if rows.is_empty() {
        widgets::empty_state("No unworked spots")
    } else {
        column![
            widgets::table_header(
                vec![
                    widgets::col_header("Band", Length::Fixed(COL_BAND_W)),
                    widgets::col_header("Calls", Length::Fixed(COL_CALLS_W)),
                    widgets::col_header("Mults", Length::Fill),
                ],
                COL_SPACING,
            ),
            column(rows),
        ]
        .into()
    };

    widgets::pane_body(
        column![
            row![
                container(widgets::stat(
                    "Calls",
                    info.total_qsos.to_string(),
                    style::TEXT_HEADER + 4.0,
                    None,
                ))
                .width(Length::FillPortion(1)),
                container(widgets::stat(
                    "Mults",
                    info.total_mults.to_string(),
                    style::TEXT_HEADER + 4.0,
                    (info.total_mults > 0).then_some(Tone::Warning),
                ))
                .width(Length::FillPortion(1)),
            ]
            .spacing(style::SP_3),
            table,
        ]
        .spacing(style::SP_4),
    )
}

fn available_cells<'a, M: 'a>(band: &str, qsos: u32, mults: u32, leader: bool) -> Element<'a, M> {
    let mult_cell: Element<M> = if mults > 0 {
        widgets::badge(
            if mults == 1 {
                "1 mult".to_string()
            } else {
                format!("{mults} mults")
            },
            Tone::Warning,
        )
    } else {
        text("—")
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO)
            .style(style::very_muted)
            .into()
    };
    row![
        text(band.to_string())
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO_BOLD)
            .style(move |t: &Theme| if leader { style::accent(t) } else { style::body(t) })
            .width(Length::Fixed(COL_BAND_W)),
        text(qsos.to_string())
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO)
            .style(style::body)
            .width(Length::Fixed(COL_CALLS_W)),
        mult_cell,
    ]
    .spacing(COL_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
