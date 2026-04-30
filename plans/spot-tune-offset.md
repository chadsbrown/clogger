# CW spot-tune offset for bandmap navigation

## Context

Today, clogger's S&P bandmap navigation (`Ctrl+Up/Down` for R1,
`Ctrl+Alt+Up/Down` for R2) tunes exactly to the spotted frequency.
That is convenient, but on CW it also encourages every caller to land
on the same zero-beat frequency. In a pileup, that makes it harder for
the runner to distinguish individual callers.

N1MM+ has a related feature that randomizes incoming spot frequencies
slightly to improve packet pileup behavior. Clogger does not need to
copy that exact design; the operator goal here is narrower:

- Keep bandmap spot navigation fast and deterministic.
- Avoid exact zero-beat tuning on CW.
- Stay comfortably within a typical runner filter.

Reasonable assumptions for v1:

- Typical CW receive filter on the runner side is about **250 Hz**.
- In more normal contests, **300 Hz** is common.
- Therefore the offset should be **small**, not a large random jump.

## Recommendation

Add a **CW-only spot tune offset** feature for spot-based tuning actions:

- `Ctrl+Up/Down`
- `Ctrl+Alt+Up/Down`
- bandmap spot selection actions that snap to a real spot

Do **not** apply the offset to:

- empty-space bandmap clicks
- non-CW modes
- free/manual tuning unrelated to a spot

This should be treated as a **tuning policy** feature, not a general
macro or workflow feature.

## Suggested configuration

### `spot_tune_offset_hz`

Fixed CW offset applied around the spotted frequency.

Suggested values:

- `0` = disabled
- `50`
- `75`
- `100`
- `125`

Initial recommendation:

- ship with default `0` if we want conservative rollout
- use `75` as the recommended enabled value

### `spot_tune_offset_mode`

Controls which side of the spot we land on.

Suggested values:

- `none`
- `above`
- `below`
- `alternate`
- `random`

Semantics:

- `above`: tune to `spot + offset`
- `below`: tune to `spot - offset`
- `alternate`: flip above/below each spot tune
- `random`: choose above or below per tune

Initial recommendation:

- support `above`, `below`, `alternate`
- defer `random` unless we specifically want N1MM-style anti-pileup behavior

### `spot_tune_offset_modes`

Controls which operating modes use the feature.

Suggested values:

- `cw_only`
- `all`

Initial recommendation:

- `cw_only`

### `spot_tune_offset_sources`

Controls which spot-driven tune actions use the offset.

Suggested values:

- `keyboard_nav_only`
- `spot_select_only`
- `all_spot_tuning`

Initial recommendation:

- `all_spot_tuning` for any action that tunes to a real spot

## Recommended MVP behavior

The simplest good first implementation is:

- CW only
- fixed offset
- side = `alternate`
- offset size = `75 Hz`

Why:

- `75 Hz` is enough to avoid exact zero-beating
- it stays well inside the practical center of a 250-300 Hz CW filter
- `alternate` avoids everybody clustering on the same always-above or
  always-below convention

## What not to do in v1

- large random offsets like ±150 or ±200 Hz
- SSB or digital offset tuning
- filter-width-relative logic based on guessed DSP shapes
- applying offset to clicks in empty bandmap space
- overloading this with wider pileup or macro behavior

## Possible later enhancements

If the simple fixed-offset version proves useful, later options could
include:

- `random` side selection
- using the rig's reported filter width when available
- a `filter_relative` strategy that chooses a fraction of passband width
- per-contest or per-mode presets

Those should come only after the fixed CW-only version proves itself.

## Overall assessment

This is a good clogger feature candidate because it is:

- operationally useful
- narrow in scope
- easy to explain
- independent of macro-language complexity

The safest first design is:

- `spot_tune_offset_hz = 75`
- `spot_tune_offset_mode = alternate`
- `spot_tune_offset_modes = cw_only`

That captures most of the value without turning spot tuning into a
complicated policy engine.
