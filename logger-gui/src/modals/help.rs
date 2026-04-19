//! Help modal — keybinding reference. First modal; establishes the pattern
//! (backdrop + centered content + close affordances).

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Border, Color, Element, Length, Theme};

const BINDINGS: &[(&str, &str)] = &[
    ("Enter", "ESM (send macro + log per contest flow)"),
    ("Alt-Enter", "QuickLog — log current entry without sending CW"),
    ("Esc", "Close this modal; also clears entry / resets state"),
    ("Tab", "Next entry field"),
    ("Backspace", "Delete previous character in focused field"),
    ("↑ / ↓", "Focus radio 1 / radio 2"),
    ("Ctrl-↑ / Ctrl-↓", "Bandmap navigate (radio 1)"),
    ("Ctrl-Alt-↑ / ↓", "Bandmap navigate (radio 2)"),
    ("Insert", "Toggle Run / S&P mode"),
    ("PageUp / PageDown", "±2 WPM CW speed on focused radio"),
    ("F1 – F9", "Send macro"),
    ("Ctrl-Alt-F1 – F12", "Secondary macro"),
    ("Ctrl-+ / Ctrl--", "Increase / decrease global UI scale"),
    ("Ctrl-0", "Reset UI scale to 100%"),
    ("Click spot", "Tune focused radio to that spot's frequency"),
    ("Drag title bar", "Move pane"),
    ("Drag ◢ corner", "Resize pane"),
    ("✕ on pane", "Hide pane (reopen via toolbar)"),
    ("Toolbar button", "Toggle pane visibility"),
    ("☀ / ☾ in status bar", "Toggle light/dark theme"),
];

pub fn view<'a, M: 'a + Clone + 'static>(on_close: M) -> Element<'a, M> {
    let mut rows: Vec<Element<M>> = Vec::with_capacity(BINDINGS.len());
    for (keys, desc) in BINDINGS {
        rows.push(
            row![
                text(keys.to_string())
                    .size(13.0)
                    .color(Color::from_rgb(0.55, 0.7, 0.95))
                    .width(Length::Fixed(170.0)),
                text(desc.to_string()).size(13.0),
            ]
            .spacing(10)
            .into(),
        );
    }

    let close = button(text("✕").size(14.0))
        .padding([0, 8])
        .on_press(on_close.clone())
        .style(|t: &Theme, status| {
            let pal = t.extended_palette().background.strong;
            let bg = match status {
                button::Status::Hovered => t.extended_palette().danger.base.color,
                _ => pal.color,
            };
            let fg = match status {
                button::Status::Hovered => t.extended_palette().danger.base.text,
                _ => pal.text,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: fg,
                border: Border::default(),
                ..button::Style::default()
            }
        });

    let header = row![
        text("Keybindings").size(15.0),
        Space::new().width(Length::Fill),
        close,
    ]
    .align_y(iced::alignment::Vertical::Center);

    let body: Element<M> = container(
        column![
            header,
            Space::new().height(8),
            scrollable(column(rows).spacing(4))
                .width(Length::Fill)
                .height(Length::Fill),
            Space::new().height(6),
            text("Esc or click outside to close").size(11.0).color(
                Color::from_rgb(0.5, 0.5, 0.5),
            ),
        ]
        .spacing(0),
    )
    .padding(16)
    .width(Length::Fixed(560.0))
    .height(Length::Fixed(480.0))
    .style(|t: &Theme| {
        let pal = t.extended_palette().background.base;
        container::Style {
            background: Some(pal.color.into()),
            border: Border {
                color: Color::from_rgb(0.45, 0.55, 0.85),
                width: 1.5,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        }
    })
    .into();

    // Center it in the parent stack layer.
    container(body)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
