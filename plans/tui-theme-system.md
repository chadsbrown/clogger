# TUI theme system

## Context

The clogger TUI uses ratatui with ~50 hardcoded `Color::Foo` literals
scattered across ~10 widget files in `logger-tui/src/ui/`. Colors that
look fine on a black-background terminal (Color::White, Color::DarkGray)
read poorly on a Solarized Light or Solarized Dark terminal — the
operator can't easily switch palettes between sessions. There's no
existing color constants module.

This plan introduces a theme system that:
- Defines ~25-30 **semantic role** keys (e.g., `dupe_badge`,
  `bandmap_mult`, `entry_focused_border`) rather than per-widget
  overrides, so themes are short and easy to author.
- Ships **built-in themes** (default = current colors, solarized-dark,
  solarized-light, monochrome) baked into the binary, selected by name
  in config.
- Allows users to provide a **custom theme.toml** for full bespoke
  themes.
- Uses **24-bit hex strings only** (`#rrggbb`) for color values.
  Truecolor is supported by every actively-developed terminal; ratatui
  auto-downgrades to the nearest terminal-supported color when needed.

## Design

### New module: `logger-tui/src/theme.rs`

Defines the `Theme` struct, the `ThemeStyle` building block, hex
parsing, default theme constants, and the loader.

```rust
pub struct Theme {
    pub name: String,
    // Entry line
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
    // Bandmap
    pub bandmap_border: ThemeStyle,
    pub bandmap_divider: ThemeStyle,
    pub bandmap_worked: ThemeStyle,
    pub bandmap_mult: ThemeStyle,
    pub bandmap_unworked: ThemeStyle,
    pub bandmap_highlight: ThemeStyle,  // typically uses modifiers
    // Status bar
    pub status_callsign_badge: ThemeStyle,
    pub status_mult_badge: ThemeStyle,
    pub status_qrm_badge: ThemeStyle,
    pub status_connected: ThemeStyle,
    pub status_disconnected: ThemeStyle,
    pub status_idle: ThemeStyle,
    pub scp_label: ThemeStyle,
    pub scp_dim: ThemeStyle,
    // Generic boxes (score, avail, rate, log_tail share these)
    pub box_border: ThemeStyle,
    pub box_header: ThemeStyle,
    pub box_total: ThemeStyle,
    pub box_value: ThemeStyle,
    // Modal
    pub modal_border: ThemeStyle,
    pub modal_active_option: ThemeStyle,
    pub modal_disabled_option: ThemeStyle,
    pub modal_help_text: ThemeStyle,
    pub modal_input: ThemeStyle,
    pub modal_success: ThemeStyle,
    pub modal_error: ThemeStyle,
    // Footer / global
    pub footer: ThemeStyle,
    pub error_banner: ThemeStyle,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ThemeStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub modifiers: Modifier,  // bitflags from ratatui::style::Modifier
}

impl From<ThemeStyle> for ratatui::style::Style { ... }
```

`ThemeStyle` carries fg + bg + modifiers because some roles need all
three together (e.g., `modal_active_option` = yellow + bold,
`bandmap_highlight` = reversed only).

### TOML format

