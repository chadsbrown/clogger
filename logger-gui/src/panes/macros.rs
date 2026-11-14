//! Macros pane: the current F-key bindings. It is a read-only display;
//! editing will come later through a settings panel.
//!
//! Each row is a neutral keycap plus the macro text. Earlier versions
//! drew every key as an accent-filled badge inside a bordered card, so
//! twelve bright blue blocks were the loudest thing on screen for a pane
//! that is only reference material.

use iced::widget::{column, row, text};
use iced::{Element, Length, Theme};
use logger_core::Macros;

use super::style;
use super::widgets;

const KEY_W: f32 = 64.0;

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
    sections.push(group("Run keys", bare.iter().map(|(k, v)| macro_row(k, v)).collect()));

    if let Some(sp_f2) = &macros.sp_f2 {
        sections.push(group("S&P override", vec![macro_row("F2", sp_f2.as_str())]));
    }

    let secondary = [
        ("C-A-F1", macros.ctrl_alt_f1.as_str()),
        ("C-A-F2", macros.ctrl_alt_f2.as_str()),
        ("C-A-F3", macros.ctrl_alt_f3.as_str()),
        ("C-A-F4", macros.ctrl_alt_f4.as_str()),
        ("C-A-F5", macros.ctrl_alt_f5.as_str()),
    ];
    let extra_rows: Vec<Element<M>> = secondary
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| macro_row(k, v))
        .collect();
    if !extra_rows.is_empty() {
        sections.push(group("Ctrl+Alt macros", extra_rows));
    }

    widgets::pane_body(column(sections).spacing(style::SP_4))
}

fn group<'a, M: 'a>(title: &str, rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    column![widgets::section_label(title), column(rows).spacing(style::SP_2)]
        .spacing(style::SP_2)
        .into()
}

fn macro_row<'a, M: 'a>(label: &str, value: &str) -> Element<'a, M> {
    let empty = value.is_empty();
    let value_str = if empty { "—".to_string() } else { value.to_string() };
    row![
        widgets::keycap(label, Some(KEY_W)),
        text(value_str)
            .size(style::TEXT_BODY)
            .font(style::FONT_MONO)
            .style(move |t: &Theme| if empty { style::very_muted(t) } else { style::body(t) })
            .width(Length::Fill),
    ]
    .spacing(style::SP_3)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
