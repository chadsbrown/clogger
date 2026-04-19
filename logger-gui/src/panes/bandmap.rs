//! Bandmap pane — vertical-axis frequency canvas for a specific radio.
//! Spots are color-coded per that radio's band+mode:
//!   - worked (dupe) → gray
//!   - mult-needed → yellow
//!   - new → green
//! Click anywhere below the axis to tune that radio. Clicks within 5 kHz of
//! a spot snap to the spot's exact frequency and populate the entry's CALL.

use std::collections::HashSet;

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use logger_core::{contest::BandmapCache, AppState, RadioId};
use logger_runtime::{compute_worked_calls, LogAdapter};

use super::style;

const AXIS_X: f32 = 56.0;
const TICK_KHZ: u64 = 5;
const LABEL_KHZ: u64 = 25;
const SNAP_HZ: u64 = 5_000;

pub fn view<'a, M: 'a + Clone + Send + Sync + 'static>(
    state: &'a AppState,
    log: &'a LogAdapter,
    radio_id: RadioId,
    on_click: fn(Option<String>, u64) -> M,
) -> Element<'a, M> {
    let radio = state.radios.get(&radio_id);
    let cursor_hz = radio.map(|r| r.freq_hz).unwrap_or(0);
    let mode = radio.map(|r| r.mode.as_str()).unwrap_or("CW");
    let (band_low, band_high) = pick_band(cursor_hz);

    let mut cache = BandmapCache::new();
    let worked = if cursor_hz > 0 {
        compute_worked_calls(
            &state.bandmap,
            state.bandmap_version,
            &mut cache,
            cursor_hz,
            mode,
            log,
        )
    } else {
        logger_runtime::WorkedCalls::default()
    };

    Canvas::new(BandmapProgram {
        spots: &state.bandmap,
        worked: worked.worked,
        mults: worked.mults,
        band_low_hz: band_low,
        band_high_hz: band_high,
        cursor_hz,
        on_click,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn pick_band(cursor_hz: u64) -> (u64, u64) {
    const BANDS: &[(u64, u64)] = &[
        (1_800_000, 2_000_000),
        (3_500_000, 4_000_000),
        (7_000_000, 7_300_000),
        (14_000_000, 14_350_000),
        (21_000_000, 21_450_000),
        (28_000_000, 29_700_000),
    ];
    for (lo, hi) in BANDS {
        if cursor_hz >= *lo && cursor_hz <= *hi {
            return (*lo, *hi);
        }
    }
    (14_000_000, 14_350_000)
}

fn snap_to_spot_with_call(
    spots: &[logger_core::state::Spot],
    target_hz: u64,
) -> Option<(String, u64)> {
    let mut best: Option<(u64, &logger_core::state::Spot)> = None;
    for s in spots {
        let d = s.freq_hz.abs_diff(target_hz);
        if d <= SNAP_HZ {
            match best {
                None => best = Some((d, s)),
                Some((bd, _)) if d < bd => best = Some((d, s)),
                _ => {}
            }
        }
    }
    best.map(|(_, s)| (s.call.clone(), s.freq_hz))
}

struct BandmapProgram<'a, M> {
    spots: &'a [logger_core::state::Spot],
    worked: HashSet<String>,
    mults: HashSet<String>,
    band_low_hz: u64,
    band_high_hz: u64,
    cursor_hz: u64,
    on_click: fn(Option<String>, u64) -> M,
}

impl<'a, M> canvas::Program<M> for BandmapProgram<'a, M> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<M>) {
        use iced::mouse::{Button, Event as MouseEvent};
        if let canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                if pos.x < AXIS_X {
                    return (canvas::event::Status::Ignored, None);
                }
                let band_span = (self.band_high_hz - self.band_low_hz).max(1) as f32;
                let raw = self.band_low_hz
                    + ((pos.y.clamp(0.0, bounds.height) / bounds.height) * band_span) as u64;
                let snapped = snap_to_spot_with_call(self.spots, raw);
                let (call, target) = match snapped {
                    Some((call, freq)) => (Some(call), freq),
                    None => (None, raw),
                };
                return (
                    canvas::event::Status::Captured,
                    Some((self.on_click)(call, target)),
                );
            }
        }
        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let h = bounds.height;
        let w = bounds.width;

        let band_span = (self.band_high_hz - self.band_low_hz).max(1) as f32;
        let y_for = |hz: u64| -> f32 {
            let offset = hz.saturating_sub(self.band_low_hz) as f32;
            (offset / band_span) * h
        };

        // Theme-driven background and gridlines.
        let pal = theme.extended_palette();
        let bg_color = pal.background.weak.color;
        let axis_color = if style::is_light(theme) {
            Color::from_rgb(0.55, 0.55, 0.6)
        } else {
            Color::from_rgb(0.3, 0.3, 0.35)
        };
        let label_color = style::muted_color(theme);
        let text_color = pal.background.base.text;

        let bg = Path::rectangle(Point::ORIGIN, Size::new(w, h));
        frame.fill(&bg, bg_color);

        let axis = Path::line(Point::new(AXIS_X, 0.0), Point::new(AXIS_X, h));
        frame.stroke(
            &axis,
            Stroke::default().with_width(1.0).with_color(axis_color),
        );

        let mut hz = (self.band_low_hz / (TICK_KHZ * 1000)) * (TICK_KHZ * 1000);
        if hz < self.band_low_hz {
            hz += TICK_KHZ * 1000;
        }
        while hz <= self.band_high_hz {
            let y = y_for(hz);
            let is_label = hz % (LABEL_KHZ * 1000) == 0;
            let tick_len = if is_label { 8.0 } else { 4.0 };
            let tick = Path::line(Point::new(AXIS_X - tick_len, y), Point::new(AXIS_X, y));
            frame.stroke(
                &tick,
                Stroke::default().with_width(1.0).with_color(axis_color),
            );
            if is_label {
                frame.fill_text(Text {
                    content: format!("{:.3}", hz as f64 / 1_000_000.0),
                    position: Point::new(2.0, y - 6.0),
                    color: label_color,
                    size: 10.0.into(),
                    ..Text::default()
                });
            }
            hz += TICK_KHZ * 1000;
        }

        for spot in self.spots {
            if spot.freq_hz < self.band_low_hz || spot.freq_hz > self.band_high_hz {
                continue;
            }
            let y = y_for(spot.freq_hz);
            let (dot_color, call_color) = if self.worked.contains(&spot.call) {
                (style::worked_spot_color(theme), style::very_muted_color(theme))
            } else if self.mults.contains(&spot.call) {
                (style::MULT_COLOR, style::MULT_COLOR)
            } else {
                (style::new_spot_color(theme), text_color)
            };
            let dot_size = 6.0;
            let dot = Path::rectangle(
                Point::new(AXIS_X - dot_size / 2.0, y - dot_size / 2.0),
                Size::new(dot_size, dot_size),
            );
            frame.fill(&dot, dot_color);
            frame.fill_text(Text {
                content: spot.call.clone(),
                position: Point::new(AXIS_X + 6.0, y - 7.0),
                color: call_color,
                size: 12.0.into(),
                ..Text::default()
            });
        }

        if self.cursor_hz >= self.band_low_hz && self.cursor_hz <= self.band_high_hz {
            let y = y_for(self.cursor_hz);
            let line = Path::line(Point::new(AXIS_X, y), Point::new(w, y));
            frame.stroke(
                &line,
                Stroke::default()
                    .with_width(1.5)
                    .with_color(style::CURSOR_COLOR),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(pos) if pos.x >= AXIS_X => mouse::Interaction::Crosshair,
            _ => mouse::Interaction::default(),
        }
    }
}
