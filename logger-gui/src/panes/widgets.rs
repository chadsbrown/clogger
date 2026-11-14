//! Shared building blocks for panes, chrome and modals.
//!
//! Before this module existed, each pane hand-rolled its own badge, table
//! header, empty state and close button. The copies drifted apart, ending
//! up with 8 badge variants, 4 paddings and 3 radii. Everything that looks
//! the same should now come from one function here, so a later visual
//! change is made in one place. See `docs/gui-visual-design.md`.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Color, Element, Length, Padding, Shadow, Theme, Vector};

use super::style::{self, Icon};

// ---------------------------------------------------------------------
// Tones
// ---------------------------------------------------------------------

/// The semantic color roles available to badges, bars and buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

/// Base color for a tone, used for fills, bars and dots.
pub fn tone_color(t: &Theme, tone: Tone) -> Color {
    match tone {
        Tone::Neutral => style::very_muted_color(t),
        Tone::Accent => style::accent_color(t),
        Tone::Success => style::success_color(t),
        Tone::Warning => style::warning_color(t),
        Tone::Danger => style::danger_color(t),
    }
}

/// A tone's color, adjusted to read as text on the pane surface.
pub fn tone_text(t: &Theme, tone: Tone) -> Color {
    match tone {
        Tone::Neutral => style::muted_color(t),
        _ => style::ensure_contrast(tone_color(t, tone), style::surface_pane(t), 4.5),
    }
}

/// A tone's palette pair (fill plus the palette's matching readable
/// text), used for solid badges.
fn tone_pair(t: &Theme, tone: Tone) -> iced::theme::palette::Pair {
    let p = t.extended_palette();
    match tone {
        Tone::Neutral => p.background.strong,
        Tone::Accent => p.primary.base,
        Tone::Success => p.success.base,
        Tone::Warning => p.warning.base,
        Tone::Danger => p.danger.base,
    }
}

// ---------------------------------------------------------------------
// Badges, dots, keycaps
// ---------------------------------------------------------------------

/// A soft badge: tinted background with tone-colored text. This is the
/// default for state labels (R1, RUN, RX LEFT, …) that should be visible
/// without shouting.
pub fn badge<'a, M: 'a>(label: impl Into<String>, tone: Tone) -> Element<'a, M> {
    container(
        text(label.into())
            .size(style::TEXT_TINY)
            .font(style::FONT_UI_SEMIBOLD)
            .style(move |t: &Theme| text::Style {
                color: Some(tone_text(t, tone)),
            }),
    )
    .padding(style::PAD_BADGE)
    .style(move |t: &Theme| {
        let fill = match tone {
            Tone::Neutral => style::lerp(style::surface_pane(t), style::text_color(t), 0.09),
            _ => style::lerp(style::surface_pane(t), tone_color(t, tone), 0.18),
        };
        container::Style {
            background: Some(fill.into()),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: style::RADIUS_SM.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}

/// A solid badge: full tone fill. Reserved for alerts the operator must
/// not miss (DUPE, MULT, TX, VOID).
pub fn badge_solid<'a, M: 'a>(label: impl Into<String>, tone: Tone) -> Element<'a, M> {
    container(
        text(label.into())
            .size(style::TEXT_TINY)
            .font(style::FONT_UI_SEMIBOLD)
            .style(move |t: &Theme| text::Style {
                color: Some(tone_pair(t, tone).text),
            }),
    )
    .padding(style::PAD_BADGE)
    .style(move |t: &Theme| container::Style {
        background: Some(tone_pair(t, tone).color.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: style::RADIUS_SM.into(),
        },
        ..container::Style::default()
    })
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotState {
    /// Not configured, or no data yet.
    Idle,
    Ok,
    Error,
}

/// Connection indicator: a small colored dot plus a label. It replaces
/// the solid-fill pills that made the status bar look like a row of
/// traffic lights. The label turns danger-colored on error so a failure
/// is still easy to spot.
pub fn status_dot<'a, M: 'a>(label: impl Into<String>, state: DotState) -> Element<'a, M> {
    let dot = container(Space::new().width(7).height(7))
        .width(7)
        .height(7)
        .style(move |t: &Theme| container::Style {
            background: Some(
                match state {
                    DotState::Idle => style::very_muted_color(t),
                    DotState::Ok => style::success_color(t),
                    DotState::Error => style::danger_color(t),
                }
                .into(),
            ),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 3.5.into(),
            },
            ..container::Style::default()
        });
    row![
        dot,
        text(label.into())
            .size(style::TEXT_TINY)
            .font(style::FONT_UI_MEDIUM)
            .style(move |t: &Theme| text::Style {
                color: Some(match state {
                    DotState::Idle => style::very_muted_color(t),
                    DotState::Ok => style::muted_color(t),
                    DotState::Error => style::danger_text_color(t),
                }),
            }),
    ]
    .spacing(style::SP_2 + 1.0)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// A physical-key look-alike for F-keys and shortcuts. It is neutral,
/// not accent-filled, so a column of 12 macro keys doesn't dominate the
/// screen.
pub fn keycap<'a, M: 'a>(label: impl Into<String>, width: Option<f32>) -> Element<'a, M> {
    let c = container(
        text(label.into())
            .size(style::TEXT_LABEL)
            .font(style::FONT_MONO)
            .style(style::body),
    )
    .padding(Padding {
        top: 1.0,
        right: 6.0,
        bottom: 2.0,
        left: 6.0,
    })
    .style(|t: &Theme| container::Style {
        background: Some(style::surface_raised(t).into()),
        border: Border {
            color: style::border_color(t),
            width: 1.0,
            radius: style::RADIUS_SM.into(),
        },
        // A hard 1px bottom shadow suggests the key's depth.
        shadow: Shadow {
            color: style::border_color(t),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 0.0,
        },
        ..container::Style::default()
    });
    match width {
        Some(w) => c.width(Length::Fixed(w)).into(),
        None => c.into(),
    }
}