```toml
# theme.toml
name = "Solarized Dark"

[entry]
mode_badge        = { fg = "#002b36", bg = "#2aa198" }
tx_badge          = { fg = "#002b36", bg = "#dc322f" }
dupe_badge        = { fg = "#002b36", bg = "#dc322f" }
serial_number     = { fg = "#b58900" }
border_focused    = { fg = "#2aa198" }
border_unfocused  = { fg = "#586e75" }
field_label       = { fg = "#586e75" }
field_valid       = { fg = "#859900" }
field_invalid     = { fg = "#dc322f" }
field_unknown     = { fg = "#839496" }
scp_match         = { fg = "#859900" }
cw_echo           = { fg = "#2aa198" }
frequency         = { fg = "#b58900" }

[bandmap]
border    = { fg = "#586e75" }
divider   = { fg = "#586e75" }
worked    = { fg = "#586e75" }
mult      = { fg = "#859900" }
unworked  = { fg = "#839496" }
highlight = { modifiers = ["reversed"] }

[status]
callsign_badge   = { fg = "#fdf6e3", bg = "#268bd2" }
mult_badge       = { fg = "#002b36", bg = "#859900" }
qrm_badge        = { fg = "#002b36", bg = "#b58900" }
connected        = { fg = "#859900" }
disconnected     = { fg = "#dc322f" }
idle             = { fg = "#b58900" }
scp_label        = { fg = "#2aa198" }
scp_dim          = { fg = "#586e75" }

[box]
border  = { fg = "#586e75" }
header  = { fg = "#b58900", modifiers = ["bold"] }
total   = { fg = "#fdf6e3" }
value   = { fg = "#2aa198" }

[modal]
border           = { fg = "#2aa198" }
active_option    = { fg = "#b58900", modifiers = ["bold"] }
disabled_option  = { fg = "#586e75" }
help_text        = { fg = "#586e75" }
input            = { fg = "#fdf6e3", bg = "#586e75" }
success          = { fg = "#859900" }
error            = { fg = "#dc322f" }

[global]
footer       = { fg = "#586e75" }
error_banner = { fg = "#dc322f" }
```

Top-level field is `name = "..."`. Each section maps directly to the
struct grouping. Missing keys fall back to the default theme's value
(graceful — a partial theme works).

### Color parsing

Hex strings only: `#rrggbb` → `Color::Rgb(r, g, b)`. Case-insensitive,
strip leading `#`, parse three pairs. Reject anything else with a clear
error. ~30 lines of code; no external crate needed.

### Built-in themes

In `logger-tui/src/theme/builtin/`:
- `default.toml` — current literal colors, byte-for-byte identical to
  today (regression safety net).
- `solarized-dark.toml` — Solarized Dark, full canonical palette.
- `solarized-light.toml` — Solarized Light.
- `monochrome.toml` — black/white/grays only for very low-color or
  high-contrast preferences.

Embedded via `include_str!`. Built-in registry:
```rust
const BUILTIN_THEMES: &[(&str, &str)] = &[
    ("default",         include_str!("theme/builtin/default.toml")),
    ("solarized-dark",  include_str!("theme/builtin/solarized-dark.toml")),
    ("solarized-light", include_str!("theme/builtin/solarized-light.toml")),
    ("monochrome",      include_str!("theme/builtin/monochrome.toml")),
];
```

### Config integration

In `logger-tui/src/config.rs` `Config` struct (around line 117), add:

```rust
pub theme: Option<String>,        // built-in theme name
pub theme_file: Option<PathBuf>,  // path to custom theme.toml
```

Resolution order (in `theme::load`):
1. If `theme_file` set → load that file, validate, return.
2. Else if `theme` set → look up in `BUILTIN_THEMES`, error if unknown.
3. Else → `default` built-in.

For partial themes (custom or built-in), missing roles fall back to
`default`'s values — so a user who only wants to override a few colors
can write a 5-line theme.toml.

### Plumbing into widgets

Add `theme: Theme` field to `TuiState` (`logger-tui/src/main.rs:46`
area). Initialize in `event_loop::run` startup (around line 73)
alongside other `TuiState` fields, by calling `theme::load(&config)`.

`TuiState` is already passed to `ui::render` and forwarded to every
widget — no signature changes to widget functions.

Migrate every widget to use `tui.theme.<role>.into()` instead of
inline `Style::default().fg(Color::X)`. Concrete sites to migrate:

- `logger-tui/src/ui/entry_line.rs` — lines 34, 42, 50, 55, 75, 80-82,
  100, 120, 133, 142
- `logger-tui/src/ui/bandmap.rs` — lines 43, 62, 64, 66, 68, 105
- `logger-tui/src/ui/status_bar.rs` — lines 18, 26, 35, 53, 77-79, 105,
  106
