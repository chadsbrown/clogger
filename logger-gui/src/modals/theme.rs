//! Theme-picker modal: a scrollable grid of all iced built-in themes.
//! Clicking a tile selects it; the active theme is outlined and marked
//! with a check.
//!
//! Each tile is a small preview drawn in that theme's own colors, with a
//! pane surface, a line of text and accent/success/warning/danger
//! swatches, so the operator can judge it before choosing.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Border, Color, Element, Length, Theme};

use crate::panes::style::{self, Icon};
use crate::panes::widgets;

const PER_ROW: usize = 3;

pub fn view<'a, M: 'a + Clone + 'static>(
    current: &Theme,
    on_select: fn(Theme) -> M,
    on_close: M,
) -> Element<'a, M> {
    let current_name = current.to_string();

    let tiles: Vec<Element<M>> = Theme::ALL
        .iter()
        .map(|t| tile(t, t.to_string() == current_name, on_select))
        .collect();

    let body: Element<M> = scrollable(
        column(split_into_rows(tiles, PER_ROW))
            .spacing(style::SP_3)
            .width(Length::Fill)
            .padding(iced::Padding {
                right: style::SP_4,
                ..iced::Padding::ZERO
            }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into();

    let footer: Element<M> = row![
        text("Current").size(style::TEXT_TINY).style(style::very_muted),
        text(current_name)
            .size(style::TEXT_LABEL)
            .font(style::FONT_UI_MEDIUM)
            .style(style::body),
    ]
    .spacing(style::SP_3)
    .align_y(iced::alignment::Vertical::Center)
    .into();

    widgets::modal_frame("Theme", body, Some(footer), on_close, 680.0, Some(560.0))
}

fn tile<'a, M: 'a + Clone + 'static>(
    theme: &'a Theme,
    is_current: bool,
    on_select: fn(Theme) -> M,
) -> Element<'a, M> {
    let preview_theme = theme.clone();
    let pal = theme.extended_palette();
    let name_color = pal.background.base.text;

    let name_row = row![
        text(theme.to_string())
            .size(style::TEXT_BODY)
            .font(style::FONT_UI_MEDIUM)
            .color(name_color)
            .width(Length::Fill),
        if is_current {
            Element::from(style::icon(Icon::Check, style::TEXT_BODY).color(pal.primary.base.color))
        } else {
            Space::new().width(0).into()
        },
    ]
    .align_y(iced::alignment::Vertical::Center);

    let inner = container(
        column![
            name_row,
            row![
                swatch(pal.primary.base.color),
                swatch(pal.success.base.color),
                swatch(pal.warning.base.color),
                swatch(pal.danger.base.color),
                Space::new().width(Length::Fill),
                // A miniature "text line" in the theme's muted tone.
                container(Space::new().width(28).height(4))
                    .style(move |_t: &Theme| container::Style {
                        background: Some(style::muted_color(&preview_theme).into()),
                        border: Border {
                            radius: 2.0.into(),
                            ..Border::default()
                        },
                        ..container::Style::default()
                    }),
            ]
            .spacing(style::SP_2)
            .align_y(iced::alignment::Vertical::Center),
        ]
        .spacing(style::SP_3),
    )
    .padding([style::SP_3 + 2.0, style::SP_4])
    .width(Length::Fill)
    .style({
        let bg = pal.background.base.color;
        move |_t: &Theme| container::Style {
            background: Some(bg.into()),
            border: Border {
                radius: (style::RADIUS_MD - 1.0).into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    });

    let theme_for_press = theme.clone();
    button(inner)
        .on_press(on_select(theme_for_press))
        .padding(1)
        .width(Length::Fill)
        .style(move |t: &Theme, status| {
            let border_color = if is_current {
                style::accent_color(t)
            } else if matches!(status, button::Status::Hovered) {
                style::muted_color(t)
            } else {
                style::border_color(t)
            };
            button::Style {
                background: Some(border_color.into()),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: style::RADIUS_MD.into(),
                },
                ..button::Style::default()
            }
        })
        .into()
}

fn swatch<'a, M: 'a>(color: Color) -> Element<'a, M> {
    container(Space::new().width(14).height(14))
        .width(14)
        .height(14)
        .style(move |_t: &Theme| container::Style {
            background: Some(color.into()),
            border: Border {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
                width: 1.0,
                radius: 7.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn split_into_rows<'a, M: 'a>(items: Vec<Element<'a, M>>, per_row: usize) -> Vec<Element<'a, M>> {
    let mut out: Vec<Element<M>> = Vec::new();
    let mut current: Vec<Element<M>> = Vec::with_capacity(per_row);
    for item in items {
        current.push(item);
        if current.len() == per_row {
            out.push(row(std::mem::take(&mut current)).spacing(style::SP_3).into());
        }
    }
    if !current.is_empty() {
        // Pad the final partial row so tiles keep a consistent width.
        while current.len() < per_row {
            current.push(Space::new().width(Length::Fill).into());
        }
        out.push(row(current).spacing(style::SP_3).into());
    }
    out
}