// ---------------------------------------------------------------------
// Labels, tables, stats, empty states
// ---------------------------------------------------------------------

/// Small uppercase heading for a group within a pane, or for a field
/// label. It uses semibold weight rather than color to separate itself
/// from data.
pub fn section_label<'a, M: 'a>(label: &str) -> Element<'a, M> {
    text(label.to_uppercase())
        .size(style::TEXT_TINY)
        .font(style::FONT_UI_SEMIBOLD)
        .style(style::muted)
        .into()
}

/// A table column header cell.
pub fn col_header<'a, M: 'a>(label: &str, width: Length) -> Element<'a, M> {
    text(label.to_uppercase())
        .size(style::TEXT_TINY)
        .font(style::FONT_UI_SEMIBOLD)
        .style(style::muted)
        .width(width)
        .into()
}

/// The header row of a table. It uses the same horizontal padding as
/// `table_row` (plus the accent-bar gutter) so columns line up, and has
/// a hairline underneath.
pub fn table_header<'a, M: 'a>(cells: Vec<Element<'a, M>>, spacing: f32) -> Element<'a, M> {
    column![
        container(row(cells).spacing(spacing).width(Length::Fill)).padding(Padding {
            top: 0.0,
            right: style::PAD_ROW.right,
            bottom: style::SP_2,
            left: style::PAD_ROW.left + ACCENT_BAR_W,
        }),
        hairline(),
    ]
    .into()
}

/// Width of the left accent bar on highlighted rows and tiles.
pub const ACCENT_BAR_W: f32 = 2.0;

/// A flat table row. Rows are separated by an alternating stripe instead
/// of a border per row; with up to 512 log rows, bordered cards were
/// visually overwhelming. `highlight` draws a 2px tone-colored bar on the
/// left plus a faint tint. Every row reserves the bar's width, so
/// highlighted and plain rows stay aligned.
pub fn table_row<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    stripe: bool,
    highlight: Option<Tone>,
) -> Element<'a, M> {
    let row_bg = move |t: &Theme| -> Color {
        match highlight {
            Some(tone) => style::lerp(style::surface_pane(t), tone_color(t, tone), 0.10),
            None if stripe => style::row_stripe(t),
            None => style::surface_pane(t),
        }
    };
    let inner = container(content)
        .padding(style::PAD_ROW)
        .width(Length::Fill)
        .style(move |t: &Theme| container::Style {
            background: Some(row_bg(t).into()),
            ..container::Style::default()
        });
    container(inner)
        .padding(Padding {
            left: ACCENT_BAR_W,
            ..Padding::ZERO
        })
        .width(Length::Fill)
        .style(move |t: &Theme| container::Style {
            background: Some(
                match highlight {
                    Some(tone) => tone_color(t, tone),
                    None => row_bg(t),
                }
                .into(),
            ),
            ..container::Style::default()
        })
        .into()
}

