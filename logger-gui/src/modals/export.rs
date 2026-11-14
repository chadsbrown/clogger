//! Export modal: pick a format, choose a path, see the result. It is
//! built on the shared `widgets::modal_frame`.

use iced::widget::{button, column, row, text, text_input, Space};
use iced::{Border, Element, Length, Theme};

use crate::panes::style::{self, Icon};
use crate::panes::widgets::{self, ButtonVariant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Adif,
    Cabrillo,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Adif => "ADIF",
            Self::Cabrillo => "Cabrillo",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExportStep {
    SelectFormat,
    ChoosePath {
        format: ExportFormat,
        path: String,
    },
    Result {
        message: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone)]
pub struct ExportState {
    pub step: ExportStep,
}

impl Default for ExportState {
    fn default() -> Self {
        Self { step: ExportStep::SelectFormat }
    }
}

pub fn view<'a, M: 'a + Clone + 'static>(
    state: &'a ExportState,
    on_pick_adif: M,
    on_pick_cabrillo: M,
    on_path_changed: impl Fn(String) -> M + 'a + Clone,
    on_export: M,
    on_close: M,
) -> Element<'a, M> {
    let (body, footer): (Element<M>, Option<Element<M>>) = match &state.step {
        ExportStep::SelectFormat => (
            column![
                text("Choose a format")
                    .size(style::TEXT_BODY)
                    .style(style::muted),
                format_choice("ADIF", ".adi · for logbooks and LoTW", on_pick_adif),
                format_choice("Cabrillo", ".log · for contest submission", on_pick_cabrillo),
            ]
            .spacing(style::SP_3)
            .into(),
            None,
        ),
        ExportStep::ChoosePath { format, path } => {
            let can_export = !path.trim().is_empty();
            (
                column![
                    widgets::section_label(&format!("{} destination", format.label())),
                    text_input("Path", path)
                        .on_input(on_path_changed.clone())
                        .on_submit(on_export.clone())
                        .padding([style::SP_3 - 1.0, style::SP_4 - 2.0])
                        .size(style::TEXT_BODY)
                        .font(style::FONT_MONO)
                        .style(input_style),
                ]
                .spacing(style::SP_3)
                .into(),
                Some(
                    row![
                        Space::new().width(Length::Fill),
                        widgets::text_button("Cancel", ButtonVariant::Secondary, Some(on_close.clone())),
                        widgets::text_button(
                            "Export",
                            ButtonVariant::Primary,
                            can_export.then_some(on_export.clone()),
                        ),
                    ]
                    .spacing(style::SP_3)
                    .align_y(iced::alignment::Vertical::Center)
                    .into(),
                ),
            )
        }
        ExportStep::Result { message, is_error } => {
            let is_error = *is_error;
            (
                row![
                    style::icon(
                        if is_error { Icon::Alert } else { Icon::Check },
                        style::TEXT_VALUE,
                    )
                    .style(move |t: &Theme| text::Style {
                        color: Some(if is_error {
                            style::danger_text_color(t)
                        } else {
                            style::new_spot_color(t)
                        }),
                    }),
                    text(message.clone())
                        .size(style::TEXT_BODY)
                        .style(style::body)
                        .width(Length::Fill),
                ]
                .spacing(style::SP_3)
                .into(),
                Some(
                    row![
                        Space::new().width(Length::Fill),
                        widgets::text_button("Close", ButtonVariant::Secondary, Some(on_close.clone())),
                    ]
                    .into(),
                ),
            )
        }
    };

    widgets::modal_frame("Export log", body, footer, on_close, 460.0, None)
}

/// A selectable format row: name plus a short hint about what it's for,
/// with a hover state so it's clearly clickable. It replaced a plain
/// bordered box with no hover feedback.
fn format_choice<'a, M: 'a + Clone + 'static>(
    label: &'static str,
    hint: &'static str,
    on_press: M,
) -> Element<'a, M> {
    button(
        column![
            text(label)
                .size(style::TEXT_BODY)
                .font(style::FONT_UI_SEMIBOLD),
            text(hint).size(style::TEXT_TINY).style(style::muted),
        ]
        .spacing(style::SP_1),
    )
    .padding([style::SP_3, style::SP_4])
    .width(Length::Fill)
    .on_press(on_press)
    .style(|t: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(
                if hovered {
                    style::accent_tint(t)
                } else {
                    style::surface_raised(t)
                }
                .into(),
            ),
            text_color: style::text_color(t),
            border: Border {
                color: if hovered {
                    style::accent_color(t)
                } else {
                    style::border_color(t)
                },
                width: 1.0,
                radius: style::RADIUS_MD.into(),
            },
            ..button::Style::default()
        }
    })
    .into()
}

fn input_style(t: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: style::surface_sunken(t).into(),
        border: Border {
            color: if focused {
                style::accent_color(t)
            } else {
                style::border_color(t)
            },
            width: if focused { 1.5 } else { 1.0 },
            radius: style::RADIUS_SM.into(),
        },
        icon: style::muted_color(t),
        placeholder: style::very_muted_color(t),
        value: style::text_color(t),
        selection: style::with_alpha(style::accent_color(t), 0.35),
    }
}
