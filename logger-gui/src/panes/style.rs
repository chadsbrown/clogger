//! Design tokens and theme-derived colors for the GUI.
//!
//! Every pane, modal and piece of chrome takes sizes, spacing, radii,
//! fonts and colors from here instead of hardcoding them. That keeps the
//! app consistent, and it makes every iced built-in theme (Dracula, Nord,
//! Gruvbox, Solarized, Catppuccin, …) readable, not only Dark and Light.
//!
//! Design rationale lives in `docs/gui-visual-design.md`. In short:
//! * **Neutral chrome, restrained accent.** The primary/accent color is
//!   reserved for focus, the active radio, the newest QSO and alerts, so
//!   those are what the eye finds first.
//! * **Neutrals are derived from the palette** (blends of the theme's
//!   background and text colors) rather than fixed grays, so they pick up
//!   each theme's tint (Gruvbox warm, Nord cool, …).
//! * **Semantic colors stay fixed in meaning.** DUPE is danger, MULT is
//!   warning, TX is danger and the rig cursor is primary. Operators learn
//!   these and they must not move between releases.

use iced::theme::palette::Pair;
use iced::widget::{container, text};
use iced::{Border, Color, Font, Padding, Shadow, Theme, Vector, font};

// ---------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------

// Type scale in logical pixels. The global font scale multiplies these
// via iced's `scale_factor`. Nothing goes below 11px: in an earlier
// version, muted 10px text was hard to read on dark themes.
pub const TEXT_TINY: f32 = 11.0;
pub const TEXT_LABEL: f32 = 12.0;
pub const TEXT_BODY: f32 = 13.0;
pub const TEXT_VALUE: f32 = 15.0;
pub const TEXT_HEADER: f32 = 16.0;
pub const TEXT_DISPLAY: f32 = 28.0;

/// UI face: Inter. It is bundled (see `FONT_FILES`) so the look is the
/// same on every machine instead of depending on whatever sans-serif
/// fontconfig picks.
pub const FONT_UI: Font = Font::with_name("Inter");
pub const FONT_UI_MEDIUM: Font = Font {
    weight: font::Weight::Medium,
    ..FONT_UI
};
pub const FONT_UI_SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..FONT_UI
};
/// Data face: JetBrains Mono, for calls, frequencies, exchanges and
/// numbers. Its fixed-width digits keep columns and changing counters
/// steady.
pub const FONT_MONO: Font = Font::with_name("JetBrains Mono");
pub const FONT_MONO_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..FONT_MONO
};
/// Icon face: Lucide, subset to the glyphs in [`Icon`].
pub const FONT_ICON: Font = Font::with_name("lucide");

/// Font binaries registered with iced at startup. They are subset to
/// Latin plus the punctuation, arrows and symbols the app draws, which
/// keeps the binary small (about 400 KB in total). Licenses sit next to
/// the files in `assets/fonts/`.
pub const FONT_FILES: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/lucide.ttf"),
];

/// Icons available in the bundled Lucide subset. To add one, find its
/// codepoint in lucide-static's `font/info.json`, add it to the
/// `pyftsubset --unicodes` list (see `assets/fonts/README.md`), then add
/// a variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    X,
    Palette,
    Help,
    Download,
    TextSmaller,
    TextLarger,
    Alert,
    Check,
}

impl Icon {
    pub fn codepoint(self) -> char {
        match self {
            Icon::X => '\u{e1b2}',
            Icon::Palette => '\u{e1dd}',
            Icon::Help => '\u{e082}',
            Icon::Download => '\u{e0b6}',
            Icon::TextSmaller => '\u{e58a}',
            Icon::TextLarger => '\u{e58b}',
            Icon::Alert => '\u{e193}',
            Icon::Check => '\u{e070}',
        }
    }
}

/// A Lucide glyph as a text widget. The line height is 1.0 so the glyph
/// box is square and centers cleanly in buttons.
pub fn icon<'a>(i: Icon, size: f32) -> iced::widget::Text<'a> {
    text(i.codepoint().to_string())
        .font(FONT_ICON)
        .size(size)
        .line_height(1.0)
}

// ---------------------------------------------------------------------
// Spacing, padding, radii
// ---------------------------------------------------------------------

// A 4px-based spacing scale (with a 2px half-step). Use these for
// `.spacing()` and `Space` sizes so rhythm stays consistent.
pub const SP_1: f32 = 2.0;
pub const SP_2: f32 = 4.0;
pub const SP_3: f32 = 8.0;
pub const SP_4: f32 = 12.0;
pub const SP_5: f32 = 16.0;
#[allow(dead_code)]
pub const SP_6: f32 = 24.0;

