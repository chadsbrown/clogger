//! Bandmap pane — vertical-axis frequency canvas for a specific radio.
//! Scroll wheel zooms in/out; at any zoom level < 1, the view stays
//! centered on the rig's current frequency (clamped to band edges).
//! Click tunes; clicks within 5 kHz of a spot snap to the spot's exact
//! frequency and populate the entry's CALL field.

use std::collections::HashSet;

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use logger_core::{contest::BandmapCache, AppState, RadioId};
use logger_runtime::{compute_worked_calls, LogAdapter};

use super::style;

const AXIS_X: f32 = 56.0;
const SNAP_HZ: u64 = 5_000;
/// Zoom factor multiplier for each scroll-wheel click (in/out).
const ZOOM_STEP: f32 = 0.75;
/// Don't zoom past this fraction of the full band — keeps a few spots
/// on screen at most-zoomed-in.
const MIN_ZOOM: f32 = 0.02;

pub fn view<'a, M: 'a + Clone + Send + Sync + 'static>(
    state: &'a AppState,
    log: &'a LogAdapter,
    radio_id: RadioId,
    zoom: f32,
    on_click: fn(Option<String>, u64) -> M,
    on_zoom: fn(f32) -> M,
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
        zoom: zoom.clamp(MIN_ZOOM, 1.0),
        on_click,
        on_zoom,
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

/// Zoom lives on the host (`App.bandmap_zoom[radio]`), NOT in the Canvas's
/// `Program::State` — iced's widget-tree diffing keys canvas state by the
/// widget's position in the tree, so z-order changes (from clicking panes)
/// would otherwise reset zoom to 1.0.
struct BandmapProgram<'a, M> {
    spots: &'a [logger_core::state::Spot],
    worked: HashSet<String>,
    mults: HashSet<String>,
    band_low_hz: u64,
    band_high_hz: u64,
    cursor_hz: u64,
    zoom: f32,
    on_click: fn(Option<String>, u64) -> M,
    on_zoom: fn(f32) -> M,
}

impl<'a, M> BandmapProgram<'a, M> {
    /// Current visible frequency range. Centered on `cursor_hz` when
    /// there's room; clamped to band edges otherwise.
    fn visible_range(&self) -> (u64, u64) {
        let z = self.zoom.clamp(MIN_ZOOM, 1.0);
        if z >= 1.0 || self.cursor_hz == 0 {
            return (self.band_low_hz, self.band_high_hz);
        }
        let full_span = self.band_high_hz - self.band_low_hz;
        let span = ((full_span as f32 * z) as u64).max(1000);
        let half = span / 2;
        let cursor = self.cursor_hz.clamp(self.band_low_hz, self.band_high_hz);
        let want_low = cursor.saturating_sub(half);
        let want_high = cursor.saturating_add(half);
        if want_low < self.band_low_hz {
            (self.band_low_hz, self.band_low_hz + span)
        } else if want_high > self.band_high_hz {
            (self.band_high_hz.saturating_sub(span), self.band_high_hz)
        } else {
            (want_low, want_high)
        }
    }
}

/// Pick sensible tick + label intervals for the currently visible span.
fn tick_intervals(visible_span_hz: u64) -> (u64, u64) {
    // (tick_hz, label_hz) — labels only on multiples of `label_hz`.
    if visible_span_hz <= 30_000 {
        (1_000, 5_000)
    } else if visible_span_hz <= 100_000 {
        (2_000, 10_000)
    } else if visible_span_hz <= 200_000 {
        (5_000, 20_000)
    } else {
        (5_000, 25_000)
    }
}