- `logger-tui/src/ui/log_tail.rs` — lines 38-39, 57
- `logger-tui/src/ui/score_box.rs` — lines 14, 26, 48, 58
- `logger-tui/src/ui/avail_box.rs` — lines 15, 36, 47
- `logger-tui/src/ui/rate_box.rs` — lines 14, 25/28, 32/35, 39/42
- `logger-tui/src/ui/export_modal.rs` — lines 20, 147-149, 158-159, 166,
  194-196, 207, 209, 226, 234
- `logger-tui/src/ui/mod.rs` — lines 82, 136

### Files modified

**New**:
- `logger-tui/src/theme.rs` (or `theme/mod.rs` if split)
- `logger-tui/src/theme/builtin/default.toml`
- `logger-tui/src/theme/builtin/solarized-dark.toml`
- `logger-tui/src/theme/builtin/solarized-light.toml`
- `logger-tui/src/theme/builtin/monochrome.toml`

**Modified**:
- `logger-tui/src/main.rs` — `mod theme;`, add `theme: Theme` to
  `TuiState`, initialize at startup
- `logger-tui/src/config.rs` — add `theme` and `theme_file` fields
- `logger-tui/src/event_loop.rs` — load theme during `TuiState`
  construction
- All ten widget files listed above — replace inline color literals
  with `theme` lookups
- `config.example.toml` — document the two new options
- `README.md` — short note pointing at theme system

### Tests

- Unit test `theme::parse_hex_color`: valid hex, invalid hex, edge
  cases (`#000000`, `#ffffff`, lowercase, uppercase, with/without
  leading `#`).
- Unit test built-in theme loading: each of the four shipped themes
  parses without error and produces a complete `Theme`.
- Unit test partial-theme fallback: a TOML with only `[entry]
  tx_badge` defined yields a `Theme` where `tx_badge` is overridden
  and all other fields equal the `default` theme's values.
- Default-theme regression: assert byte-for-byte that the `default`
  theme's `Theme` struct produces the exact same colors that are
  hardcoded today (catches accidental drift during migration).

### Effort estimate

Moderate, ~1 day:
- Theme module + hex parser + serde derive: ~200 lines
- Built-in theme files: 4 × ~40 lines TOML
- Widget migration: 10 files, mechanical find-replace style edits,
  ~30 minutes
- Tests: ~100 lines
- Docs: small

The widget migration is the largest line-count change but mechanically
straightforward. Risk is low because the `default` theme regression
test catches any accidental visual changes.

### Out of scope

- **Live theme reload**: not in this plan. Would require a config-watch
  mechanism. Operator can restart clogger to swap themes.
- **In-app theme picker UI**: not in this plan. Theme is selected via
  config file only.
- **Per-contest themes**: theme lives in stable config, not contest
  config. If desired later, trivial to move or layer.
- **Background color of the entire terminal**: not themed — that's the
  terminal emulator's job. Widgets that need a background (badges,
  modal input) get their own bg via `ThemeStyle`.
- **Modifier vocabulary expansion**: only ratatui's existing
  `Modifier` flags are accepted. No new visual effects added.

## Verification

1. `cargo build -p logger-tui` — clean build with no warnings related
   to the new module.
2. `cargo test -p logger-tui` — all theme tests pass; the default-theme
   regression test confirms zero visual change.
3. Manual: launch with no theme config → identical to today.
4. Manual: add `theme = "solarized-dark"` to config, launch in a
   black-background terminal — should look like a Solarized Dark
   palette.
5. Manual: launch the same theme in a Solarized Light terminal —
   colors should still render correctly because they're absolute hex
   values, not theme-of-theme.
6. Manual: write a tiny `~/my-theme.toml` overriding just one role,
   point `theme_file = "~/my-theme.toml"` — that one role changes,
   everything else falls back to default.
7. Manual: introduce a typo in a hex value (e.g., `"#zzzzzz"`) — the
   loader should refuse to start with a clear error pointing at the
   bad role and value.
