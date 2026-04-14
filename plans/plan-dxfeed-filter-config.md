# Plan: DxFeed filter.json and SkimmerQualityConfig support

## Goal

Expose dxfeed's two configuration axes — the filter pipeline (filter.json)
and the skimmer quality engine (SkimmerQualityConfig) — through clogger's
config, with hot-reload support for both.

## Current state

- `DxFeedConfig` has only `sources: Vec<DxFeedSourceConfig>` (host/port/callsign).
- `spawn_dxfeed_adapter` hardcodes `SkimmerQualityConfig::default()` and
  uses no filter at all.
- The adapter returns `Result<()>` — the `DxFeed` handle (which exposes
  `update_filter()`, `update_skimmer_config()`, etc.) is consumed inside the
  spawned task and not accessible afterward.

## Open question

**Where should the new config fields live — stable config or contest config?**

Arguments for **stable config** (config.toml): filter preferences and skimmer
quality thresholds are station infrastructure — they describe how you want to
receive spots regardless of which contest you're operating. Similar to rig,
keyer, and SO2R config.

Arguments for **contest config** (contest.toml): you might want different
filter profiles per contest (e.g., band-deny lists that match your contest
category, or geographic filters tuned to a particular contest's scoring
rules). A CQWW filter profile might look very different from a state QSO
party filter.

A possible compromise: put the fields in stable config (since that's where
`[dxfeed]` already lives), but allow contest.toml to override the
`filter_file` path. This mirrors how `db_path` works — defaulted in one
place, overridable per-contest. Decide before implementing.

## Changes

### 1. Extend `DxFeedConfig` (logger-runtime/src/config.rs)

Add two optional fields to the existing `DxFeedConfig`:

```toml
[dxfeed]
sources = [...]
filter_file = "/path/to/filter.json"       # optional

[dxfeed.skimmer_quality]                   # optional; omit entire section for defaults
enabled = true
gate_skimmer_output = true
apply_only_to_skimmer = true
compute_valid = true
compute_busted = true
compute_qsy = true
allow_valid = true
allow_qsy = false
allow_unknown = false
allow_busted = false
valid_required_distinct_skimmers = 3
valid_freq_window_hz = 300
lookback_window_secs = 180
busted_freq_window_hz = 100
similar_call_max_edit_distance = 1
qsy_freq_window_hz = 400
max_tracked_observations = 100000
```

In Rust:

- Add `filter_file: Option<PathBuf>` to `DxFeedConfig`.
- Add `skimmer_quality: Option<SkimmerQualityConfigSerde>` to `DxFeedConfig`.
- Define `SkimmerQualityConfigSerde` as a serde-friendly mirror of dxfeed's
  `SkimmerQualityConfig` with all fields optional and defaulting to dxfeed's
  defaults. This keeps clogger's config decoupled from dxfeed's internal
  types (serde impls may differ, and we want TOML-friendly field names like
  `lookback_window_secs` rather than a `Duration`).
- Add a `fn to_dxfeed(&self) -> SkimmerQualityConfig` conversion method.
- When `skimmer_quality` is `None`, use `SkimmerQualityConfig::default()`
  (preserves current behavior).
- When `filter_file` is `None`, use `FilterConfigSerde::default()` (preserves
  current behavior: no filtering).

### 2. Load and validate at startup (logger-runtime/src/dxfeed_adapter.rs)

Update `spawn_dxfeed_adapter`:

