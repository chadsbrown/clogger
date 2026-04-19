//! Macros pane — current F-key bindings. Read-only display; editing comes
//! later via a settings panel.

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length};
use logger_core::Macros;

use super::style;

pub fn view<'a, M: 'a>(macros: &'a Macros) -> Element<'a, M> {
    let _perf = crate::perf::Span::new("pane.macros"); // PROFILING
    let bare = [
        ("F1", macros.f1.as_str()),
        ("F2", macros.f2.as_str()),
        ("F3", macros.f3.as_str()),
        ("F5", macros.f5.as_str()),
        ("F7", macros.f7.as_str()),
        ("F8", macros.f8.as_str()),
        ("F9", macros.f9.as_str()),
    ];

    let mut rows: Vec<Element<M>> = Vec::with_capacity(bare.len() + 4);
    for (label, value) in bare {
        rows.push(macro_row(label, value));
    }

    if let Some(sp_f2) = &macros.sp_f2 {
        rows.push(Space::new().height(4).into());
        rows.push(text("S&P overrides").size(11.0).style(style::muted).into());
        rows.push(macro_row("S&P F2", sp_f2.as_str()));
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
        rows.push(Space::new().height(4).into());
        rows.push(text("Ctrl+Alt+Fn").size(11.0).style(style::muted).into());
        for (label, value) in secondary {
            if !value.is_empty() {
                rows.push(macro_row(label, value));
            }
        }
    }

    container(column(rows).spacing(2))
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn macro_row<'a, M: 'a>(label: &str, value: &str) -> Element<'a, M> {
    let empty = value.is_empty();
    let value_str = if empty { "—".to_string() } else { value.to_string() };
    row![
        text(label.to_string())
            .size(12.0)
            .style(style::accent)
            .width(Length::Fixed(70.0)),
        text(value_str).size(12.0).style(move |t: &iced::Theme| {
            if empty {
                style::very_muted(t)
            } else {
                style::body(t)
            }
        }),
    ]
    .spacing(4)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}
