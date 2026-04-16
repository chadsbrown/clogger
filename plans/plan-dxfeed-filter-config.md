# Plan: DxFeed filter.json and SkimmerQualityConfig support

> **Status (2026-04-16): Phase 1 shipped.** The TOML-driven config surface
> is live; hot-reload was explicitly deferred at planning time. See
> "Shipped" and "Deferred" sections below.

## Goal

Expose dxfeed's two configuration axes — the filter pipeline (filter.json)
and the skimmer quality engine (SkimmerQualityConfig) — through clogger's
config so operators can tune RBN noise rejection without recompiling.

## Open question — resolved

**Where should the new config fields live — stable config or contest config?**
→ **Stable config (`config.toml`) only.** Decided at implementation time.
Skimmer quality and filter rules are station infrastructure; per-contest
filter profiles can be revisited as a follow-up if needed (see Deferred).

---

## Shipped

### 1. `DxFeedConfig` extended (`logger-runtime/src/config.rs`)

```toml
[dxfeed]
filter_file = "/path/to/filter.json"        # optional

[dxfeed.skimmer_quality]                    # optional; omit → dxfeed defaults
valid_required_distinct_skimmers = 5
lookback_window_secs = 90
allow_busted = false
# ...any subset of fields; missing ones use dxfeed::SkimmerQualityConfig::default()
```

Implementation:
- `DxFeedConfig` gains `filter_file: Option<PathBuf>` and
  `skimmer_quality: Option<SkimmerQualityConfigSerde>`.
- `SkimmerQualityConfigSerde` is a TOML-friendly mirror with **every field
  optional** and a `to_dxfeed()` conversion that fills missing values from
  `SkimmerQualityConfig::default()`. This decouples clogger's TOML schema
  from dxfeed's internal type and lets users specify only what they want
  to override.
- `#[serde(deny_unknown_fields)]` on the wrapper catches typos
  (`valid_requierd_…`) at parse time instead of silently ignoring them.
- `validate()` rejects nonsensical inputs: zero corroborators, zero
  lookback, non-positive frequency windows.

### 2. Adapter loads & validates at startup (`logger-runtime/src/dxfeed_adapter.rs`)

- `filter_file` is read & deserialized as `FilterConfigSerde` (extracted
  helper `load_filter_file()` for testability). Missing or invalid file →
  startup fails with the path in the error. Matches the `db_path` /
  `contest.toml` strictness pattern: an operator who asked for a filter
  shouldn't accidentally start without one.
- `skimmer_quality` is validated, converted, then passed to
  `DxFeedBuilder::set_skimmer_quality()`. When omitted, dxfeed's defaults
  apply (no behavior change for existing users).
- `spawn_dxfeed_adapter` signature is unchanged — still returns
  `Result<()>`. The `DxFeed` handle is consumed inside the spawned task
  (no hot-reload surface — see Deferred).

### 3. Tests (`logger-runtime/src/{config,dxfeed_adapter}.rs`)

Eight unit tests cover:
- `SkimmerQualityConfigSerde::default()` round-trip matches dxfeed defaults.
- Partial override (just two fields) preserves all other defaults.
- Validation rejects `valid_required_distinct_skimmers = 0`.
- Validation rejects negative frequency windows.
- `deny_unknown_fields` rejects typo'd field names.
- `load_filter_file` errors clearly on missing path.
- `load_filter_file` errors clearly on invalid JSON.
- `load_filter_file` succeeds on a valid serialized `FilterConfigSerde`.

### 4. Documentation (`config.example.toml`)

Both `filter_file` and the full `[dxfeed.skimmer_quality]` section are
shown commented-out with dxfeed's defaults inline and per-knob hints on
which direction to push for noisy environments.

---

## Deferred

These were in the original plan but consciously left out of v1 to keep
the patch small. Pick up when the corresponding need surfaces.

### Hot reload of `filter_file`

Originally Section 4 of this plan. Required signature change to return
the `DxFeed` handle from `spawn_dxfeed_adapter`, plumbing through the
event loop, and a key binding to re-read the JSON. Skipped because:

- Filter changes are uncommon during a contest.
- Restart is acceptable for the v1 user.
- Adds a non-trivial UI surface (status bar feedback for success/error).

When picked up:
- Change `spawn_dxfeed_adapter` → `Result<DxFeed>`.
- Capture handle in `logger-tui/src/main.rs`; pass to event loop.
- New `Effect` or direct keybinding handler that calls
  `dxfeed_handle.update_filter(load_filter_file(path)?)`.
- Status-bar feedback line on success/failure.

### Skimmer quality hot reload

`DxFeed` exposes `update_skimmer_config()` and convenience methods like
`set_skimmer_gate(bool)` / `set_allow_unverified(bool)` that would be
nice keyboard shortcuts mid-contest ("show me unknown spots for 30 sec
to find the rare one"). Same hot-reload plumbing as above.

### Per-contest `filter_file` override in `contest.toml`

Originally listed as "possible compromise" in the open question.
Mirrors how `db_path` works. Trivial extension once the stable-config
path is shipped: add `Option<PathBuf>` to `ContestConfig`, merge in
`Config::from_parts()`, contest overrides stable.

### Spot metadata for downstream filtering

dxfeed parses `snr_db`, `wpm`, `unique_originators`, and originator kind
(skimmer vs human) per `SpotObservation`, but the clogger `Spot` struct
(`logger-core/src/state.rs`) only carries `call`, `freq_hz`, `mode`. To
support clogger-side filters like "drop RBN spots below 8 dB SNR" or
"prefer human spotters in dedup," extend `Spot` to carry the metadata
and surface it in `AppEvent::SpotReceived`.

This would unlock filters that the dxfeed crate doesn't (and shouldn't)
own — they're consumer policy, not pipeline policy.

### Observability of dropped spots

dxfeed has zero logging or metrics about why spots were dropped. To
diagnose "is the busted-reject actually firing?" today, an operator has
no signal. Two cheap options when needed:

1. `tracing::debug!` at `aggregator/core.rs:268-272` (the gating point)
   logging `(call, freq, tag, originator_kind)` for every dropped spot;
   gate behind `RUST_LOG=dxfeed=debug`.
2. Per-tag drop counters surfaced in clogger's status bar
   (`valid_passed`, `busted_dropped`, `unknown_dropped`,
   `human_passthrough`).

Lives in dxfeed, not clogger; needs cross-repo coordination.

### GUI filter editor

Out of scope, mentioned by user as a long-horizon possibility.

---

## Files touched (Phase 1)

| File | Change |
|------|--------|
| `logger-runtime/src/config.rs` | `filter_file` + `skimmer_quality` on `DxFeedConfig`; `SkimmerQualityConfigSerde`; tests |
| `logger-runtime/src/dxfeed_adapter.rs` | Load filter, convert/validate skimmer config, pass to builder; `load_filter_file` helper + tests |
| `logger-runtime/Cargo.toml` | `toml = "0.8"` dev-dep for round-trip tests |
| `config.example.toml` | Document both new sections with annotated defaults |
