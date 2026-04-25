//! Theme-aware color helpers. Panes call these instead of hardcoding RGB
//! so every iced theme (not just Dark/Light — also Dracula, Nord,
//! Catppuccin, Solarized, etc.) actually looks readable. Semantic colors
//! (dupe red, mult yellow, mult-new green, rig-cursor orange) stay fixed
//! — those have meaning the operator learns.

use iced::theme::palette::Pair;
use iced::widget::{container, text};
use iced::{Border, Color, Theme};

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
pub const TEXT_DISPLAY: f32 = 24.0;

// --- Shape constants for a more modern feel. ---
pub const RADIUS_FRAME: f32 = 6.0;
#[allow(dead_code)]
pub const RADIUS_INPUT: f32 = 4.0;
pub const RADIUS_CHIP: f32 = 3.0;
pub const RADIUS_CARD: f32 = 8.0;

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
// iced built-in). MULT uses the palette's `warning` slot — yellow/gold in
// every built-in theme — so it reads as "heads up, act on this" rather
// than the alarming red of DUPE. CURSOR uses `primary.strong` so it
// stays palette-native and visually distinct from MULT.
//
// The `_pair` variants return the full `Pair` so callers painting a
// filled surface (badge, banner) get the matching readable-text color
// from the same palette slot.

pub fn dupe_pair(t: &Theme) -> Pair {
    t.extended_palette().danger.base
}

pub fn qrm_pair(t: &Theme) -> Pair {
    t.extended_palette().danger.weak
}

pub fn mult_pair(t: &Theme) -> Pair {
    t.extended_palette().warning.base
}

pub fn error_banner_pair(t: &Theme) -> Pair {
    t.extended_palette().danger.strong
}

pub fn mult_color(t: &Theme) -> Color {
    // Used as a foreground accent on the bandmap and in the Available pane.
    // `warning.base.color` is typically yellow/gold in every built-in theme.
    mult_pair(t).color
}

pub fn cursor_color(t: &Theme) -> Color {
    // The theme's primary accent — always palette-native and distinct
    // from MULT (which derives from the warning slot).
    t.extended_palette().primary.strong.color
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

// --- Reusable pane surfaces ---

pub fn card_style(t: &Theme) -> container::Style {
    let pal = t.extended_palette();
    let bg = lerp(pal.background.base.color, pal.background.strong.color, 0.22);
    container::Style {
        background: Some(bg.into()),
        border: Border {
            color: border_color(t),
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
        ..container::Style::default()
    }
}

pub fn accent_card_style(t: &Theme) -> container::Style {
    let pair = t.extended_palette().primary.weak;
    container::Style {
        background: Some(pair.color.into()),
        border: Border {
            color: t.extended_palette().primary.base.color,
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
        text_color: Some(pair.text),
        ..container::Style::default()
    }
}

pub fn badge_style(_t: &Theme, pair: Pair) -> container::Style {
    container::Style {
        background: Some(pair.color.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_CHIP.into(),
        },
        text_color: Some(pair.text),
        ..container::Style::default()
    }
}

pub fn muted_badge_style(t: &Theme) -> container::Style {
    let pal = t.extended_palette().background.strong;
    container::Style {
        background: Some(pal.color.into()),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_CHIP.into(),
        },
        text_color: Some(muted_color(t)),
        ..container::Style::default()
    }
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
