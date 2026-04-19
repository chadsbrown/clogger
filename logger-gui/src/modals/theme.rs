//! Theme-picker modal — scrollable grid of all iced built-in themes.
//! Clicking a tile selects it; the currently active theme is outlined.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Border, Color, Element, Length, Theme};

use crate::panes::style;

pub fn view<'a, M: 'a + Clone + 'static>(
    current: &Theme,
    on_select: fn(Theme) -> M,
    on_close: M,
) -> Element<'a, M> {
    let current_name = current.to_string();
    let themes = Theme::ALL;

    let mut tiles: Vec<Element<M>> = Vec::with_capacity(themes.len());
    for t in themes {
        tiles.push(tile(t, t.to_string() == current_name, on_select).into());
    }

    let close = button(text("✕").size(14))
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
        text("Choose a theme").size(style::TEXT_HEADER),
        Space::with_width(Length::Fill),
        close,
    ]
    .align_y(iced::alignment::Vertical::Center);

    let body: Element<M> = container(
        column![
            header,
            Space::with_height(10),
            scrollable(
                column(split_into_rows(tiles, 3))
                    .spacing(6)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill),
            Space::with_height(8),
            text(format!("Current: {current_name}"))
                .size(style::TEXT_LABEL)
                .style(style::muted),
        ]
        .spacing(0),
    )
    .padding(16)
    .width(Length::Fixed(640.0))
    .height(Length::Fixed(520.0))
    .style(|t: &Theme| container::Style {
        background: Some(t.extended_palette().background.base.color.into()),
        border: Border {
            color: style::focused_border_color(t),
            width: 1.5,
            radius: style::RADIUS_FRAME.into(),
        },
        ..container::Style::default()
    })
    .into();

    container(body)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn tile<'a, M: 'a + Clone + 'static>(
    theme: &'a Theme,
    is_current: bool,
    on_select: fn(Theme) -> M,
) -> iced::widget::Button<'a, M> {
    // Each tile shows the theme name against a preview of the theme's own
    // base background color, so the operator can actually see the palette
    // change before selecting.
    let theme_for_preview = theme.clone();
    let theme_for_press = theme.clone();
    let name = theme.to_string();
    let inner = container(
        column![
            text(name).size(style::TEXT_BODY),
            Space::with_height(6),
            row![
                swatch(theme.extended_palette().primary.strong.color),
                swatch(theme.extended_palette().success.base.color),
                swatch(theme.extended_palette().danger.base.color),
                swatch(theme.extended_palette().background.strong.color),
            ]
            .spacing(4),
        ]
        .spacing(0),
    )
    .padding(10)
    .width(Length::Fill)
    .style(move |_t: &Theme| {
        let pal = theme_for_preview.extended_palette();
        container::Style {
            background: Some(pal.background.base.color.into()),
            text_color: Some(pal.background.base.text),
            ..container::Style::default()
        }
    });

    button(inner)
        .on_press(on_select(theme_for_press))
        .padding(0)
        .width(Length::Fill)
        .style(move |t: &Theme, status| {
            let pal = t.extended_palette();
            let border_color = if is_current {
                pal.primary.strong.color
            } else {
                match status {
                    button::Status::Hovered => pal.primary.weak.color,
                    _ => style::border_color(t),
                }
            };
            button::Style {
                background: None,
                text_color: pal.background.base.text,
                border: Border {
                    color: border_color,
                    width: if is_current { 2.0 } else { 1.0 },
                    radius: style::RADIUS_FRAME.into(),
                },
                ..button::Style::default()
            }
        })
}

fn swatch<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new(Length::Fill, Length::Fill))
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(move |_t: &Theme| container::Style {
            background: Some(color.into()),
            border: Border {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                width: 1.0,
                radius: style::RADIUS_CHIP.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn split_into_rows<'a, M: 'a>(
    items: Vec<Element<'a, M>>,
    per_row: usize,
) -> Vec<Element<'a, M>> {
    let mut out: Vec<Element<M>> = Vec::new();
    let mut current: Vec<Element<M>> = Vec::with_capacity(per_row);
    for item in items {
        current.push(item);
        if current.len() == per_row {
            out.push(row(std::mem::take(&mut current)).spacing(6).into());
        }
    }
    if !current.is_empty() {
        // Pad the final partial row so tiles keep a consistent width.
        while current.len() < per_row {
            current.push(Space::with_width(Length::Fill).into());
        }
        out.push(row(current).spacing(6).into());
    }
    out
}
