use std::path::Path;

use anyhow::{Context, bail};
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// ThemeStyle + Theme
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThemeStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub modifiers: Modifier,
}

impl From<ThemeStyle> for Style {
    fn from(ts: ThemeStyle) -> Self {
        let mut s = Style::default();
        if let Some(fg) = ts.fg { s = s.fg(fg); }
        if let Some(bg) = ts.bg { s = s.bg(bg); }
        s = s.add_modifier(ts.modifiers);
        s
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    /// Terminal background. Applied as a full-frame fill before all widgets.
    /// `Color::Reset` means "use the terminal's native background" (default
    /// theme behavior for backward compat).
    pub background: Color,
    pub mode_badge: ThemeStyle,
    pub tx_badge: ThemeStyle,
    pub dupe_badge: ThemeStyle,
    pub serial_number: ThemeStyle,
    pub entry_border_focused: ThemeStyle,
    pub entry_border_unfocused: ThemeStyle,
    pub field_label: ThemeStyle,
    pub field_valid: ThemeStyle,
    pub field_invalid: ThemeStyle,
    pub field_unknown: ThemeStyle,
    pub scp_match: ThemeStyle,
    pub cw_echo: ThemeStyle,
    pub frequency: ThemeStyle,
    pub bandmap_border: ThemeStyle,
    pub bandmap_divider: ThemeStyle,
    pub bandmap_worked: ThemeStyle,
    pub bandmap_mult: ThemeStyle,
    pub bandmap_unworked: ThemeStyle,
    pub bandmap_highlight: ThemeStyle,
    pub status_callsign_badge: ThemeStyle,
    pub status_mult_badge: ThemeStyle,
    pub status_qrm_badge: ThemeStyle,
    pub status_clock: ThemeStyle,
    pub status_connected: ThemeStyle,
    pub status_disconnected: ThemeStyle,
    pub status_idle: ThemeStyle,
    pub scp_label: ThemeStyle,
    pub scp_dim: ThemeStyle,
    pub box_border: ThemeStyle,
    pub box_header: ThemeStyle,
    pub box_total: ThemeStyle,
    pub box_value: ThemeStyle,
    pub box_label: ThemeStyle,
    pub modal_border: ThemeStyle,
    pub modal_active_option: ThemeStyle,
    pub modal_disabled_option: ThemeStyle,
    pub modal_help_text: ThemeStyle,
    pub modal_input: ThemeStyle,
    pub modal_success: ThemeStyle,
    pub modal_error: ThemeStyle,
    pub footer: ThemeStyle,
    pub error_banner: ThemeStyle,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ts(fg: Color) -> ThemeStyle {
    ThemeStyle { fg: Some(fg), ..ThemeStyle::default() }
}

fn ts_bg(fg: Color, bg: Color) -> ThemeStyle {
    ThemeStyle { fg: Some(fg), bg: Some(bg), ..ThemeStyle::default() }
}

fn ts_bold(fg: Color) -> ThemeStyle {
    ThemeStyle { fg: Some(fg), modifiers: Modifier::BOLD, ..ThemeStyle::default() }
}

// ---------------------------------------------------------------------------
// Default theme — matches today's hardcoded colors exactly
// ---------------------------------------------------------------------------

pub fn default_theme() -> Theme {
    Theme {
        background: Color::Reset,
        mode_badge:             ts_bg(Color::Black, Color::Cyan),
        tx_badge:               ts_bg(Color::White, Color::Red),
        dupe_badge:             ts_bg(Color::White, Color::Red),
        serial_number:          ts(Color::Yellow),
        entry_border_focused:   ts(Color::Cyan),
        entry_border_unfocused: ts(Color::DarkGray),
        field_label:            ts(Color::DarkGray),
        field_valid:            ts(Color::Green),
        field_invalid:          ts(Color::Red),
        field_unknown:          ts(Color::White),
        scp_match:              ts(Color::Green),
        cw_echo:                ts(Color::Cyan),
        frequency:              ts(Color::Yellow),
        bandmap_border:         ts(Color::DarkGray),
        bandmap_divider:        ts(Color::DarkGray),
        bandmap_worked:         ts(Color::DarkGray),
        bandmap_mult:           ts(Color::Green),
        bandmap_unworked:       ts(Color::White),
        bandmap_highlight:      ThemeStyle { modifiers: Modifier::REVERSED, ..ThemeStyle::default() },
        status_callsign_badge:  ts_bg(Color::White, Color::Blue),
        status_mult_badge:      ts_bg(Color::Black, Color::Green),
        status_qrm_badge:       ts_bg(Color::Black, Color::Yellow),
        status_clock:           ts(Color::Cyan),
        status_connected:       ts(Color::Green),
        status_disconnected:    ts(Color::Red),
        status_idle:            ts(Color::Yellow),
        scp_label:              ts(Color::Cyan),
        scp_dim:                ts(Color::DarkGray),
        box_border:             ts(Color::DarkGray),
        box_header:             ts_bold(Color::Yellow),
        box_total:              ts(Color::White),
        box_value:              ts(Color::Cyan),
        box_label:              ts(Color::White),
        modal_border:           ts(Color::Cyan),
        modal_active_option:    ts_bold(Color::Yellow),
        modal_disabled_option:  ts(Color::DarkGray),
        modal_help_text:        ts(Color::DarkGray),
        modal_input:            ts_bg(Color::White, Color::DarkGray),
        modal_success:          ts(Color::Green),
        modal_error:            ts(Color::Red),
        footer:                 ts(Color::DarkGray),
        error_banner:           ts(Color::Red),
    }
}

// ---------------------------------------------------------------------------
// Embedded palettes (sourced from ratatui-themes 0.2, MIT licensed)
// ---------------------------------------------------------------------------

struct Palette {
    accent: Color,
    secondary: Color,
    bg: Color,
    fg: Color,
    muted: Color,
    selection: Color,
    error: Color,
    warning: Color,
    success: Color,
    info: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color { Color::Rgb(r, g, b) }

fn palette_for(name: &str) -> Option<Palette> {
    Some(match name {
        "dracula" => Palette {
            accent: rgb(189,147,249), secondary: rgb(255,121,198), bg: rgb(40,42,54),
            fg: rgb(248,248,242), muted: rgb(98,114,164), selection: rgb(68,71,90),
            error: rgb(255,85,85), warning: rgb(255,184,108), success: rgb(80,250,123),
            info: rgb(139,233,253),
        },
        "one-dark-pro" => Palette {
            accent: rgb(97,175,239), secondary: rgb(198,120,221), bg: rgb(40,44,52),
            fg: rgb(171,178,191), muted: rgb(92,99,112), selection: rgb(62,68,81),
            error: rgb(224,108,117), warning: rgb(229,192,123), success: rgb(152,195,121),
            info: rgb(86,182,194),
        },
        "nord" => Palette {
            accent: rgb(136,192,208), secondary: rgb(129,161,193), bg: rgb(46,52,64),
            fg: rgb(236,239,244), muted: rgb(76,86,106), selection: rgb(67,76,94),
            error: rgb(191,97,106), warning: rgb(235,203,139), success: rgb(163,190,140),
            info: rgb(94,129,172),
        },
        "catppuccin-mocha" => Palette {
            accent: rgb(137,180,250), secondary: rgb(245,194,231), bg: rgb(30,30,46),
            fg: rgb(205,214,244), muted: rgb(108,112,134), selection: rgb(49,50,68),
            error: rgb(243,139,168), warning: rgb(249,226,175), success: rgb(166,227,161),
            info: rgb(148,226,213),
        },
        "catppuccin-latte" => Palette {
            accent: rgb(30,102,245), secondary: rgb(234,118,203), bg: rgb(239,241,245),
            fg: rgb(76,79,105), muted: rgb(140,143,161), selection: rgb(204,208,218),
            error: rgb(210,15,57), warning: rgb(223,142,29), success: rgb(64,160,43),
            info: rgb(23,146,153),
        },
        "gruvbox-dark" => Palette {
            accent: rgb(250,189,47), secondary: rgb(211,134,155), bg: rgb(40,40,40),
            fg: rgb(235,219,178), muted: rgb(146,131,116), selection: rgb(80,73,69),
            error: rgb(251,73,52), warning: rgb(254,128,25), success: rgb(184,187,38),
            info: rgb(131,165,152),
        },
        "gruvbox-light" => Palette {
            accent: rgb(181,118,20), secondary: rgb(143,63,113), bg: rgb(251,241,199),
            fg: rgb(60,56,54), muted: rgb(146,131,116), selection: rgb(213,196,161),
            error: rgb(157,0,6), warning: rgb(175,58,3), success: rgb(121,116,14),
            info: rgb(66,123,88),
        },
        "tokyo-night" => Palette {
            accent: rgb(122,162,247), secondary: rgb(187,154,247), bg: rgb(26,27,38),
            fg: rgb(192,202,245), muted: rgb(86,95,137), selection: rgb(41,46,66),
            error: rgb(247,118,142), warning: rgb(224,175,104), success: rgb(158,206,106),
            info: rgb(125,207,255),
        },
        "solarized-dark" => Palette {
            accent: rgb(38,139,210), secondary: rgb(108,113,196), bg: rgb(0,43,54),
            fg: rgb(131,148,150), muted: rgb(88,110,117), selection: rgb(7,54,66),
            error: rgb(220,50,47), warning: rgb(181,137,0), success: rgb(133,153,0),
            info: rgb(42,161,152),
        },
        "solarized-light" => Palette {
            accent: rgb(38,139,210), secondary: rgb(108,113,196), bg: rgb(253,246,227),
            fg: rgb(101,123,131), muted: rgb(147,161,161), selection: rgb(238,232,213),
            error: rgb(220,50,47), warning: rgb(181,137,0), success: rgb(133,153,0),
            info: rgb(42,161,152),
        },
        "monokai-pro" => Palette {
            accent: rgb(255,216,102), secondary: rgb(171,157,242), bg: rgb(45,42,46),
            fg: rgb(252,252,250), muted: rgb(114,113,105), selection: rgb(81,80,79),
            error: rgb(255,97,136), warning: rgb(252,152,103), success: rgb(169,220,118),
            info: rgb(120,220,232),
        },
        "rose-pine" => Palette {
            accent: rgb(235,188,186), secondary: rgb(196,167,231), bg: rgb(25,23,36),
            fg: rgb(224,222,244), muted: rgb(110,106,134), selection: rgb(38,35,58),
            error: rgb(235,111,146), warning: rgb(246,193,119), success: rgb(156,207,216),
            info: rgb(49,116,143),
        },
        "kanagawa" => Palette {
            accent: rgb(127,180,202), secondary: rgb(149,127,184), bg: rgb(31,31,40),
            fg: rgb(220,215,186), muted: rgb(84,84,109), selection: rgb(54,54,70),
            error: rgb(195,64,67), warning: rgb(255,169,107), success: rgb(118,148,106),
            info: rgb(126,156,216),
        },
        "everforest" => Palette {
            accent: rgb(131,193,120), secondary: rgb(214,153,182), bg: rgb(47,53,55),
            fg: rgb(211,198,170), muted: rgb(133,146,137), selection: rgb(68,78,79),
            error: rgb(230,126,128), warning: rgb(219,188,127), success: rgb(167,192,128),
            info: rgb(124,195,191),
        },
        "cyberpunk" => Palette {
            accent: rgb(0,255,255), secondary: rgb(255,0,255), bg: rgb(13,2,33),
            fg: rgb(240,240,240), muted: rgb(100,100,140), selection: rgb(40,20,80),
            error: rgb(255,0,60), warning: rgb(255,230,0), success: rgb(0,255,100),
            info: rgb(0,180,255),
        },
        _ => return None,
    })
}

const THEME_NAMES: &[&str] = &[
    "dracula", "one-dark-pro", "nord", "catppuccin-mocha", "catppuccin-latte",
    "gruvbox-dark", "gruvbox-light", "tokyo-night", "solarized-dark",
    "solarized-light", "monokai-pro", "rose-pine", "kanagawa", "everforest",
    "cyberpunk",
];

fn from_palette(p: &Palette) -> Theme {
    let d = ThemeStyle::default();
    Theme {
        background: p.bg,
        mode_badge:             ThemeStyle { fg: Some(p.bg), bg: Some(p.secondary), ..d },
        tx_badge:               ThemeStyle { fg: Some(p.bg), bg: Some(p.error), ..d },
        dupe_badge:             ThemeStyle { fg: Some(p.bg), bg: Some(p.error), ..d },
        serial_number:          ThemeStyle { fg: Some(p.warning), ..d },
        entry_border_focused:   ThemeStyle { fg: Some(p.accent), ..d },
        entry_border_unfocused: ThemeStyle { fg: Some(p.muted), ..d },
        field_label:            ThemeStyle { fg: Some(p.muted), ..d },
        field_valid:            ThemeStyle { fg: Some(p.success), ..d },
        field_invalid:          ThemeStyle { fg: Some(p.error), ..d },
        field_unknown:          ThemeStyle { fg: Some(p.fg), ..d },
        scp_match:              ThemeStyle { fg: Some(p.success), ..d },
        cw_echo:                ThemeStyle { fg: Some(p.info), ..d },
        frequency:              ThemeStyle { fg: Some(p.warning), ..d },
        bandmap_border:         ThemeStyle { fg: Some(p.muted), ..d },
        bandmap_divider:        ThemeStyle { fg: Some(p.muted), ..d },
        bandmap_worked:         ThemeStyle { fg: Some(p.muted), ..d },
        bandmap_mult:           ThemeStyle { fg: Some(p.success), ..d },
        bandmap_unworked:       ThemeStyle { fg: Some(p.fg), ..d },
        bandmap_highlight:      ThemeStyle { modifiers: Modifier::REVERSED, ..d },
        status_callsign_badge:  ThemeStyle { fg: Some(p.bg), bg: Some(p.accent), ..d },
        status_mult_badge:      ThemeStyle { fg: Some(p.bg), bg: Some(p.success), ..d },
        status_qrm_badge:       ThemeStyle { fg: Some(p.bg), bg: Some(p.warning), ..d },
        status_clock:           ThemeStyle { fg: Some(p.info), ..d },
        status_connected:       ThemeStyle { fg: Some(p.success), ..d },
        status_disconnected:    ThemeStyle { fg: Some(p.error), ..d },
        status_idle:            ThemeStyle { fg: Some(p.warning), ..d },
        scp_label:              ThemeStyle { fg: Some(p.info), ..d },
        scp_dim:                ThemeStyle { fg: Some(p.muted), ..d },
        box_border:             ThemeStyle { fg: Some(p.muted), ..d },
        box_header:             ThemeStyle { fg: Some(p.accent), modifiers: Modifier::BOLD, ..d },
        box_total:              ThemeStyle { fg: Some(p.fg), ..d },
        box_value:              ThemeStyle { fg: Some(p.info), ..d },
        box_label:              ThemeStyle { fg: Some(p.fg), ..d },
        modal_border:           ThemeStyle { fg: Some(p.accent), ..d },
        modal_active_option:    ThemeStyle { fg: Some(p.warning), modifiers: Modifier::BOLD, ..d },
        modal_disabled_option:  ThemeStyle { fg: Some(p.muted), ..d },
        modal_help_text:        ThemeStyle { fg: Some(p.muted), ..d },
        modal_input:            ThemeStyle { fg: Some(p.fg), bg: Some(p.selection), ..d },
        modal_success:          ThemeStyle { fg: Some(p.success), ..d },
        modal_error:            ThemeStyle { fg: Some(p.error), ..d },
        footer:                 ThemeStyle { fg: Some(p.muted), ..d },
        error_banner:           ThemeStyle { fg: Some(p.error), ..d },
    }
}

// ---------------------------------------------------------------------------
// Public loader
// ---------------------------------------------------------------------------

pub fn load(theme_name: Option<&str>, theme_file: Option<&Path>) -> anyhow::Result<Theme> {
    let mut theme = match theme_name {
        None | Some("default") => default_theme(),
        Some(name) => {
            let palette = palette_for(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown theme \"{name}\". Valid names: default, {}",
                    THEME_NAMES.join(", ")
                )
            })?;
            from_palette(&palette)
        }
    };

    if let Some(path) = theme_file {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading theme_file {}", path.display()))?;
        let overlay: ThemeOverlay = toml::from_str(&raw)
            .with_context(|| format!("parsing theme_file {}", path.display()))?;
        overlay.apply(&mut theme)?;
    }

    Ok(theme)
}

// ---------------------------------------------------------------------------
// Hex color parsing
// ---------------------------------------------------------------------------

fn parse_hex_color(s: &str) -> anyhow::Result<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        bail!("color must be #rrggbb, got \"{s}\"");
    }
    let r = u8::from_str_radix(&hex[0..2], 16)
        .with_context(|| format!("bad red in \"{s}\""))?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .with_context(|| format!("bad green in \"{s}\""))?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .with_context(|| format!("bad blue in \"{s}\""))?;
    Ok(Color::Rgb(r, g, b))
}