/// A raised tile, optionally with a left accent bar. Used for metric
/// cards (rate, perf) and SO2R radio cards. `accent` is how one tile is
/// singled out, instead of filling the whole tile with primary color.
///
/// `fill_height` makes the tile stretch to its slot, as in the rate
/// pane's 2×2 grid. Otherwise it shrinks to fit its content.
pub fn tile<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    accent: Option<Tone>,
    fill_height: bool,
) -> iced::widget::Container<'a, M> {
    let inner = container(content)
        .padding(style::PAD_TILE)
        .width(Length::Fill)
        .height(if fill_height { Length::Fill } else { Length::Shrink })
        .style(style::tile_style);
    // The outer container's fill shows through its left padding as the
    // accent bar. This works at any tile height, which a separate
    // fixed-size bar widget would not.
    container(inner)
        .padding(Padding {
            left: if accent.is_some() { 3.0 } else { 0.0 },
            ..Padding::ZERO
        })
        .style(move |t: &Theme| match accent {
            Some(tone) => container::Style {
                background: Some(tone_color(t, tone).into()),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: style::RADIUS_MD.into(),
                },
                ..container::Style::default()
            },
            None => container::Style::default(),
        })
}

/// A label-over-value stat block. Values are monospace so changing
/// numbers don't jitter.
pub fn stat<'a, M: 'a>(
    label: &str,
    value: impl Into<String>,
    size: f32,
    tone: Option<Tone>,
) -> Element<'a, M> {
    column![
        section_label(label),
        text(value.into())
            .size(size)
            .font(style::FONT_MONO)
            .style(move |t: &Theme| text::Style {
                color: Some(match tone {
                    Some(tone) => tone_text(t, tone),
                    None => style::text_color(t),
                }),
            }),
    ]
    .spacing(style::SP_1)
    .into()
}

/// One consistent empty state: a muted line of text, with no card
/// around it and no parentheses.
pub fn empty_state<'a, M: 'a>(msg: &str) -> Element<'a, M> {
    container(
        text(msg.to_string())
            .size(style::TEXT_BODY)
            .style(style::very_muted),
    )
    .padding([style::SP_3, style::SP_2])
    .into()
}

/// A 1px horizontal divider in the border color.
pub fn hairline<'a, M: 'a>() -> Element<'a, M> {
    container(Space::new().height(1))
        .width(Length::Fill)
        .height(1)
        .style(|t: &Theme| container::Style {
            background: Some(style::border_color(t).into()),
            ..container::Style::default()
        })
        .into()
}

/// The standard pane body wrapper: fills the pane with `PAD_PANE` inset.
pub fn pane_body<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .padding(style::PAD_PANE)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The main action in a dialog, such as Export.
    Primary,
    /// A secondary dialog action, such as Cancel.
    Secondary,
    /// A chrome button with no fill at rest, such as toolbar or
    /// status-bar icons.
    Ghost,
    /// A ghost button that turns red on hover, such as close buttons.
    GhostDanger,
}

