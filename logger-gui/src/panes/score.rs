//! Score pane — compact band score table plus totals.

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};
use logger_runtime::LogAdapter;

use super::style;

const COL_BAND_W: f32 = 64.0;
const COL_QSO_W: f32 = 48.0;
const COL_MULT_W: f32 = 48.0;
const ROW_GAP: f32 = 2.0;
const HEADER_GAP: f32 = 4.0;
const SECTION_GAP: f32 = 8.0;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let summary = log.score_summary();

    let mut rows: Vec<Element<M>> = Vec::new();
    for (band, bs) in &summary.by_band {
        rows.push(band_score_row(band, bs.qsos, bs.mults));
    }

    let body: Element<M> = if rows.is_empty() {
        text("(no QSOs logged)")
            .size(style::TEXT_BODY)
            .style(style::very_muted)
            .into()
    } else {
        column(rows).spacing(ROW_GAP).into()
    };

    container(
        column![
            header_row(),
            body,
            totals_row(summary.total_qsos, summary.total_mults),
            score_footer(summary.claimed_score),
        ]
        .spacing(SECTION_GAP),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn header_row<'a, M: 'a>() -> Element<'a, M> {
    row![
        text("BAND")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_BAND_W)),
        text("QSO")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_QSO_W)),
        text("MULT")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_MULT_W)),
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}

fn band_score_row<'a, M: 'a>(band: &str, qsos: u32, mults: u32) -> Element<'a, M> {
    row![
        text(band.to_string())
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(style::body)
            .width(Length::Fixed(COL_BAND_W)),
        text(qsos.to_string())
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(style::body)
            .width(Length::Fixed(COL_QSO_W)),
        text(mults.to_string())
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(move |t: &Theme| {
                if mults > 0 {
                    iced::widget::text::Style {
                        color: Some(style::mult_color(t)),
                    }
                } else {
                    style::body(t)
                }
            })
            .width(Length::Fixed(COL_MULT_W)),
    ]
    .spacing(8)
    .align_y(iced::alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}

fn totals_row<'a, M: 'a>(total_qsos: u32, total_mults: u32) -> Element<'a, M> {
    container(
        column![
            row![
                text("TOTAL")
                    .size(style::TEXT_TINY)
                    .font(Font::MONOSPACE)
                    .style(style::accent)
                    .width(Length::Fixed(COL_BAND_W)),
                text(total_qsos.to_string())
                    .size(style::TEXT_BODY)
                    .font(Font::MONOSPACE)
                    .style(style::body)
                    .width(Length::Fixed(COL_QSO_W)),
                text(total_mults.to_string())
                    .size(style::TEXT_BODY)
                    .font(Font::MONOSPACE)
                    .style(move |t: &Theme| {
                        if total_mults > 0 {
                            iced::widget::text::Style {
                                color: Some(style::mult_color(t)),
                            }
                        } else {
                            style::body(t)
                        }
                    })
                    .width(Length::Fixed(COL_MULT_W)),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .width(Length::Fill),
        ]
        .spacing(HEADER_GAP),
    )
    .padding(0)
    .width(Length::Fill)
    .into()
}

fn score_footer<'a, M: 'a>(claimed_score: i64) -> Element<'a, M> {
    row![
        text("SCORE")
            .size(style::TEXT_TINY)
            .font(Font::MONOSPACE)
            .style(style::muted)
            .width(Length::Fixed(COL_BAND_W)),
        text(claimed_score.to_string())
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(style::accent)
    ]
    .spacing(8)
    .align_y(iced::alignment::Vertical::Center)
    .width(Length::Fill)
    .into()
}