impl<'a, M> canvas::Program<M> for BandmapProgram<'a, M> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        use iced::mouse::{Button, Event as MouseEvent, ScrollDelta};

        match event {
            canvas::Event::Mouse(MouseEvent::WheelScrolled { delta }) => {
                if cursor.position_in(bounds).is_none() {
                    return None;
                }
                let y = match delta {
                    ScrollDelta::Lines { y, .. } => *y,
                    // ~20 pixels per "line" is a rough but reasonable match
                    // for a typical mouse wheel click on most platforms.
                    ScrollDelta::Pixels { y, .. } => y / 20.0,
                };
                let new_zoom = if y > 0.0 {
                    (self.zoom * ZOOM_STEP).max(MIN_ZOOM)
                } else if y < 0.0 {
                    (self.zoom / ZOOM_STEP).min(1.0)
                } else {
                    return None;
                };
                Some(canvas::Action::publish((self.on_zoom)(new_zoom)).and_capture())
            }
            canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    if pos.x < AXIS_X {
                        return None;
                    }
                    let (vlow, vhigh) = self.visible_range();
                    let band_span = (vhigh - vlow).max(1) as f32;
                    let raw = vlow
                        + ((pos.y.clamp(0.0, bounds.height) / bounds.height) * band_span) as u64;
                    let snapped = snap_to_spot_with_call(self.spots, raw);
                    let (call, target) = match snapped {
                        Some((call, freq)) => (Some(call), freq),
                        None => (None, raw),
                    };
                    Some(canvas::Action::publish((self.on_click)(call, target)).and_capture())
                } else {
                    None
                }
            }
            _ => None,
        }
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

        let (vlow, vhigh) = self.visible_range();
        let visible_span = vhigh - vlow;
        let span_f = visible_span.max(1) as f32;
        let y_for = |hz: u64| -> f32 {
            let offset = hz.saturating_sub(vlow) as f32;
            (offset / span_f) * h
        };

        let pal = theme.extended_palette();
        let bg_color = pal.background.weak.color;
        let axis_color = if style::is_light(theme) {
            Color::from_rgb(0.55, 0.55, 0.6)
        } else {
            Color::from_rgb(0.3, 0.3, 0.35)
        };
        let label_color = style::muted_color(theme);
        let text_color = pal.background.base.text;

        frame.fill(
            &Path::rectangle(Point::ORIGIN, Size::new(w, h)),
            bg_color,
        );

        frame.stroke(
            &Path::line(Point::new(AXIS_X, 0.0), Point::new(AXIS_X, h)),
            Stroke::default().with_width(1.0).with_color(axis_color),
        );

        let (tick_hz, label_hz) = tick_intervals(visible_span);
        let first_tick = vlow.div_ceil(tick_hz) * tick_hz;
        let mut hz = first_tick;
        while hz <= vhigh {
            let y = y_for(hz);
            let is_label = hz % label_hz == 0;
            let tick_len = if is_label { 8.0 } else { 4.0 };
            frame.stroke(
                &Path::line(Point::new(AXIS_X - tick_len, y), Point::new(AXIS_X, y)),
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
            hz = hz.saturating_add(tick_hz);
            if hz == 0 {
                break;
            }
        }

        // Cluster visible spots whose rendered y-positions would cause
        // label overlap (same freq, or close enough that 12-pt text would
        // smear together). Within a cluster, stagger labels vertically
        // around the cluster's geometric center and draw a leader line
        // from each dot to its displaced label. Dots stay at the true
        // frequency; only the label moves.
        struct Placement<'b> {
            spot: &'b logger_core::state::Spot,
            y: f32,
        }
        let mut placements: Vec<Placement> = self
            .spots
            .iter()
            .filter(|s| s.freq_hz >= vlow && s.freq_hz <= vhigh)
            .map(|s| Placement {
                spot: s,
                y: y_for(s.freq_hz),
            })
            .collect();
        placements.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));

        // Minimum vertical distance between adjacent label baselines. Set
        // slightly larger than the rendered text height so descenders don't
        // clip into the next label.
        const LABEL_SPACE: f32 = 13.0;
        let leader_color = style::very_muted_color(theme);
        let dot_size = 6.0;

        let mut i = 0;
        while i < placements.len() {
            // Grow the cluster while consecutive placements are closer
            // together than LABEL_SPACE.
            let start = i;
            let mut end = i + 1;
            while end < placements.len()
                && (placements[end].y - placements[end - 1].y).abs() < LABEL_SPACE
            {
                end += 1;
            }
            let members = &placements[start..end];
            let mean_y: f32 = members.iter().map(|p| p.y).sum::<f32>() / members.len() as f32;
            let spread = (members.len() - 1) as f32 * LABEL_SPACE;
            let first_label_y = mean_y - spread / 2.0;

            for (j, p) in members.iter().enumerate() {
                let label_y = first_label_y + j as f32 * LABEL_SPACE;
                let (dot_color, call_color) = if self.worked.contains(&p.spot.call) {
                    (
                        style::worked_spot_color(theme),
                        style::very_muted_color(theme),
                    )
                } else if self.mults.contains(&p.spot.call) {
                    (style::MULT_COLOR, style::MULT_COLOR)
                } else {
                    (style::new_spot_color(theme), text_color)
                };

                // Dot at the true frequency (not the label position).
                frame.fill(
                    &Path::rectangle(
                        Point::new(AXIS_X - dot_size / 2.0, p.y - dot_size / 2.0),
                        Size::new(dot_size, dot_size),
                    ),
                    dot_color,
                );

                // Leader line from the dot to the label's left edge, drawn
                // only when the label is actually displaced. Colored muted
                // so it doesn't compete with the call text.
                if (label_y - p.y).abs() > 1.5 {
                    let leader_start_x = AXIS_X + dot_size / 2.0;
                    let leader_end_x = AXIS_X + 10.0;
                    frame.stroke(
                        &Path::line(
                            Point::new(leader_start_x, p.y),
                            Point::new(leader_end_x, label_y),
                        ),
                        Stroke::default().with_width(0.75).with_color(leader_color),
                    );
                }

                // Horizontal nudge based on position within the cluster so
                // the labels have a slight diagonal cascade — easier to
                // follow when many spots stack up at one frequency.
                let label_x_nudge = if members.len() > 1 {
                    (j as f32).min(4.0) * 2.0
                } else {
                    0.0
                };
                frame.fill_text(Text {
                    content: p.spot.call.clone(),
                    position: Point::new(AXIS_X + 12.0 + label_x_nudge, label_y - 7.0),
                    color: call_color,
                    size: 12.0.into(),
                    ..Text::default()
                });
            }

            i = end;
        }

        if self.cursor_hz >= vlow && self.cursor_hz <= vhigh {
            let y = y_for(self.cursor_hz);
            frame.stroke(
                &Path::line(Point::new(AXIS_X, y), Point::new(w, y)),
                Stroke::default()
                    .with_width(1.5)
                    .with_color(style::CURSOR_COLOR),
            );
        }

        // Zoom indicator in the top-right — "100%" when full band,
        // smaller values as the user zooms in.
        let pct = (self.zoom.clamp(MIN_ZOOM, 1.0) * 100.0).round() as u32;
        if pct < 100 {
            frame.fill_text(Text {
                content: format!("{pct}%"),
                position: Point::new(w - 40.0, 4.0),
                color: label_color,
                size: 10.0.into(),
                ..Text::default()
            });
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