- Accept the extended `DxFeedConfig`.
- If `filter_file` is `Some`, read and deserialize it as
  `FilterConfigSerde`. Fail startup with a clear error if the file is missing
  or invalid (same pattern as config.toml — don't silently degrade).
- Pass the filter to `DxFeedBuilder::set_filter()`.
- Convert `skimmer_quality` config (or default) and pass to
  `DxFeedBuilder::set_skimmer_quality()`.
- Return the `DxFeed` handle instead of `()` so the caller can use it for
  hot reload.

New signature:

```rust
pub async fn spawn_dxfeed_adapter(
    config: &DxFeedConfig,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<DxFeed>
```

### 3. Return and store the DxFeed handle (logger-tui/src/main.rs)

- Capture the `DxFeed` handle returned by `spawn_dxfeed_adapter`.
- Pass it into the event loop (or store it in a struct accessible from the
  event loop) so hot-reload commands can reach `update_filter()` and
  `update_skimmer_config()`.

### 4. Hot reload

Add a key binding (or reuse an existing command mechanism) that:

1. Re-reads the `filter_file` from disk.
2. Deserializes it as `FilterConfigSerde`.
3. Calls `dxfeed_handle.update_filter(new_filter)`.
4. On success, shows a brief confirmation in the status bar or error banner.
5. On failure (bad JSON, validation error), shows the error without changing
   the active filter.

SkimmerQualityConfig hot reload is also possible via
`dxfeed_handle.update_skimmer_config()`, but there's no external file to
re-read — the values come from TOML config. Options:

- **Option A:** Don't hot-reload skimmer quality config. It changes rarely
  and a restart is acceptable. Simpler.
- **Option B:** Re-read the TOML config file on the same key binding and
  apply changes to both filter and skimmer quality. More complete but means
  parsing TOML mid-session.
- **Option C:** Expose individual skimmer quality toggles as key bindings
  (e.g., toggle `allow_unknown`). Useful during a contest for quick
  adjustments. Could use the `set_allow_unverified()` / `set_skimmer_gate()`
  convenience methods on the DxFeed handle.

Recommend starting with hot reload for filter.json only (Option A for
skimmer quality). Add skimmer quality hot-reload later if needed.

### 5. Wire through config layers (logger-tui/src/config.rs)

No structural changes needed — `DxFeedConfig` flows from `StableConfig`
through `Config` to the adapter already. The new fields will carry through
automatically since `DxFeedConfig` is passed as-is.

If the open question is resolved with a contest-config override for
`filter_file`, add an `Option<PathBuf>` field to `ContestConfig` and merge
it in `Config::from_parts()` (contest overrides stable, same as `db_path`).

### 6. Startup failure behavior

- Missing or invalid `filter_file`: **fail startup** with a descriptive
  error. The operator explicitly asked for a filter; silently ignoring it
  could let unwanted spots through during a contest, which is worse than
  refusing to start.
- Missing `[dxfeed.skimmer_quality]` section: use defaults (current
  behavior).
- Invalid skimmer quality values (e.g., `valid_required_distinct_skimmers =
  0`): **fail startup**. Validate before passing to dxfeed.

### 7. Testing

- Add a golden script test (logger-cli) that exercises spot filtering? Not
  straightforward — the CLI runner doesn't go through dxfeed, it injects
  `SpotReceived` events directly. Filtering happens inside dxfeed before
  events reach clogger.
- Unit tests for `SkimmerQualityConfigSerde` -> `SkimmerQualityConfig`
  conversion, especially defaults and partial overrides.
- Unit test for filter file loading (valid JSON, invalid JSON, missing file).
- Integration test: can defer to dxfeed's own test suite for filter behavior.

## Files to modify

| File | Change |
|------|--------|
| `logger-runtime/src/config.rs` | Add `filter_file`, `skimmer_quality` to `DxFeedConfig`; add `SkimmerQualityConfigSerde` |
| `logger-runtime/src/dxfeed_adapter.rs` | Load filter, convert skimmer config, pass to builder, return `DxFeed` handle |
| `logger-runtime/src/lib.rs` | Re-export new types if needed |
| `logger-tui/src/main.rs` | Store `DxFeed` handle, pass to event loop |
| `logger-tui/src/event_loop.rs` | Accept `DxFeed` handle; add hot-reload key binding handler |
| `logger-tui/src/adapters/terminal.rs` | Add key binding for filter reload (if a new key is chosen) |
| `logger-core/src/events.rs` | Add `ReloadDxFilter` event if hot reload goes through the reducer (or handle it purely in TUI) |

## Out of scope (future)

- GUI filter editor (mentioned by user as a future possibility).
- Per-contest filter_file override in contest.toml (depends on open question).
- Skimmer quality hot-reload (Option B or C above).
- Exposing filter/skimmer state in the TUI (e.g., showing active filter
  name, skimmer quality stats).
