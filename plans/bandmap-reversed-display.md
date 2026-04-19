# Optional reversed bandmap display (high-freq at top)

## Context

Today both bandmap panels render spots sorted **ascending by freq** — the lowest-freq spot sits at the top of the panel, highest at the bottom. Some operators prefer the opposite visual (highest-freq at top). Request: make it a config option with the current behavior as the default.

## Why this is bounded (answering "is it a big change?")

Not big. Two wrinkles make it more than a one-liner:

1. **The canonical sort must stay ascending.** `filtered_bandmap_spots` at `logger-core/src/contest/mod.rs:68` sorts by freq and is consumed by both the reducer and the renderer. `snap_bandmap_cursor_to_freq` (`logger-core/src/reducer.rs:720`) calls `spots.partition_point(|s| s.freq_hz < target)`, which requires ascending order for correctness. Flipping the sort itself would silently break freq-based anchoring.

2. **The flip must happen at both the render layer and the nav-step direction.** When the bandmap displays highest-freq at top, the operator still expects Ctrl-↓ to mean "visually down" (= lower freq in the reversed layout). That's a negation of the step direction in the reducer.

Everything else (config plumbing, AppState field) follows the exact pattern used for `show_passband_qrm` and the recently-added `bandmap_skip_worked`.

Estimate: ~50–80 lines total across 5 files + ~3 tests.

## Scope

- **Off by default** (preserves current operators' muscle memory).
- Applies equally to R1 and R2 bandmaps.
- `snap_bandmap_cursor_to_freq` stays unchanged — freq-based anchoring still works on the ascending internal slice.
- Dual-bandmap mode (R1 stacked above R2) is not affected by this flag — it's a spatial layout choice, not a freq-orientation choice. Each panel renders its own contents reversed.

## Design

### New AppState field

- **`logger-core/src/state.rs`** — add `pub bandmap_high_at_top: bool` next to `bandmap_skip_worked`. Update the sibling `AppState { ... }` initializers in `reducer.rs`, `macro_expand.rs`, and `logger-cli/src/runner.rs` (grep for `bandmap_skip_worked:` will show every call site since they were all updated together recently).

### Config plumbing

- **`logger-tui/src/config.rs`** — add `bandmap_high_at_top: bool` to `StableConfig` (with `#[serde(default)]`) and to the flat `Config`. Follow the template used by `bandmap_skip_worked`.
- **`logger-runtime/src/bootstrap.rs`** — add `pub bandmap_high_at_top: bool` to `SessionConfig`. Copy into `AppState` in the state builder.
- **`logger-tui/src/main.rs`** — pass `bandmap_high_at_top: config.bandmap_high_at_top` into the `SessionConfig` literal.
- **`config.example.toml`** — document next to `bandmap_skip_worked`.

### Reducer — negate step direction when flag is set

- **`logger-core/src/reducer.rs`** — in the `AppEvent::BandmapUp | BandmapDown` arm (lines ~441–540), after `let is_down = matches!(ev, AppEvent::BandmapDown { .. })`, apply:

  ```rust
  let is_down = if st.bandmap_high_at_top { !is_down } else { is_down };
  ```

  That's the only reducer change. All the downstream index arithmetic (`prev_idx`, step wrap, the skip-worked loop) keeps operating on the ascending-by-freq natural slice, but now the interpretation of "down" matches the operator's visual expectation.

### Render — reverse display order and relocate divider

- **`logger-tui/src/ui/bandmap.rs`** — the slice returned by the cache is always ascending. Build the display rows in the chosen direction:

  ```rust
  let reverse_display = app.bandmap_high_at_top;
  let len = spots.len();

  // Resolve cursor to natural-slice indices first (unchanged math).
  let highlight_nat = match cursor { Some(On { call, .. }) => spots.iter().position(|s| &s.call == call), _ => None };
  let divider_nat   = match cursor { Some(Between { freq_hz }) => Some(spots.partition_point(|s| s.freq_hz < *freq_hz)), _ => None };

  // Translate to display positions.
  // In reversed mode, natural index n maps to display row (len - 1 - n).
  // The divider at natural insertion-point p (sits between spot p-1 and p
  // in natural order) lands at display insertion-point (len - p) in
  // reversed mode (between spot at display row len-p-1 and len-p).
  let highlight_disp = highlight_nat.map(|n| if reverse_display { len - 1 - n } else { n });
  let divider_disp   = divider_nat.map(|p| if reverse_display { len - p } else { p });
  ```

  Then iterate the spot slice in natural or reversed order depending on the flag (`spots.iter().rev()` branch), and compare against `*_disp` for highlight/divider placement. Scroll math already uses the display-position target, so it needs no further change once `highlight_disp` / `divider_disp` are substituted.

## Tests

Add to `logger-core/src/reducer.rs` test module (next to the existing `bandmap_nav_*` tests):

1. **Flag off — Down steps to higher freq** (control test, confirms today's behavior).
2. **Flag on — Down steps to lower freq.** Two spots (K5ZD at 14.025, W1AW at 14.027), cursor cleared. With `bandmap_high_at_top = true`, `BandmapDown` from nothing should land on the *lower*-freq spot (K5ZD), because reversed display puts K5ZD visually below W1AW.
3. **Flag on — interaction with skip-worked.** Both flags set; confirm skip loop steps in the visually-correct direction (skipping worked goes further "visually down," not further "up in freq").

Render-side correctness is hard to unit-test without a ratatui harness; the reducer tests cover the semantic correctness, and a manual TUI smoke test covers the visual.

## Verification (end-to-end)

1. `cargo test -p logger-core` — new reducer tests pass; existing `bandmap_nav_*` / `bandmap_snap_*` tests stay green.
2. `cargo test` workspace — no regressions.
3. Manual TUI: add `bandmap_high_at_top = true` to a `config.toml` with an active contest. Confirm:
   - Lowest-freq spot is now at the bottom of each bandmap panel; highest is at the top.
   - Ctrl-↓ still moves the highlight *visually down* (which is now toward lower freq).
   - Rig actually tunes to the new spot's freq (not the wrong direction).
   - Freq divider (when the rig is parked between spots) sits at the right visual insertion point.
   - Toggle the flag off → identical to pre-change behavior.

## Files touched (summary)

- `logger-core/src/state.rs` — +1 field.
- `logger-core/src/reducer.rs` — ~1 line in handler + 3 test cases + the new field in 2 `AppState { .. }` initializers.
- `logger-core/src/macro_expand.rs` — 4 `AppState { .. }` initializer updates.
- `logger-cli/src/runner.rs` — 1 `AppState { .. }` initializer update.
- `logger-runtime/src/bootstrap.rs` — +1 `SessionConfig` field + 1 assignment.
- `logger-tui/src/main.rs` — 1 line passing the config value.
- `logger-tui/src/config.rs` — +2 fields (StableConfig, Config) + 1 copy.
- `logger-tui/src/ui/bandmap.rs` — ~20 lines of index-mapping + reversed iteration.
- `config.example.toml` — 4-line comment block.
