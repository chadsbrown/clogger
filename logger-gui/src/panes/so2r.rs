//! SO2R routing-state pane — FOCUS (operator's entry focus), RX (OTRSP
//! audio routing), TX (which radio the keyer last sent to).

use iced::widget::{column, row, text};
use iced::{Element, Length, Theme};
use logger_core::{RadioId, So2rRxMode};

use super::style;
use super::widgets::{self, Tone};
use crate::bridge::AdapterHandles;

pub fn view<'a, M: 'a>(
    focused_radio: RadioId,
    tx_radio: RadioId,
    handles: &'a AdapterHandles,
) -> Element<'a, M> {
    let rx_mode = handles.so2r_default_rx_mode;
    widgets::pane_body(
        column![
            row![
                radio_card(1, focused_radio, tx_radio, rx_mode),
                radio_card(2, focused_radio, tx_radio, rx_mode),
            ]
            .spacing(style::SP_3)
            .height(Length::Fill),
            row![
                widgets::keycap("`", None),
                text("toggles RX mono / stereo")
                    .size(style::TEXT_TINY)
                    .style(style::very_muted),
            ]
            .spacing(style::SP_2)
            .align_y(iced::alignment::Vertical::Center),
        ]
        .spacing(style::SP_3),
    )
}

/// One radio's routing card. A radio you can hear gets an accent bar and
/// an accent "R1"/"R2", with focus, TX and RX role as badges. Earlier
/// versions filled the whole audible card with primary color, so in
/// stereo both cards were bright and the distinction disappeared.
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

    let mut badges: Vec<Element<M>> = vec![widgets::badge(
        rx,
        if audible { Tone::Accent } else { Tone::Neutral },
    )];
    if focus {
        badges.push(widgets::badge("FOCUS", Tone::Accent));
    }
    if tx {
        badges.push(widgets::badge_solid("TX", Tone::Danger));
    }

    widgets::tile(
        column![
            row![
                text(format!("R{radio}"))
                    .size(style::TEXT_DISPLAY)
                    .font(style::FONT_MONO)
                    .style(move |t: &Theme| if audible { style::accent(t) } else { style::muted(t) }),
                text(if audible { "audio on" } else { "muted" })
                    .size(style::TEXT_TINY)
                    .style(style::very_muted),
            ]
            .spacing(style::SP_3)
            .align_y(iced::alignment::Vertical::Center),
            row(badges).spacing(style::SP_2).wrap(),
            text(rx_detail(radio, focused_radio, rx_mode))
                .size(style::TEXT_TINY)
                .style(style::muted),
        ]
        .spacing(style::SP_3),
        audible.then_some(Tone::Accent),
        true,
    )
    .width(Length::FillPortion(1))
    .into()
}

fn is_audible(radio: RadioId, focused_radio: RadioId, rx_mode: So2rRxMode) -> bool {
    match rx_mode {
        So2rRxMode::Mono => radio == focused_radio,
        So2rRxMode::Stereo | So2rRxMode::ReverseStereo => true,
    }
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
