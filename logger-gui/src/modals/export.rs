//! Export modal — generate ADIF files via a native Save File dialog.
//! Mirrors the TUI flow: SelectFormat → (OS dialog) → Result. Cabrillo
//! is shown but disabled, matching the TUI.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Element, Length, Theme};

use crate::panes::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Adif,
    Cabrillo,
}

#[derive(Debug, Clone)]
pub enum ExportStep {
    SelectFormat,
    /// File dialog is open or the export is being written. The modal is
    /// inert in this state; the OS dialog drives. The chosen format
    /// rides on `Message::ExportPathChosen` rather than this state.
    Working,
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
    on_close: M,
) -> Element<'a, M> {
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
        text("Export log").size(style::TEXT_HEADER),
        Space::new().width(Length::Fill),
        close,
    ]
    .align_y(iced::alignment::Vertical::Center);

    let inner: Element<M> = match &state.step {
        ExportStep::SelectFormat => column![
            text("Select export format:").size(style::TEXT_BODY),
            Space::new().height(10),
            format_choice("ADIF (.adi)", on_pick_adif),
            Space::new().height(6),
            format_choice("Cabrillo (.log)", on_pick_cabrillo),
        ]
        .spacing(0)
        .into(),
        ExportStep::Working => column![
            text("Choose destination…").size(style::TEXT_BODY),
            Space::new().height(8),
            text("File dialog open. Cancel the dialog to abort.")
                .size(style::TEXT_LABEL)
                .style(style::muted),
        ]
        .spacing(0)
        .into(),
        ExportStep::Result { message, is_error } => {
            let result_color = move |t: &Theme| -> iced::widget::text::Style {
                iced::widget::text::Style {
                    color: Some(if *is_error {
                        style::danger_color(t)
                    } else {
                        t.extended_palette().success.base.color
                    }),
                }
            };
            column![
                text(message.clone())
                    .size(style::TEXT_BODY)
                    .style(result_color),
                Space::new().height(12),
                button(text("Close").size(style::TEXT_BODY))
                    .padding([4, 12])
                    .on_press(on_close.clone()),
            ]
            .spacing(0)
            .into()
        }
    };

    let body: Element<M> = container(
        column![header, Space::new().height(10), inner].spacing(0),
    )
    .padding(16)
    .width(Length::Fixed(440.0))
    .height(Length::Fixed(220.0))
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

fn format_choice<'a, M: 'a + Clone + 'static>(label: &'static str, on_press: M) -> Element<'a, M> {
    button(
        row![
            text(label).size(style::TEXT_BODY),
        ]
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .on_press(on_press)
    .style(|t: &Theme, status| {
        let pal = t.extended_palette();
        let bg = match status {
            button::Status::Hovered => pal.primary.weak.color,
            _ => pal.background.weak.color,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: pal.background.base.text,
            border: Border {
                color: style::border_color(t),
                width: 1.0,
                radius: style::RADIUS_INPUT.into(),
            },
            ..button::Style::default()
        }
    })
    .into()
}
