//! Score pane: claimed score as a headline number above a compact
//! per-band table.
//!
//! The claimed score is what an operator glances at, so it is drawn at
//! display size at the top. Earlier versions put it at body size at the
//! bottom. The band table is flat and striped, with a totals row set off
//! by a hairline.

use iced::widget::{column, row, text};
use iced::{Element, Length, Theme};
use logger_runtime::LogAdapter;

use super::style;
use super::widgets::{self, Tone};

const COL_BAND_W: f32 = 64.0;
const COL_QSO_W: f32 = 56.0;
const COL_MULT_W: f32 = 56.0;
const COL_SPACING: f32 = style::SP_3;

pub fn view<'a, M: 'a>(log: &'a LogAdapter) -> Element<'a, M> {
    let summary = log.score_summary();

    let mut rows: Vec<Element<M>> = Vec::new();
    for (i, (band, bs)) in summary.by_band.iter().enumerate() {
        rows.push(widgets::table_row(
            score_cells(band.to_string(), bs.qsos, bs.mults, false),
            i % 2 == 1,
            None,
        ));
    }

    let table: Element<M> = if rows.is_empty() {
        widgets::empty_state("No QSOs logged")
    } else {
        column![
            widgets::table_header(
                vec![
                    widgets::col_header("Band", Length::Fixed(COL_BAND_W)),
                    widgets::col_header("QSOs", Length::Fixed(COL_QSO_W)),
                    widgets::col_header("Mults", Length::Fixed(COL_MULT_W)),
                ],
                COL_SPACING,
            ),
            column(rows),
            widgets::hairline(),
            widgets::table_row(
                score_cells(
                    "TOTAL".to_string(),
                    summary.total_qsos,
                    summary.total_mults,
                    true
                ),
                false,
                None,
            ),
        ]
        .into()
    };

    widgets::pane_body(
        column![
            widgets::stat(
                "Claimed score",
                format_thousands(summary.claimed_score),
                style::TEXT_DISPLAY,
                Some(Tone::Accent),
            ),
            table,
        ]
        .spacing(style::SP_4),
    )
}

fn score_cells<'a, M: 'a>(band: String, qsos: u32, mults: u32, total: bool) -> Element<'a, M> {
    let font = if total {
        style::FONT_MONO_BOLD
    } else {
        style::FONT_MONO
    };
    row![
        text(band)
            .size(if total { style::TEXT_TINY } else { style::TEXT_BODY })
            .font(if total { style::FONT_UI_SEMIBOLD } else { style::FONT_MONO })
            .style(move |t: &Theme| if total { style::muted(t) } else { style::body(t) })
            .width(Length::Fixed(COL_BAND_W)),
        text(qsos.to_string())
            .size(style::TEXT_BODY)
            .font(font)
            .style(style::body)
            .width(Length::Fixed(COL_QSO_W)),
        text(mults.to_string())
            .size(style::TEXT_BODY)
            .font(font)
            .style(move |t: &Theme| if mults > 0 { style::mult(t) } else { style::body(t) })
            .width(Length::Fixed(COL_MULT_W)),
    ]
    .spacing(COL_SPACING)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// 1234567 → "1,234,567". Big contest scores are hard to read at a
/// glance without separators.
fn format_thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_thousands;

    #[test]
    fn thousands() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1000), "1,000");
        assert_eq!(format_thousands(1234567), "1,234,567");
        assert_eq!(format_thousands(-4200), "-4,200");
    }
}
