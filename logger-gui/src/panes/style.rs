//! Theme-aware color helpers. Panes call these instead of hardcoding RGB
//! so Light mode actually looks readable. Semantic colors (dupe red, mult
//! yellow, mult-new green, rig-cursor orange) stay fixed — those have
//! meaning the operator learns.

use iced::widget::text;
use iced::{Color, Theme};

pub fn is_light(t: &Theme) -> bool {
    matches!(t, Theme::Light)
}

// --- Raw colors ---

pub fn text_color(t: &Theme) -> Color {
    t.extended_palette().background.base.text
}

pub fn muted_color(t: &Theme) -> Color {
    if is_light(t) {
        Color::from_rgb(0.42, 0.42, 0.48)
    } else {
        Color::from_rgb(0.6, 0.6, 0.65)
    }
}

pub fn very_muted_color(t: &Theme) -> Color {
    if is_light(t) {
        Color::from_rgb(0.62, 0.62, 0.65)
    } else {
        Color::from_rgb(0.42, 0.42, 0.46)
    }
}

pub fn accent_color(t: &Theme) -> Color {
    t.extended_palette().primary.strong.color
}

pub fn accent_text_color(t: &Theme) -> Color {
    if is_light(t) {
        Color::from_rgb(0.15, 0.3, 0.65)
    } else {
        Color::from_rgb(0.55, 0.75, 1.0)
    }
}

pub fn success_color(t: &Theme) -> Color {
    t.extended_palette().success.base.color
}

pub fn danger_color(t: &Theme) -> Color {
    t.extended_palette().danger.base.color
}

pub fn border_color(t: &Theme) -> Color {
    if is_light(t) {
        Color::from_rgb(0.72, 0.72, 0.76)
    } else {
        Color::from_rgb(0.3, 0.3, 0.34)
    }
}

pub fn focused_border_color(t: &Theme) -> Color {
    accent_color(t)
}

// --- Semantic (intentionally theme-independent) ---

pub const MULT_COLOR: Color = Color::from_rgb(0.92, 0.68, 0.12);
pub const CURSOR_COLOR: Color = Color::from_rgb(1.0, 0.65, 0.15);
pub const DUPE_BADGE: Color = Color::from_rgb(0.72, 0.22, 0.22);
pub const MULT_BADGE: Color = Color::from_rgb(0.85, 0.65, 0.1);
pub const QRM_BADGE: Color = Color::from_rgb(0.72, 0.42, 0.02);
pub const ERROR_BANNER: Color = Color::from_rgb(0.65, 0.18, 0.18);

pub fn new_spot_color(t: &Theme) -> Color {
    if is_light(t) {
        Color::from_rgb(0.15, 0.55, 0.22)
    } else {
        Color::from_rgb(0.55, 0.85, 0.4)
    }
}

pub fn worked_spot_color(t: &Theme) -> Color {
    very_muted_color(t)
}

// --- Text style closures (for `.style(...)` on text widgets) ---

pub fn body(t: &Theme) -> text::Style {
    text::Style {
        color: Some(text_color(t)),
    }
}

pub fn muted(t: &Theme) -> text::Style {
    text::Style {
        color: Some(muted_color(t)),
    }
}

pub fn very_muted(t: &Theme) -> text::Style {
    text::Style {
        color: Some(very_muted_color(t)),
    }
}

pub fn accent(t: &Theme) -> text::Style {
    text::Style {
        color: Some(accent_text_color(t)),
    }
}
