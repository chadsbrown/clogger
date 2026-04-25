//! Macros pane — current F-key bindings. Read-only display; editing comes
//! later via a settings panel.

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};
use logger_core::Macros;

use super::style;

pub fn view<'a, M: 'a>(macros: &'a Macros) -> Element<'a, M> {
    let bare = [
        ("F1", macros.f1.as_str()),
        ("F2", macros.f2.as_str()),
        ("F3", macros.f3.as_str()),
        ("F5", macros.f5.as_str()),
        ("F7", macros.f7.as_str()),
        ("F8", macros.f8.as_str()),
        ("F9", macros.f9.as_str()),
    ];

    let mut sections: Vec<Element<M>> = Vec::new();
    sections.push(section_header("Run keys"));
    let mut run_rows: Vec<Element<M>> = Vec::with_capacity(bare.len());
    for (label, value) in bare {
        run_rows.push(macro_row(label, value));
    }
    sections.push(column(run_rows).spacing(4).into());

    if let Some(sp_f2) = &macros.sp_f2 {
        sections.push(section_header("S&P override"));
        sections.push(macro_row("S&P F2", sp_f2.as_str()));
    }

    let secondary = [
        ("C-A-F1", macros.ctrl_alt_f1.as_str()),
        ("C-A-F2", macros.ctrl_alt_f2.as_str()),
        ("C-A-F3", macros.ctrl_alt_f3.as_str()),
        ("C-A-F4", macros.ctrl_alt_f4.as_str()),
        ("C-A-F5", macros.ctrl_alt_f5.as_str()),
    ];
    let any_secondary = secondary.iter().any(|(_, v)| !v.is_empty());
    if any_secondary {
        let mut extra_rows: Vec<Element<M>> = Vec::new();
        for (label, value) in secondary {
            if !value.is_empty() {
                extra_rows.push(macro_row(label, value));
            }
        }
        sections.push(section_header("Ctrl+Alt macros"));
        sections.push(column(extra_rows).spacing(4).into());
    }

    container(column(sections).spacing(8))
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn macro_row<'a, M: 'a>(label: &str, value: &str) -> Element<'a, M> {
    let empty = value.is_empty();
    let value_str = if empty { "—".to_string() } else { value.to_string() };
    container(
        row![
            keycap(label),
            text(value_str)
                .size(style::TEXT_BODY)
                .font(Font::MONOSPACE)
                .style(move |t: &Theme| {
                    if empty {
                        style::very_muted(t)
                    } else {
                        style::body(t)
                    }
                })
                .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([6, 8])
    .style(style::card_style)
    .into()
}

fn section_header<'a, M: 'a>(label: &'static str) -> Element<'a, M> {
    text(label)
        .size(style::TEXT_TINY)
        .style(style::muted)
        .into()
}

fn keycap<'a, M: 'a>(label: &str) -> Element<'a, M> {
    container(
        text(label.to_string())
            .size(style::TEXT_BODY)
            .font(Font::MONOSPACE)
            .style(move |t: &Theme| iced::widget::text::Style {
                color: Some(t.extended_palette().primary.base.text),
            }),
    )
    .padding([4, 8])
    .width(Length::Fixed(84.0))
    .style(move |t: &Theme| style::badge_style(t, t.extended_palette().primary.base))
    .into()
}
