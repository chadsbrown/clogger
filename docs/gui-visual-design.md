# GUI visual design: rationale and rules

This document records **why** `logger-gui` looks the way it does, so later
changes can be judged against the same reasoning rather than taste of the
day. It came out of a full UI review in November 2026 (branch
`gui-visual-refresh`). The "before" state is pictured in
`docs/clogger-gui-20260425.png`.

## The problem

The GUI worked but felt dated and noisy. The review found that this came
from a handful of systemic issues, not from any single pane:

1. **The accent color was everywhere.** `primary.strong` filled every
   visible toolbar pill (about 13), the focused title bar, all 12 macro
   keycaps, the newest log row, the emphasized rate tile and the audible
   SO2R card. Since everything was highlighted, nothing stood out, and the
   cues that matter (focused field, active radio, TX) competed with chrome.
2. **Card soup.** Every log row (up to 512), condx band row, macro row and
   available row was its own bordered card inside a bordered pane. That
   meant borders inside borders and a lot of visual noise for tabular data.
3. **Tokens existed but were half used.** `style.rs` had `TEXT_*` and
   `RADIUS_*` constants, but the status bar, toolbar, help modal and entry
   pane used literals. There was no spacing scale: the code had 22
   distinct padding values.
4. **Duplicated widgets drifted apart.** There were about 8 badge or chip
   implementations with different paddings and radii. The modal frame and
   close button were copy-pasted three times. Table header chains were
   repeated 5–8 times per pane.
5. **Theme fidelity.** Muted grays, accent text (a fixed blue), the
   new-spot green, the bandmap axis and the whole help modal used
   hardcoded RGB values. On Nord, Gruvbox or Dracula they looked off-tone,
   for example blue accent text next to a non-blue primary.
6. **Typography.** The app used the system default sans and mono fonts,
   so it looked different on every machine, and had no weight hierarchy.
   The smallest muted text was 10px, which was hard to read on dark themes.
   Icons mixed a color emoji (🎨) with assorted Unicode glyphs.
