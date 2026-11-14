//! Help modal: keybinding reference.
//!
//! Bindings are grouped by task, and keys are drawn as keycaps so a
//! chord like Ctrl-Alt-↑ scans as keys rather than prose. The list is
//! maintained by hand against `keys.rs` and `main.rs::keyboard_subscription`.
//! When you add a binding there, add it here too. A 2026-11 review found
//! this list had drifted: it still described the old ☀/☾ theme toggle
//! and was missing `=`, `` ` ``, Space and ←/→.

use iced::widget::{column, row, scrollable, text};
use iced::{Element, Length};

use crate::panes::style;
use crate::panes::widgets;

/// A key sequence. Each `&[&str]` is one chord rendered as adjacent
/// keycaps, and alternatives are separated by "/".
type Keys = &'static [&'static [&'static str]];

const SECTIONS: &[(&str, &[(Keys, &str)])] = &[
    (
        "Entry",
        &[
            (&[&["Enter"]], "ESM: send the next macro and log per contest flow"),
            (&[&["Alt", "Enter"]], "QuickLog: log the entry without sending CW"),
            (&[&["Tab"]], "Next field"),
            (&[&["Space"]], "Next field, skipping the auto-filled RST"),
            (&[&["←"], &["→"]], "Move the cursor within the field"),
            (&[&["Backspace"]], "Delete the character before the cursor"),
            (&[&["="]], "Cycle SCP matches into the call field"),
            (&[&["Insert"]], "Toggle Run / S&P"),
            (&[&["Esc"]], "Abort CW; also closes a dialog"),
        ],
    ),
    (
        "Radios & bandmap",
        &[
            (&[&["↑"], &["↓"]], "Focus radio 1 / radio 2"),
            (&[&["Ctrl", "↑"], &["Ctrl", "↓"]], "Bandmap: next spot (radio 1)"),
            (
                &[&["Ctrl", "Alt", "↑"], &["Ctrl", "Alt", "↓"]],
                "Bandmap: next spot (radio 2)",
            ),
            (&[&["PgUp"], &["PgDn"]], "CW speed ±2 WPM on the focused radio"),
            (&[&["`"]], "Toggle SO2R RX mono / stereo"),
            (&[&["Click"]], "Tune to the clicked frequency or spot"),
            (&[&["Scroll"]], "Zoom the bandmap"),
        ],
    ),
    (
        "Macros",
        &[
            (&[&["F1–F12"]], "Send a macro"),
            (&[&["Ctrl", "Alt", "F1–F12"]], "Send a secondary macro"),
        ],
    ),
    (
        "Window",
        &[
            (&[&["Ctrl", "+"], &["Ctrl", "−"]], "Increase / decrease UI scale"),
            (&[&["Ctrl", "0"]], "Reset UI scale to 100%"),
            (&[&["Drag title"]], "Move a pane"),
            (&[&["Drag edge"]], "Resize a pane"),
            (&[&["Toolbar"]], "Show or hide a pane"),
            (&[&["Status bar"]], "Font size, export, theme picker, help"),
        ],
    ),
];

const KEYS_COL_W: f32 = 200.0;

pub fn view<'a, M: 'a + Clone + 'static>(on_close: M) -> Element<'a, M> {
    let mut groups: Vec<Element<M>> = Vec::with_capacity(SECTIONS.len());
    for (title, bindings) in SECTIONS {
        let mut rows: Vec<Element<M>> = Vec::with_capacity(bindings.len());
        for (keys, desc) in *bindings {
            rows.push(
                row![
                    row(keys_view(keys))
                        .spacing(style::SP_2)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Length::Fixed(KEYS_COL_W)),
                    text(desc.to_string())
                        .size(style::TEXT_BODY)
                        .style(style::body)
                        .width(Length::Fill),
                ]
                .spacing(style::SP_4)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            );
        }
        groups.push(
            column![
                widgets::section_label(title),
                column(rows).spacing(style::SP_3 - 2.0)
            ]
            .spacing(style::SP_3)
            .into(),
        );
    }

    let body = scrollable(column(groups).spacing(style::SP_5 + 2.0).padding(iced::Padding {
        right: style::SP_4,
        ..iced::Padding::ZERO
    }))
    .width(Length::Fill)
    .height(Length::Fill)
    .into();

    let footer = text("Esc or click outside to close")
        .size(style::TEXT_TINY)
        .style(style::very_muted)
        .into();

    widgets::modal_frame("Keyboard shortcuts", body, Some(footer), on_close, 620.0, Some(560.0))
}

fn keys_view<'a, M: 'a>(keys: Keys) -> Vec<Element<'a, M>> {
    let mut out: Vec<Element<M>> = Vec::new();
    for (i, chord) in keys.iter().enumerate() {
        if i > 0 {
            out.push(text("/").size(style::TEXT_LABEL).style(style::very_muted).into());
        }
        for (j, k) in chord.iter().enumerate() {
            if j > 0 {
                out.push(text("+").size(style::TEXT_TINY).style(style::very_muted).into());
            }
            out.push(widgets::keycap(*k, None));
        }
    }
    out
}
