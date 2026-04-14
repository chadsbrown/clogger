# Contest History Auto-Population

## Context

When a callsign is typed, clogger auto-populates exchange fields from a `.ch` call history file. But the `.ch` file contains pre-contest data. If the operator already worked a station in the current contest and that station sent a different exchange (or isn't in the `.ch` file at all), the logged contest data should take priority. Two scenarios:

1. N3QE is in `.ch` with name "TINA", but sent "TIM" in this contest. On re-work, "TIM" should populate.
2. KB9RPG isn't in `.ch` at all. He sent "STEVE" earlier. On re-work, "STEVE" should populate.

## Current Call History Flow

- `reduce()` in `logger-core/src/reducer.rs:68` receives `call_history: &dyn CallHistoryLookup`
- `CallHistoryLookup` trait (reducer.rs:40): `fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>>` — returns .ch column-name/value pairs like `[("Name", "TINA"), ("CqZone", "5")]`
- When the call field changes, `apply_call_history()` (reducer.rs:636) calls `apply_history_lookup()` (reducer.rs:681)
- `apply_history_lookup()` calls `call_history.lookup(callsign)`, then uses `contest.history_field_mapping()` to map .ch column names to form field_ids, and populates matching fields
- `history_field_mapping()` is per-contest, e.g. CWT: `[("Name", 2), ("Exch1", 3)]`, CQWW: `[("CqZone", 3)]`
- Fields auto-populated from history are tagged with `from_history = true` on `EntryFieldState` — only empty or already-from-history fields are overwritten

## Exchange Data in Logged QSOs

- `QsoDraft.exchange_pairs: Vec<(String, String)>` uses contest-engine spec field IDs as keys (e.g. "name", "zone")
- For spec-driven contests, keys come from `spec_field.id` (logger-core/src/contest/spec_driven.rs:162)
- Form field_ids are positional: first received field → field_id 2, second → field_id 3, etc.
- Exchange pairs are encoded as JSON blobs in `ExchangeBlob`, decoded via `decode_exchange_pairs()` (logger-runtime/src/log_adapter.rs:269)
- `LogAdapter` exposes `ordered_records() -> Vec<QsoRecord>` — `QsoRecord` has `callsign_norm`, `exchange: ExchangeBlob`, `flags.is_void`
- No existing method to look up exchange by callsign — only `is_dupe(call, band, mode) -> bool`

## Design

### New trait: `ContestHistoryLookup` (in logger-core)

Follows the same pattern as `CallHistoryLookup` but returns field_id → value pairs directly (no column-name indirection needed since the data comes from our own logged QSOs):

```rust
pub trait ContestHistoryLookup {
    fn lookup(&self, call_norm: &str) -> Option<Vec<(u16, String)>>;
}
```

Plus a `NoContestHistory` stub (like `NoCallHistory`).

### New struct: `ContestHistoryIndex` (in logger-runtime)

An in-memory `HashMap<String, Vec<(u16, String)>>` keyed by normalized callsign. Maps each call to its most recently logged exchange fields as (field_id, value) pairs.

- Built from `LogAdapter::ordered_records()` at bootstrap via `rebuild()`
- Updated incrementally via `on_inserted()` after each `Effect::LogInsert`
- Rebuilt from scratch on undo/redo (matches existing scorer rebuild pattern)
- Implements the `ContestHistoryLookup` trait

Needs a mapping from exchange_pair keys (spec field IDs like "name") to form field_ids (2, 3...). This is positional — first received field → field_id 2, second → 3, etc. Add a `exchange_field_id_mapping()` method to `ContestEntry` trait that returns `Vec<(String, u16)>`. Spec-driven contests derive it from `received_fields()`. The index stores this mapping at construction time.

### Priority in `apply_history_lookup()`

Contest history checked first. For any field populated by contest history, skip call history. Call history fills remaining gaps. The `from_history` flag is set either way — the field tracks "auto-populated" regardless of source.

## Files to modify

### 1. `logger-core/src/reducer.rs` — trait + reduce() signature + lookup logic
- Add `ContestHistoryLookup` trait and `NoContestHistory` stub (next to `CallHistoryLookup`)
- Add `contest_history: &dyn ContestHistoryLookup` parameter to `reduce()`
- Pass it through to `apply_call_history()` → `apply_history_lookup()`
- In `apply_history_lookup()`: check contest history first, then call history for remaining empty fields
- Update the test helper `reduce()` wrapper (around line 759) to pass `&NoContestHistory`

### 2. `logger-core/src/contest/traits.rs` — new trait method
- Add `fn exchange_field_id_mapping(&self) -> Vec<(String, u16)> { vec![] }` to `ContestEntry`

### 3. `logger-core/src/contest/spec_driven.rs` — implement mapping
- Override `exchange_field_id_mapping()` on `SpecDrivenContest`: iterate `received_fields()`, return `(spec_field.id.clone(), (idx as u16) + 2)` pairs

### 4. `logger-runtime/src/contest_history.rs` — new file
- `ContestHistoryIndex` struct with `HashMap<String, Vec<(u16, String)>>` and the field mapping
- `new(mapping: Vec<(String, u16)>)` constructor
- `rebuild(&mut self, records: &[QsoRecord])` — scans all non-voided records, decodes exchange pairs, maps to field_ids, last one wins per callsign
- `on_inserted(&mut self, call_norm: &str, exchange_pairs: &[(String, String)])` — upserts a single entry
- `impl ContestHistoryLookup for ContestHistoryIndex`

### 5. `logger-runtime/src/lib.rs` — expose new module

### 6. `logger-runtime/src/bootstrap.rs` — construct and seed index
- Create `ContestHistoryIndex` from `contest.exchange_field_id_mapping()`
- Seed it via `rebuild()` from `log_adapter.ordered_records()` (so restart picks up prior session data)
- Add to `Session` struct

### 7. `logger-tui/src/event_loop.rs` — wire into event loop
- Add `contest_history` to `run()` parameters
- Pass `&contest_history` to both `reduce()` calls
- In `dispatch_effects()`, after `Effect::LogInsert` insert succeeds, call `contest_history.on_inserted()`
- On undo/redo (if/when wired), call `contest_history.rebuild()`

### 8. `logger-cli/src/runner.rs` — wire into CLI runner
- Create `ContestHistoryIndex`, pass to `reduce()` calls
- Update after `Effect::LogInsert` in the effect dispatch
- Update `SerialContest` wrapper to delegate `exchange_field_id_mapping()`

### 9. `logger-tui/src/main.rs` — pass contest_history from Session to run()

## Verification

- `cargo test` — all existing tests pass (NoContestHistory stub means no behavioral change)
- Add golden test script: CWT contest, log N3QE with name "TIM", clear, re-enter "N3QE" — verify NAME field auto-populates with "TIM" (with call_history entry having "TINA")
- Add golden test script: CWT contest, log KB9RPG with name "STEVE" (no call_history entry), clear, re-enter "KB9RPG" — verify NAME field auto-populates with "STEVE"
- Both scripts verify `from_history` behavior: manually typed values are not overwritten