7. **Small correctness and content issues.**
   - The help modal described a theme toggle that no longer existed.
   - The help modal omitted `=`, `` ` ``, Space and ←/→, and called Esc
     "clear entry" when it actually aborts CW.
   - The log index column overflowed at 4-digit QSO numbers.
   - The claimed score was drawn at body size.
   - Empty states were inconsistent.

## Principles

These are the rules to keep following.

### 1. Neutral chrome, restrained accent

Accent (the theme's primary) is **reserved** for:

- keyboard focus: the focused entry field and its caret, and the focused
  pane's border, title text and faint title-bar tint
- the active radio, its badge and the audible SO2R radio
- the newest QSO in the log (accent bar)
- the one headline metric per pane (10-minute rate, claimed score)
- spots, as accent call text in the DX feed

Chrome (toolbar, title bars, status bar, macro keys) is neutral. A
visible pane in the toolbar is shown by text weight plus a 2px underline,
not a filled pill.

**Test for new UI:** if adding accent to something wouldn't change what
an operator does next, don't add it.

### 2. Semantic colors are fixed in meaning

| Meaning | Palette slot | Where |
|---|---|---|
| DUPE, TX, VOID, errors | `danger` | solid badge or border |
| QRM | `danger` | soft badge |
| MULT | `warning` | solid badge (entry), warning text or badge elsewhere |
| Rig cursor | `primary` | bandmap line and pointer |
| Connected / good | `success` | status dots, condx "good" |

Operators learn these meanings, so they must not move between releases.
Only the **alerts that demand action** (DUPE, MULT, TX, VOID) use
**solid** badges. Everything else uses **soft** badges (tinted background,
tone-colored text).

### 3. Flat tables, tiles only for metrics

Tabular data (log, score, available, dxfeed, condx bands) uses
`widgets::table_row`, a flat row with an alternating stripe. A highlighted
row gets a 2px left bar plus a faint tint. Every row reserves the bar's
gutter, so highlighted and plain rows stay aligned.

Raised tiles (`widgets::tile`) are for grouped metrics only (rate, perf,
the condx stat grid, SO2R radio cards). To single out a tile, give it a
left accent bar. Don't fill it with primary.

Log stripes are keyed on the **absolute QSO index**, so a row keeps its
shade as new QSOs arrive and the table doesn't flicker on each log.

### 4. All neutrals come from the palette

`muted_color`, `very_muted_color`, `border_color` and the `surface_*`
colors are blends of the theme's own background and text colors. That way
each theme's tint carries through: Gruvbox stays warm and Nord stays cool.

Contrast is enforced in code, not assumed:

- `accent_text_color` and `danger_text_color` are nudged until they reach
  4.5:1 against the pane surface (WCAG AA for body text).
- `muted_color` is held at 3:1 or better, and `very_muted_color` at 2:1.
  Solarized Light needed this, because its own body text is low-contrast.
- `mult_color` and `new_spot_color` are held at 3:1, since raw warning
  yellow is unreadable on light themes.

A unit test (`style::tests::ensure_contrast_reaches_target_on_every_builtin`)
checks the accent and muted guarantees against **every** iced built-in
theme.

### 5. Elevation, not borders, separates layers

There are four surface levels, in order:

| Level | Function | Used for |
|---|---|---|
| canvas | `surface_canvas` | workspace behind the panes, a step darker than the panes |
| pane | `surface_pane` | panes, toolbar, status bar, modals |
| raised | `surface_raised` | tiles, keycaps, the active radio card |
| sunken | `surface_sunken` | entry value boxes and text inputs |

Every pane has a soft resting shadow. The focused pane and modals get a
deeper one. The workspace used to be `background.weak`, which in dark
themes is *lighter* than the panes, so panes read as holes. Now it is
darker, so panes sit on top.

### 6. One type system, bundled

- **Inter** for UI and **JetBrains Mono** for data (calls, frequencies,
  exchanges, every number). Monospace digits stop counters and columns
  from jittering as values change.
- Both are **bundled** and subset to about 400 KB (see
  `logger-gui/assets/fonts/README.md`), so the look doesn't depend on the
  host's fontconfig. The decision was made with the user, accepting the
  binary-size cost.
- The scale is 11 / 12 / 13 / 15 / 16 / 28, with **nothing under 11px**.
- Hierarchy comes from **weight** (semibold, uppercase labels) more than
  from color, so labels separate from data even in low-contrast themes.
- **Icons are Lucide**, bundled and subset. Don't use emoji, because color
  emoji rendering varies by platform.

### 7. One implementation per visual idea

`panes/widgets.rs` is the only place that draws badges, status dots,
keycaps, section labels, table headers and rows, tiles, stats, empty
states, buttons and modal frames. When something needs to look like an
existing element, call the existing function. If it needs to look
different, change the function, which changes every caller together.

### 8. Stable geometry in the entry pane

The entry pane's echo and status rows have fixed heights, so badges
appearing or clearing never shift the R2 card. The caret is a 2px bar
rather than a `|` glyph, because the glyph took up a character cell and
shifted the text as the caret moved. Field widths come from
`field.width × 9.6px`, measured against JetBrains Mono's 0.6em advance
at 15px.

### 9. Nothing paints over the pane frame

A pane's content is inset by the frame's border width, and inner shapes
that touch the frame use the inner radius (`RADIUS_MD − FRAME_BORDER`).
Full-bleed children must not paint a square background into the rounded
corners; the bandmap canvas leaves its background unpainted for this
reason. An early version of this redesign broke both rules: a 2px focus
stripe couldn't hold the 5px corner radius, and a full-width hairline
notched the accent border.

## What changed, file by file

| File | Change and reason |
|---|---|
| `panes/style.rs` | New spacing, padding, radius, type and font tokens. Palette-derived neutrals and surfaces, contrast helpers, elevation tokens and the `Icon` enum. Dead helpers (`card_style`, `badge_style`, `dupe_pair`, …) removed. |
| `panes/widgets.rs` (new) | Shared components (principle 7). |
| `mdi/workspace.rs` | Toolbar uses ghost tabs with an underline. The title bar grew from 20 to 26px with an uppercase semibold title. Focus shows as a faint accent tint on the title bar, accent text, accent border and deeper shadow instead of a full primary fill. Frame content is inset by the border width (`FRAME_BORDER`), so nothing inside can paint over the border. The close button is a ghost Lucide X that turns red on hover. The workspace sits on `surface_canvas`. |
| `main.rs` | The status bar grew from 24 to 30px in three zones (identity, centered UTC clock, connections plus icon buttons). Connection pills became status dots. The error banner uses Lucide icons. Fonts are registered with `.font()` and `.default_font()`. |
| `panes/entry.rs` | Frequency is promoted to 15px mono body text. The active radio gets a raised accent-bordered card, and a transmitting radio gets a danger border plus a solid TX badge. Bar caret, sunken value boxes and uppercase field labels (accent when focused). DUPE/MULT are solid badges, QRM soft. The CW echo is drawn in accent text. |
| `panes/log.rs` | Flat striped table. The newest QSO gets an accent bar, a void QSO gets danger call text plus a VOID badge. The index column is wide enough for 4 digits. Column math accounts for the row chrome. Headers read Rcvd/Sent instead of RX/TX, which was ambiguous next to the SO2R RX/TX terms. |
| `panes/score.rs` | The claimed score is a headline stat at 28px with thousands separators. Flat table with a hairline above the totals row. |
| `panes/rate.rs`, `panes/perf.rs` | Shared tiles. The headline tile gets an accent bar instead of a primary fill. The perf progress bars are themed. |
| `panes/available.rs` | Calls and Mults as headline stats, then a flat table. Leaders get an accent bar, which replaces the accent border plus redundant Q/M badges. |
| `panes/macros.rs` | Neutral keycaps instead of 12 accent-filled badges. |
| `panes/condx.rs` | Solar stats as a 3×2 grid on one tile. Band conditions as a flat table. Ratings use a traffic-light scale (good is success, fair is warning, poor is danger), where poor used to look like "no data". |
| `panes/so2r.rs` | Accent bar on the audible radio's card instead of a full fill, which in stereo made both cards bright. The backtick hint is drawn as a keycap. |
| `panes/scp.rs` | Uppercase section labels. Soft accent chips for exact matches, neutral chips for off-by-one, solid accent for the selected match. The `=` hint is drawn as a keycap. |
| `panes/dxfeed.rs` | Uses the shared table, with SPOT/DROP as soft badges. The row-height estimate is updated to the new metrics. |
| `panes/bandmap.rs` | Axis color comes from the theme. Labels use bundled mono at token sizes. Spot dots are circles. The cursor line is 2px with a pointer on the axis. The zoom level is drawn as a chip. |
| `modals/*` | All three modals use `widgets::modal_frame`: a 10px radius, shadow, a header with a hairline and a footer row. Help is grouped by task with keycaps, and its stale and missing entries are fixed. Theme tiles add a warning swatch and a check on the current theme. Export uses selectable format rows with hints, a themed mono path input and Primary/Secondary buttons. |

## Deliberate exceptions to "no literals"

A few literals remain on purpose because they are geometry, not style:
pill radii equal to half of a 2px bar, dot or swatch; 1–3px button insets
that make a border-as-background effect; and the 0.55 modal backdrop
alpha. Keep any new exception tied to its own geometry in the same way.

## Not changed

- MDI behavior, layout persistence format and keybindings.
- The TUI (`logger-tui`), which has its own theme system
  (`plans/tui-theme-system.md`).

## Verifying a visual change

1. Run `cargo test -p logger-gui`. The contrast test covers every
   built-in theme.
2. Run
   `grep -rnE '\.size\([0-9]|Color::from_rgb|Font::MONOSPACE' logger-gui/src`.
   The only hits should be in `style.rs`, plus the exceptions above.
3. Run the app and step through at least Dark, Light, Nord, Gruvbox Dark,
   Solarized Light and Catppuccin Latte in the theme picker. Check that
   accent appears only where principle 1 allows, that muted text is
   legible, and that no glyph renders as a box.
