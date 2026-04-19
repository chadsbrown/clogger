//! Theme-aware color helpers. Panes call these instead of hardcoding RGB
//! so every iced theme (not just Dark/Light — also Dracula, Nord,
//! Catppuccin, Solarized, etc.) actually looks readable. Semantic colors
//! (dupe red, mult yellow, mult-new green, rig-cursor orange) stay fixed
//! — those have meaning the operator learns.

use iced::widget::text;
use iced::{Color, Theme};

// --- Unified text sizes (in logical pixels; the global font_scale
// multiplies these via iced's scale_factor). `f32` because iced 0.14's
// text widget `.size()` wants `impl Into<Pixels>`, and `Pixels` has
// `From<f32>` / `From<u32>` but not `From<u16>`.
#[allow(dead_code)]
pub const TEXT_TINY: f32 = 10.0;
pub const TEXT_LABEL: f32 = 11.0;
pub const TEXT_BODY: f32 = 13.0;
#[allow(dead_code)]
pub const TEXT_VALUE: f32 = 14.0;
pub const TEXT_HEADER: f32 = 15.0;

// --- Shape constants for a more modern feel. ---
pub const RADIUS_FRAME: f32 = 6.0;
#[allow(dead_code)]
pub const RADIUS_INPUT: f32 = 4.0;
pub const RADIUS_CHIP: f32 = 3.0;

/// Luminance-based "is this a light-ish theme?" detector. Returns `true`
/// when the theme's base background luminance is above 0.5. Works across
/// every iced built-in (SolarizedLight, GruvboxLight, CatppuccinLatte,
/// TokyoNightLight, KanagawaLotus, ...) plus any user-supplied custom
/// themes — a simple `matches!(t, Theme::Light)` would miss them all.
pub fn is_light(t: &Theme) -> bool {
    let bg = t.extended_palette().background.base.color;
    // Rec. 709 relative luminance.
    let l = 0.2126 * bg.r + 0.7152 * bg.g + 0.0722 * bg.b;
    l > 0.5
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
    // Derived from the palette so theme-specific surface tones carry
    // through (e.g. Gruvbox's warm grays, Nord's cool grays). The blend
    // stays subtle enough to read as a border, not a second-tier
    // emphasis.
    let pal = t.extended_palette().background.strong;
    lerp(pal.color, pal.text, 0.18)
}

pub fn focused_border_color(t: &Theme) -> Color {
    accent_color(t)
}