/// Inner padding of a pane body.
pub const PAD_PANE: Padding = Padding {
    top: 8.0,
    right: 10.0,
    bottom: 8.0,
    left: 10.0,
};
/// Padding for a table row.
pub const PAD_ROW: Padding = Padding {
    top: 3.0,
    right: 6.0,
    bottom: 3.0,
    left: 6.0,
};
/// Padding for a badge or chip.
pub const PAD_BADGE: Padding = Padding {
    top: 1.0,
    right: 6.0,
    bottom: 1.0,
    left: 6.0,
};
/// Padding for a text button.
pub const PAD_BUTTON: Padding = Padding {
    top: 5.0,
    right: 14.0,
    bottom: 5.0,
    left: 14.0,
};
/// Padding for a raised tile (metric/stat cards).
pub const PAD_TILE: Padding = Padding {
    top: 8.0,
    right: 10.0,
    bottom: 8.0,
    left: 10.0,
};

/// Badges, inputs, keycaps and table rows.
pub const RADIUS_SM: f32 = 4.0;
/// Panes, buttons and tiles.
pub const RADIUS_MD: f32 = 6.0;
/// Modals.
pub const RADIUS_LG: f32 = 10.0;

// ---------------------------------------------------------------------
// Color math
// ---------------------------------------------------------------------

pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

fn channel_lin(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
fn luminance(c: Color) -> f32 {
    0.2126 * channel_lin(c.r) + 0.7152 * channel_lin(c.g) + 0.0722 * channel_lin(c.b)
}

/// WCAG contrast ratio, from 1.0 up to 21.0.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Nudge `fg` toward black or white, whichever moves it away from `bg`,
/// until it reaches `min` contrast against `bg`. This lets a theme's
/// accent hue be used for text without losing legibility. Some
/// built-ins (Solarized Light, Gruvbox Light) have primaries too pale to
/// read as text on their own background.
pub fn ensure_contrast(fg: Color, bg: Color, min: f32) -> Color {
    if contrast(fg, bg) >= min {
        return fg;
    }
    let target = if luminance(bg) > 0.4 {
        Color::BLACK
    } else {
        Color::WHITE
    };
    let mut t = 0.0;
    while t <= 1.0 {
        let c = lerp(fg, target, t);
        if contrast(c, bg) >= min {
            return c;
        }
        t += 0.05;
    }
    target
}

/// Whether the theme's base background is light (Rec. 709 luminance
/// above 0.5). This checks the actual color rather than matching
/// `Theme::Light`, so it also works for SolarizedLight, CatppuccinLatte,
/// TokyoNightLight, KanagawaLotus and custom themes.
pub fn is_light(t: &Theme) -> bool {
    let bg = t.extended_palette().background.base.color;
    let l = 0.2126 * bg.r + 0.7152 * bg.g + 0.0722 * bg.b;
    l > 0.5
}

// ---------------------------------------------------------------------
// Neutrals and surfaces
// ---------------------------------------------------------------------

fn base(t: &Theme) -> Pair {
    t.extended_palette().background.base
}

pub fn text_color(t: &Theme) -> Color {
    base(t).text
}

/// Secondary text: labels, units, column headers. The blend is
/// guaranteed at least 3:1 contrast, which matters for themes whose own
/// body text is already low-contrast (Solarized Light).
pub fn muted_color(t: &Theme) -> Color {
    let b = base(t);
    ensure_contrast(lerp(b.color, b.text, 0.64), b.color, 3.0)
}

/// Tertiary text: placeholders, captions, worked spots. It is meant to
/// recede, but is kept at 2:1 or better so it stays visible.
pub fn very_muted_color(t: &Theme) -> Color {
    let b = base(t);
    ensure_contrast(lerp(b.color, b.text, 0.44), b.color, 2.0)
}

/// Hairline borders and dividers.
pub fn border_color(t: &Theme) -> Color {
    let b = base(t);
    lerp(b.color, b.text, if is_light(t) { 0.16 } else { 0.14 })
}

/// The workspace behind the panes. It is a step darker than the pane
/// surface in both modes, so panes read as sitting on top of it.
pub fn surface_canvas(t: &Theme) -> Color {
    let b = base(t);
    if is_light(t) {
        lerp(b.color, b.text, 0.07)
    } else {
        lerp(b.color, Color::BLACK, 0.32)
    }
}

/// Pane, toolbar, status-bar and modal surface.
pub fn surface_pane(t: &Theme) -> Color {
    base(t).color
}

/// A slightly raised surface for metric tiles and input boxes.
pub fn surface_raised(t: &Theme) -> Color {
    let b = base(t);
    lerp(b.color, b.text, 0.05)
}

/// A sunken surface for text-entry boxes, set into the raised radio card.
pub fn surface_sunken(t: &Theme) -> Color {
    lerp(surface_pane(t), surface_canvas(t), 0.6)
}

/// Background of the alternating stripe in tables.
pub fn row_stripe(t: &Theme) -> Color {
    let b = base(t);
    lerp(b.color, b.text, 0.028)
}

/// A faint tint behind something the accent marks, such as the newest
/// QSO or the active radio.
pub fn accent_tint(t: &Theme) -> Color {
    lerp(base(t).color, accent_color(t), 0.10)
}

// ---------------------------------------------------------------------
// Accent and semantic colors
// ---------------------------------------------------------------------

/// The theme's primary color. Used for fills and lines (focus border,
/// caret, rig cursor, accent bars).
pub fn accent_color(t: &Theme) -> Color {
    t.extended_palette().primary.base.color
}

/// The primary hue adjusted so it is readable as text on the pane
/// surface. It follows the theme's own accent (a non-blue primary stays
/// non-blue), unlike the earlier hardcoded blue.
pub fn accent_text_color(t: &Theme) -> Color {
    ensure_contrast(accent_color(t), surface_pane(t), 4.5)
}

pub fn danger_color(t: &Theme) -> Color {
    t.extended_palette().danger.base.color
}

pub fn danger_text_color(t: &Theme) -> Color {
    ensure_contrast(danger_color(t), surface_pane(t), 4.5)
}

pub fn success_color(t: &Theme) -> Color {
    t.extended_palette().success.base.color
}

pub fn warning_color(t: &Theme) -> Color {
    t.extended_palette().warning.base.color
}

pub fn focused_border_color(t: &Theme) -> Color {
    accent_color(t)
}

/// Resting elevation for every pane: a soft, tight shadow.
pub fn shadow_rest(t: &Theme) -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, if is_light(t) { 0.08 } else { 0.30 }),
        offset: Vector::new(0.0, 1.0),
        blur_radius: 4.0,
    }
}

