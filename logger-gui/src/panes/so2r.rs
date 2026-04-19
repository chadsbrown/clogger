//! SO2R routing-state pane — FOCUS (operator's entry focus), RX (OTRSP
//! audio routing), TX (which radio the keyer last sent to).

use iced::widget::{column, container, row, text, Space};
use iced::{Color, Element, Length, Theme};
use logger_core::{RadioId, So2rRxMode};

use super::style;
use crate::bridge::AdapterHandles;

pub fn view<'a, M: 'a>(
    focused_radio: RadioId,
    tx_radio: RadioId,
    handles: &'a AdapterHandles,
) -> Element<'a, M> {
    let _perf = crate::perf::Span::new("pane.so2r"); // PROFILING
    let rx_mode = handles.so2r_default_rx_mode;
    let rx_str = match rx_mode {
        So2rRxMode::Mono => format!("Mono (R{focused_radio})"),
        So2rRxMode::Stereo => "Stereo  L=R1 R=R2".to_string(),
        So2rRxMode::ReverseStereo => "Stereo  L=R2 R=R1".to_string(),
    };

    let status_line: Element<M> = if !handles.so2r_configured {
        text("(no SO2R configured)")
            .size(12.0)
            .style(style::very_muted)
            .into()
    } else if handles.so2r_connected {
        text("● connected")
            .size(11.0)
            .style(|t: &Theme| iced::widget::text::Style {
                color: Some(style::success_color(t)),
            })
            .into()
    } else {
        text("● configured, not connected")
            .size(11.0)
            .style(|t: &Theme| iced::widget::text::Style {
                color: Some(style::danger_color(t)),
            })
            .into()
    };

    let line = |label: &'static str, value: String, accent_value: bool| -> Element<'a, M> {
        row![
            text(label)
                .size(12.0)
                .style(style::muted)
                .width(Length::Fixed(70.0)),
            text(value).size(14.0).style(move |t: &Theme| {
                if accent_value {
                    style::accent(t)
                } else {
                    style::body(t)
                }
            }),
        ]
        .spacing(4)
        .into()
    };

    let _ = Color::TRANSPARENT;
    container(
        column![
            text("SO2R routing").size(11.0).style(style::accent),
            Space::new().height(6),
            line("FOCUS", format!("R{focused_radio}"), true),
            Space::new().height(2),
            line("RX", rx_str, false),
            Space::new().height(2),
            line("TX", format!("R{tx_radio}"), tx_radio != focused_radio),
            Space::new().height(10),
            status_line,
            Space::new().height(4),
            text("Press ` to toggle RX mono/stereo")
                .size(10.0)
                .style(style::very_muted),
        ]
        .spacing(0),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