/// Subtle drop-shadow color for the focused pane. Alpha-blended so it
/// reads as depth rather than a second border in both light and dark
/// themes.
pub fn shadow_color(t: &Theme) -> Color {
    if is_light(t) {
        Color::from_rgba(0.0, 0.0, 0.0, 0.12)
    } else {
        Color::from_rgba(0.0, 0.0, 0.0, 0.45)
    }
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

// --- Semantic (theme-derived) ---
//
// DUPE / QRM / ERROR come from the palette's `danger` slot (red in every
// iced built-in). MULT has no corresponding slot in iced's `Palette` —
// reusing `primary` would collide with focused-pane borders and header
// accents — so it's hand-picked per built-in theme from that theme's
// "yellow/gold" hue, with `primary.strong` as the fallback for custom
// themes. CURSOR does the same with an orange/peach so it stays visually
// distinct from MULT on the bandmap.

pub fn dupe_badge(t: &Theme) -> Color {
    t.extended_palette().danger.base.color
}

pub fn qrm_badge(t: &Theme) -> Color {
    t.extended_palette().danger.weak.color
}

pub fn error_banner(t: &Theme) -> Color {
    t.extended_palette().danger.strong.color
}

pub fn mult_color(t: &Theme) -> Color {
    match t.to_string().as_str() {
        "Gruvbox Dark" => Color::from_rgb8(0xfa, 0xbd, 0x2f),
        "Gruvbox Light" => Color::from_rgb8(0xd7, 0x99, 0x21),
        "Nord" => Color::from_rgb8(0xeb, 0xcb, 0x8b),
        "Dracula" => Color::from_rgb8(0xf1, 0xfa, 0x8c),
        "Solarized Dark" | "Solarized Light" => Color::from_rgb8(0xb5, 0x89, 0x00),
        "Catppuccin Latte" => Color::from_rgb8(0xdf, 0x8e, 0x1d),
        "Catppuccin Frappé" => Color::from_rgb8(0xe5, 0xc8, 0x90),
        "Catppuccin Macchiato" => Color::from_rgb8(0xee, 0xd4, 0x9f),
        "Catppuccin Mocha" => Color::from_rgb8(0xf9, 0xe2, 0xaf),
        "Tokyo Night" | "Tokyo Night Storm" => Color::from_rgb8(0xe0, 0xaf, 0x68),
        "Tokyo Night Light" => Color::from_rgb8(0x8f, 0x5e, 0x15),
        "Kanagawa Wave" => Color::from_rgb8(0xdc, 0xa5, 0x61),
        "Kanagawa Dragon" => Color::from_rgb8(0xc4, 0xb2, 0x8a),
        "Kanagawa Lotus" => Color::from_rgb8(0x77, 0x71, 0x3f),
        "Moonfly" => Color::from_rgb8(0xe3, 0xc7, 0x8a),
        "Nightfly" => Color::from_rgb8(0xe3, 0xd1, 0x8a),
        "Oxocarbon" => Color::from_rgb8(0xee, 0x53, 0x96),
        "Ferra" => Color::from_rgb8(0xe6, 0xb4, 0x50),
        "Light" => Color::from_rgb8(0xb8, 0x86, 0x0b),
        "Dark" => Color::from_rgb8(0xf5, 0xc7, 0x1a),
        _ => t.extended_palette().primary.strong.color,
    }
}

pub fn cursor_color(t: &Theme) -> Color {
    match t.to_string().as_str() {
        "Gruvbox Dark" => Color::from_rgb8(0xfe, 0x80, 0x19),
        "Gruvbox Light" => Color::from_rgb8(0xaf, 0x3a, 0x03),
        "Nord" => Color::from_rgb8(0xd0, 0x87, 0x70),
        "Dracula" => Color::from_rgb8(0xff, 0xb8, 0x6c),
        "Solarized Dark" | "Solarized Light" => Color::from_rgb8(0xcb, 0x4b, 0x16),
        "Catppuccin Latte" => Color::from_rgb8(0xfe, 0x64, 0x0b),
        "Catppuccin Frappé" => Color::from_rgb8(0xef, 0x9f, 0x76),
        "Catppuccin Macchiato" => Color::from_rgb8(0xf5, 0xa9, 0x7f),
        "Catppuccin Mocha" => Color::from_rgb8(0xfa, 0xb3, 0x87),
        "Tokyo Night" | "Tokyo Night Storm" => Color::from_rgb8(0xff, 0x9e, 0x64),
        "Tokyo Night Light" => Color::from_rgb8(0x96, 0x54, 0x27),
        "Kanagawa Wave" => Color::from_rgb8(0xff, 0xa0, 0x66),
        "Kanagawa Dragon" => Color::from_rgb8(0xb6, 0x92, 0x7b),
        "Kanagawa Lotus" => Color::from_rgb8(0xc7, 0x60, 0x22),
        "Moonfly" => Color::from_rgb8(0xf0, 0x9f, 0x72),
        "Nightfly" => Color::from_rgb8(0xf7, 0x8c, 0x6c),
        "Oxocarbon" => Color::from_rgb8(0xff, 0x7e, 0xb6),
        "Ferra" => Color::from_rgb8(0xd1, 0x85, 0x51),
        "Light" => Color::from_rgb8(0xd9, 0x53, 0x13),
        "Dark" => Color::from_rgb8(0xff, 0x9a, 0x3c),
        _ => t.extended_palette().primary.strong.color,
    }
}

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