/// Raised elevation for the focused pane and modals.
pub fn shadow_focus(t: &Theme) -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, if is_light(t) { 0.16 } else { 0.50 }),
        offset: Vector::new(0.0, 4.0),
        blur_radius: 16.0,
    }
}

// Semantic palette slots. DUPE, QRM, TX and errors use `danger` (red
// in every built-in). MULT uses `warning` (yellow or gold), which reads
// as "act on this" without the alarm of DUPE. The cursor uses primary.
// Badges reach these through `widgets::Tone`.

pub fn error_banner_pair(t: &Theme) -> Pair {
    t.extended_palette().danger.strong
}

/// MULT as a foreground color (bandmap call labels, score and available
/// counts). It is contrast-adjusted because raw warning yellow is
/// unreadable on light themes.
pub fn mult_color(t: &Theme) -> Color {
    ensure_contrast(warning_color(t), surface_pane(t), 3.0)
}

pub fn cursor_color(t: &Theme) -> Color {
    accent_color(t)
}

/// Unworked, non-mult spot dot on the bandmap.
pub fn new_spot_color(t: &Theme) -> Color {
    ensure_contrast(success_color(t), surface_pane(t), 3.0)
}

pub fn worked_spot_color(t: &Theme) -> Color {
    very_muted_color(t)
}

// ---------------------------------------------------------------------
// Container styles
// ---------------------------------------------------------------------

/// Raised tile: metric cards and grouped stats. Use it sparingly.
/// Tables are flat (see `widgets::table_row`), not a card per row.
pub fn tile_style(t: &Theme) -> container::Style {
    container::Style {
        background: Some(surface_raised(t).into()),
        border: Border {
            color: border_color(t),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

// ---------------------------------------------------------------------
// Text style closures (for `.style(...)` on text widgets)
// ---------------------------------------------------------------------

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

pub fn danger(t: &Theme) -> text::Style {
    text::Style {
        color: Some(danger_text_color(t)),
    }
}

pub fn mult(t: &Theme) -> text::Style {
    text::Style {
        color: Some(mult_color(t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_extremes() {
        let c = contrast(Color::BLACK, Color::WHITE);
        assert!((c - 21.0).abs() < 0.01, "{c}");
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ensure_contrast_reaches_target_on_every_builtin() {
        for theme in Theme::ALL {
            let bg = surface_pane(theme);
            let fg = accent_text_color(theme);
            assert!(
                contrast(fg, bg) >= 4.5 - 1e-3,
                "{theme}: accent text contrast {}",
                contrast(fg, bg)
            );
            assert!(contrast(muted_color(theme), bg) >= 3.0, "{theme}: muted too faint");
        }
    }
}