pub fn button_style(
    variant: ButtonVariant,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |t: &Theme, status: button::Status| {
        let pal = t.extended_palette();
        let hover = matches!(status, button::Status::Hovered);
        let pressed = matches!(status, button::Status::Pressed);
        let disabled = matches!(status, button::Status::Disabled);
        let (bg, fg, border) = match variant {
            ButtonVariant::Primary => {
                let p = if hover || pressed {
                    pal.primary.strong
                } else {
                    pal.primary.base
                };
                (Some(p.color), p.text, Color::TRANSPARENT)
            }
            ButtonVariant::Secondary => (
                Some(if hover || pressed {
                    style::lerp(style::surface_pane(t), style::text_color(t), 0.10)
                } else {
                    style::surface_raised(t)
                }),
                style::text_color(t),
                style::border_color(t),
            ),
            ButtonVariant::Ghost => (
                (hover || pressed)
                    .then(|| style::lerp(style::surface_pane(t), style::text_color(t), 0.09)),
                if hover {
                    style::text_color(t)
                } else {
                    style::muted_color(t)
                },
                Color::TRANSPARENT,
            ),
            ButtonVariant::GhostDanger => (
                (hover || pressed).then_some(pal.danger.base.color),
                if hover || pressed {
                    pal.danger.base.text
                } else {
                    style::muted_color(t)
                },
                Color::TRANSPARENT,
            ),
        };
        let fg = if disabled {
            style::very_muted_color(t)
        } else {
            fg
        };
        let bg = if disabled && variant == ButtonVariant::Primary {
            Some(style::surface_raised(t))
        } else {
            bg
        };
        button::Style {
            background: bg.map(Into::into),
            text_color: fg,
            border: Border {
                color: border,
                width: if border == Color::TRANSPARENT { 0.0 } else { 1.0 },
                radius: style::RADIUS_SM.into(),
            },
            ..button::Style::default()
        }
    }
}

/// A text button in one of the standard variants.
pub fn text_button<'a, M: 'a + Clone>(
    label: &str,
    variant: ButtonVariant,
    on_press: Option<M>,
) -> Element<'a, M> {
    button(
        text(label.to_string())
            .size(style::TEXT_BODY)
            .font(style::FONT_UI_MEDIUM),
    )
    .padding(style::PAD_BUTTON)
    .on_press_maybe(on_press)
    .style(button_style(variant))
    .into()
}

/// A square icon button (status bar, pane close, modal close).
pub fn icon_button<'a, M: 'a + Clone>(
    i: Icon,
    size: f32,
    variant: ButtonVariant,
    on_press: M,
) -> iced::widget::Button<'a, M> {
    button(
        container(style::icon(i, size))
            .center_x(Length::Shrink)
            .center_y(Length::Shrink),
    )
    .padding(style::SP_2)
    .on_press(on_press)
    .style(button_style(variant))
}

// ---------------------------------------------------------------------
// Modal frame
// ---------------------------------------------------------------------

/// The shared modal shell: a header with the title and close button, a
/// hairline, the body, an optional footer row (right-aligned actions),
/// large radius and raised shadow. All three modals use this, where
/// previously each had its own copy of the frame and close button.
pub fn modal_frame<'a, M: 'a + Clone>(
    title: &str,
    body: Element<'a, M>,
    footer: Option<Element<'a, M>>,
    on_close: M,
    width: f32,
    height: Option<f32>,
) -> Element<'a, M> {
    let header = row![
        text(title.to_string())
            .size(style::TEXT_HEADER)
            .font(style::FONT_UI_SEMIBOLD)
            .style(style::body),
        Space::new().width(Length::Fill),
        icon_button(Icon::X, 14.0, ButtonVariant::GhostDanger, on_close),
    ]
    .align_y(iced::alignment::Vertical::Center);

    let mut col = column![
        container(header).padding(Padding {
            top: style::SP_4,
            right: style::SP_4,
            bottom: style::SP_3,
            left: style::SP_5 + style::SP_1,
        }),
        hairline(),
        container(body)
            .padding([style::SP_4, style::SP_5 + style::SP_1])
            .width(Length::Fill)
            .height(if height.is_some() {
                Length::Fill
            } else {
                Length::Shrink
            }),
    ];
    if let Some(footer) = footer {
        col = col.push(hairline()).push(
            container(footer)
                .padding([style::SP_4, style::SP_5 + style::SP_1])
                .width(Length::Fill),
        );
    }

    let frame = container(col)
        .width(Length::Fixed(width))
        .height(height.map(Length::Fixed).unwrap_or(Length::Shrink))
        .style(|t: &Theme| container::Style {
            background: Some(style::surface_pane(t).into()),
            text_color: Some(style::text_color(t)),
            border: Border {
                color: style::border_color(t),
                width: 1.0,
                radius: style::RADIUS_LG.into(),
            },
            shadow: style::shadow_focus(t),
            ..container::Style::default()
        });

    container(frame)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