// ---------------------------------------------------------------------------
// TOML overlay (custom theme_file)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeOverlay {
    #[allow(dead_code)]
    name: Option<String>,
    background: Option<String>,
    #[serde(default)]
    entry: OverlaySection,
    #[serde(default)]
    bandmap: OverlaySection,
    #[serde(default)]
    status: OverlaySection,
    #[serde(rename = "box", default)]
    box_section: OverlaySection,
    #[serde(default)]
    modal: OverlaySection,
    #[serde(default)]
    global: OverlaySection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OverlaySection {
    #[serde(flatten)]
    roles: std::collections::BTreeMap<String, StyleOverlay>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleOverlay {
    fg: Option<String>,
    bg: Option<String>,
    #[serde(default)]
    modifiers: Vec<String>,
}

impl StyleOverlay {
    fn apply_to(&self, target: &mut ThemeStyle) -> anyhow::Result<()> {
        if let Some(ref fg) = self.fg { target.fg = Some(parse_hex_color(fg)?); }
        if let Some(ref bg) = self.bg { target.bg = Some(parse_hex_color(bg)?); }
        for m in &self.modifiers {
            match m.to_ascii_lowercase().as_str() {
                "bold" => target.modifiers |= Modifier::BOLD,
                "italic" => target.modifiers |= Modifier::ITALIC,
                "reversed" => target.modifiers |= Modifier::REVERSED,
                "underlined" => target.modifiers |= Modifier::UNDERLINED,
                "dim" => target.modifiers |= Modifier::DIM,
                other => bail!("unknown modifier \"{other}\""),
            }
        }
        Ok(())
    }
}

macro_rules! apply_section {
    ($roles:expr, $theme:expr, $section:literal, { $($key:literal => $field:ident),* $(,)? }) => {
        for (key, style) in $roles {
            let target = match key.as_str() {
                $($key => &mut $theme.$field,)*
                _ => bail!(concat!("[", $section, "] unknown role \"{}\""), key),
            };
            style.apply_to(target).with_context(|| format!(concat!("[", $section, ".{}]"), key))?;
        }
    };
}

impl ThemeOverlay {
    fn apply(&self, theme: &mut Theme) -> anyhow::Result<()> {
        apply_section!(&self.entry.roles, theme, "entry", {
            "mode_badge" => mode_badge, "tx_badge" => tx_badge,
            "dupe_badge" => dupe_badge, "serial_number" => serial_number,
            "border_focused" => entry_border_focused, "border_unfocused" => entry_border_unfocused,
            "field_label" => field_label, "field_valid" => field_valid,
            "field_invalid" => field_invalid, "field_unknown" => field_unknown,
            "scp_match" => scp_match, "cw_echo" => cw_echo, "frequency" => frequency,
        });
        apply_section!(&self.bandmap.roles, theme, "bandmap", {
            "border" => bandmap_border, "divider" => bandmap_divider,
            "worked" => bandmap_worked, "mult" => bandmap_mult,
            "unworked" => bandmap_unworked, "highlight" => bandmap_highlight,
        });
        apply_section!(&self.status.roles, theme, "status", {
            "callsign_badge" => status_callsign_badge, "mult_badge" => status_mult_badge,
            "qrm_badge" => status_qrm_badge, "clock" => status_clock,
            "connected" => status_connected, "disconnected" => status_disconnected,
            "idle" => status_idle, "scp_label" => scp_label, "scp_dim" => scp_dim,
        });
        apply_section!(&self.box_section.roles, theme, "box", {
            "border" => box_border, "header" => box_header,
            "total" => box_total, "value" => box_value, "label" => box_label,
        });
        apply_section!(&self.modal.roles, theme, "modal", {
            "border" => modal_border, "active_option" => modal_active_option,
            "disabled_option" => modal_disabled_option, "help_text" => modal_help_text,
            "input" => modal_input, "success" => modal_success, "error" => modal_error,
        });
        apply_section!(&self.global.roles, theme, "global", {
            "footer" => footer, "error_banner" => error_banner,
        });
        if let Some(ref bg) = self.background {
            theme.background = parse_hex_color(bg)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_valid() {
        assert_eq!(parse_hex_color("#000000").unwrap(), Color::Rgb(0, 0, 0));
        assert_eq!(parse_hex_color("#ffffff").unwrap(), Color::Rgb(255, 255, 255));
        assert_eq!(parse_hex_color("#FF8800").unwrap(), Color::Rgb(255, 136, 0));
        assert_eq!(parse_hex_color("aabbcc").unwrap(), Color::Rgb(170, 187, 204));
    }

    #[test]
    fn parse_hex_invalid() {
        assert!(parse_hex_color("#gg0000").is_err());
        assert!(parse_hex_color("#fff").is_err());
        assert!(parse_hex_color("").is_err());
    }

    #[test]
    fn all_builtin_palettes_produce_valid_theme() {
        for name in THEME_NAMES {
            let p = palette_for(name).unwrap_or_else(|| panic!("missing palette: {name}"));
            let t = from_palette(&p);
            assert!(t.status_callsign_badge.fg.is_some(), "{name}: badge fg");
            assert!(t.dupe_badge.bg.is_some(), "{name}: dupe bg");
        }
    }

    #[test]
    fn default_theme_matches_hardcoded_colors() {
        let t = default_theme();
        assert_eq!(t.mode_badge.fg, Some(Color::Black));
        assert_eq!(t.mode_badge.bg, Some(Color::Cyan));
        assert_eq!(t.dupe_badge.fg, Some(Color::White));
        assert_eq!(t.dupe_badge.bg, Some(Color::Red));
        assert_eq!(t.status_callsign_badge.fg, Some(Color::White));
        assert_eq!(t.status_callsign_badge.bg, Some(Color::Blue));
        assert_eq!(t.field_valid.fg, Some(Color::Green));
        assert_eq!(t.field_invalid.fg, Some(Color::Red));
        assert_eq!(t.bandmap_mult.fg, Some(Color::Green));
        assert_eq!(t.footer.fg, Some(Color::DarkGray));
        assert_eq!(t.status_clock.fg, Some(Color::Cyan));
    }

    #[test]
    fn load_default_when_no_config() {
        let theme = load(None, None).unwrap();
        let d = default_theme();
        assert_eq!(theme.mode_badge.fg, d.mode_badge.fg);
    }

    #[test]
    fn load_named_theme() {
        let theme = load(Some("dracula"), None).unwrap();
        assert!(theme.status_callsign_badge.fg.is_some());
    }

    #[test]
    fn unknown_theme_name_errors() {
        let err = load(Some("nonexistent"), None).unwrap_err();
        assert!(err.to_string().contains("unknown theme"));
        assert!(err.to_string().contains("Valid names"));
    }

    #[test]
    fn toml_overlay_partial_override() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[status]\nclock = { fg = \"#ff0000\" }\n").unwrap();
        let mut theme = default_theme();
        let original = theme.status_callsign_badge;
        let raw = std::fs::read_to_string(&path).unwrap();
        let overlay: ThemeOverlay = toml::from_str(&raw).unwrap();
        overlay.apply(&mut theme).unwrap();
        assert_eq!(theme.status_clock.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(theme.status_callsign_badge.fg, original.fg);
    }

    #[test]
    fn toml_overlay_bad_hex_errors() {
        let toml_str = "[entry]\nmode_badge = { fg = \"#zzzzzz\" }\n";
        let overlay: ThemeOverlay = toml::from_str(toml_str).unwrap();
        let mut theme = default_theme();
        let err = overlay.apply(&mut theme).unwrap_err();
        assert!(err.to_string().contains("entry.mode_badge"));
    }

    #[test]
    fn toml_overlay_unknown_role_errors() {
        let toml_str = "[bandmap]\nnonexistent = { fg = \"#ffffff\" }\n";
        let overlay: ThemeOverlay = toml::from_str(toml_str).unwrap();
        let mut theme = default_theme();
        let err = overlay.apply(&mut theme).unwrap_err();
        assert!(err.to_string().contains("unknown role"));
    }

    #[test]
    fn theme_style_into_ratatui_style() {
        let ts = ThemeStyle {
            fg: Some(Color::Red),
            bg: Some(Color::Blue),
            modifiers: Modifier::BOLD,
        };
        let s: Style = ts.into();
        assert_eq!(s, Style::default().fg(Color::Red).bg(Color::Blue).add_modifier(Modifier::BOLD));
    }
}
