//! SO2R routing-state pane — FOCUS (operator's entry focus), RX (OTRSP
//! audio routing), TX (which radio the keyer last sent to).

use iced::widget::{column, container, row, text};
use iced::{Element, Font, Length, Theme};
use logger_core::{RadioId, So2rRxMode};

use super::style;
use crate::bridge::AdapterHandles;

pub fn view<'a, M: 'a>(
    focused_radio: RadioId,
    tx_radio: RadioId,
    handles: &'a AdapterHandles,
) -> Element<'a, M> {
    let rx_mode = handles.so2r_default_rx_mode;
    container(
        column![
            row![
                radio_card(1, focused_radio, tx_radio, rx_mode),
                radio_card(2, focused_radio, tx_radio, rx_mode),
            ]
            .spacing(8),
            text("Press ` to toggle RX mono/stereo")
                .size(style::TEXT_TINY)
                .style(style::very_muted),
        ]
        .spacing(6),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn radio_card<'a, M: 'a>(
    radio: RadioId,
    focused_radio: RadioId,
    tx_radio: RadioId,
    rx_mode: So2rRxMode,
) -> Element<'a, M> {
    let focus = focused_radio == radio;
    let tx = tx_radio == radio;
    let rx = rx_role(radio, focused_radio, rx_mode);
    let audible = is_audible(radio, focused_radio, rx_mode);

    let mut badges: Vec<Element<M>> = vec![role_badge(rx).into()];
    if focus {
        badges.push(state_badge("FOCUS", |t| style::badge_style(t, t.extended_palette().primary.base)).into());
    }
    if tx {
        badges.push(state_badge("TX", |t| style::badge_style(t, style::dupe_pair(t))).into());
    }

    container(
        column![
            row![
                text(format!("R{radio}"))
                    .size(style::TEXT_DISPLAY)
                    .font(Font::MONOSPACE)
                    .style(move |t: &Theme| {
                        if audible {
                            style::accent(t)
                        } else {
                            style::body(t)
                        }
                    }),
                text(if audible { "audio on" } else { "muted" })
                    .size(style::TEXT_TINY)
                    .style(style::very_muted),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center),
            row(badges).spacing(4).wrap(),
            text(rx_detail(radio, focused_radio, rx_mode))
                .size(style::TEXT_TINY)
                .style(style::muted),
        ]
        .spacing(6),
    )
    .padding([10, 12])
    .width(Length::FillPortion(1))
    .height(Length::Fill)
    .style(move |t: &Theme| {
        if audible {
            style::accent_card_style(t)
        } else {
            style::card_style(t)
        }
    })
    .into()
}

fn is_audible(radio: RadioId, focused_radio: RadioId, rx_mode: So2rRxMode) -> bool {
    match rx_mode {
        So2rRxMode::Mono => radio == focused_radio,
        So2rRxMode::Stereo | So2rRxMode::ReverseStereo => true,
    }
}

fn state_badge<'a, M: 'a>(
    label: &'static str,
    style_fn: fn(&Theme) -> iced::widget::container::Style,
) -> iced::widget::Container<'a, M> {
    container(text(label).size(style::TEXT_TINY))
        .padding([2, 6])
        .style(style_fn)
}

fn role_badge<'a, M: 'a>(label: &'static str) -> iced::widget::Container<'a, M> {
    container(text(label).size(style::TEXT_TINY))
        .padding([2, 6])
        .style(style::muted_badge_style)
}

fn rx_role(radio: RadioId, focused_radio: RadioId, rx_mode: So2rRxMode) -> &'static str {
    match rx_mode {
        So2rRxMode::Mono => {
            if radio == focused_radio {
                "RX MONO"
            } else {
                "MUTED"
            }
        }
        So2rRxMode::Stereo => {
            if radio == 1 {
                "RX LEFT"
            } else {
                "RX RIGHT"
            }
        }
        So2rRxMode::ReverseStereo => {
            if radio == 1 {
                "RX RIGHT"
            } else {
                "RX LEFT"
            }
        }
    }
}

fn rx_detail(radio: RadioId, focused_radio: RadioId, rx_mode: So2rRxMode) -> &'static str {
    match rx_mode {
        So2rRxMode::Mono => {
            if radio == focused_radio {
                "Mono receive follows operator focus."
            } else {
                "Audio parked on the focused radio."
            }
        }
        So2rRxMode::Stereo => {
            if radio == 1 {
                "Stereo map: left ear."
            } else {
                "Stereo map: right ear."
            }
        }
        So2rRxMode::ReverseStereo => {
            if radio == 1 {
                "Reverse stereo: right ear."
            } else {
                "Reverse stereo: left ear."
            }
        }
    }
}
