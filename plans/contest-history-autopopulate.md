# Contest History Auto-Population

## Context

When a callsign is typed, clogger auto-populates exchange fields from a
`.ch` call history file. But `.ch` data is pre-contest and stale.
If the operator already worked a station in the current contest and the
station sent a different exchange (or isn't in the `.ch` file at all),
the logged contest data should take priority. Two scenarios:

1. N3QE is in `.ch` with name "TINA", but sent "TIM" in this contest.
   On re-work, "TIM" should populate.
2. KB9RPG isn't in `.ch` at all. He sent "STEVE" earlier.
   On re-work, "STEVE" should populate.

## Current call-history flow (for reference)

- `reduce()` in `logger-core/src/reducer.rs:68` receives
  `call_history: &dyn CallHistoryLookup`.
- `CallHistoryLookup::lookup(call_norm) -> Option<Vec<(String, String)>>`
  returns `.ch` column-name/value pairs.
- On call-field change, `apply_call_history()` calls
  `apply_history_lookup()` (reducer.rs:681), which:
  1. Calls `call_history.lookup(call)`
  2. Pulls `contest.history_field_mapping() -> Vec<(&str, u16)>` —
     pairs like `("Name", 2)` mapping `.ch` column name to form
     field_id
  3. For each `(col, field_id)` in the mapping, if the `.ch` row has
     a value for `col` and the target form field is empty or already
     tagged `from_history`, fills the field and tags it
- Fields tagged `from_history = true` on `EntryFieldState` are
  clobberable by later lookups; manually typed fields are not.

## Exchange data in logged QSOs

- `QsoDraft.exchange_pairs: Vec<(String, String)>` uses contest-engine
  spec field ids as keys (e.g. `"name"`, `"loc"`).
- Spec-driven contests populate these keys from `spec_field.id`
  (`logger-core/src/contest/spec_driven.rs:162`).
- Form field_ids are positional: first received field → `field_id 2`,
  second → `field_id 3`, etc. (CALL is always `field_id 1`.)
- `QsoRecord` exposes `callsign_norm`, `exchange: ExchangeBlob`, and
  `flags.is_void`.
- `decode_exchange_pairs()` (logger-runtime) materializes the blob back
  into `Vec<(String, String)>`.

## Design

### Key decisions baked into this plan

- **The index lives inside `LogAdapter`.** Contest-history data is
  strictly derived state from the log — same as the scorer. LogAdapter
  already owns the `insert`/`undo`/`redo` mutation points and rebuilds
  the scorer there. Putting the index next to the scorer means one
  mutation signal feeds both. No new state to thread through
  `bootstrap.rs` / event loop / runner.
- **Mirror `CallHistoryLookup`'s return shape.** The new trait returns
  `Vec<(String, String)>` keyed by **spec field id** (matches
  `QsoDraft.exchange_pairs`). The reducer side then reuses a
  `contest.history_field_mapping()`-style translation. One abstraction
  for "auto-populate from a source of (field, value) pairs", two
  sources.
- **No new `exchange_field_id_mapping()` trait method.** With the spec-
  id-keyed return shape, the reducer's existing machinery for mapping
  source keys → form field_ids is enough. The contest's
  `history_field_mapping()` names `.ch` columns; we need an analogous
  identity mapping for spec ids, but that's derivable per-contest from
  the spec (`received_fields().map(|f| (f.id.clone(), (idx+2) as u16))`).
  Cache it as an internal helper or compute on the fly per lookup (it's
  tiny).
- **Field blocklist: `nr` + `rst`.** `nr` is the received sequence
  number from the other station (NS Sprint, Sweeps); his NR advances
  every QSO he makes, so replaying an old one is always wrong. `rst`
  is usually the mode default anyway but can carry a wrong value
  across a mode change (worked him on CW=599, re-work on SSB should
  be 59). Everything else populates. Note: the operator's own *sent*
  serial (`serial` key in stored pairs) is not a received-exchange
  field and has no form slot to populate into, so it's not on the
  blocklist.

### New trait: `ContestHistoryLookup` (in `logger-core`)

Same shape as `CallHistoryLookup`:

```rust
pub trait ContestHistoryLookup: Send + Sync {
    /// Returns exchange pairs keyed by spec field id, e.g.
    /// `[("name", "TIM"), ("loc", "NH")]`. Fields `nr` and `rst`
    /// are not returned (filtered at index-build time).
    fn lookup(&self, call_norm: &str) -> Option<Vec<(String, String)>>;
}

pub struct NoContestHistory;
impl ContestHistoryLookup for NoContestHistory { ... }
```

### New index on `LogAdapter`

`LogAdapter` gains a field:

```rust
contest_history: ContestHistoryIndex,
```

where `ContestHistoryIndex` owns a
`HashMap<String, Vec<(String, String)>>` keyed by normalized callsign
and storing spec-id pairs with `nr`/`rst` filtered out.

- Populated on open (`open_db`, `open_db_async`) via `rebuild()` over
  the loaded records.
- Updated on each `insert()` (after the scorer's `on_inserted`).
- Rebuilt on `undo()` / `redo()` (same place the scorer rebuilds).
- Exposed via `impl ContestHistoryLookup for LogAdapter` — mirrors
  the existing `impl DupeChecker` / `impl MultChecker`.

The index builder consults `contest_engine::spec::embedded::spec_by_id`
to know which field ids to filter out. Simplest: a single
`const BLOCKED_FIELDS: &[&str] = &["nr", "rst"]`.

### Reducer: new `contest_history` parameter + merged lookup

`reduce()` gains `contest_history: &dyn ContestHistoryLookup`. Pass it
through to `apply_call_history()` → `apply_history_lookup()`.

`apply_history_lookup()` runs in two passes:

1. **Contest history first.** `contest_history.lookup(call)` returns
   spec-id pairs. For each pair, translate the spec id to a form
   field_id via the spec-driven mapping (or, for non-spec contests
   without exchange, an empty mapping that no-ops). Fill matching
   empty / `from_history` fields. Tag `from_history = true`.
2. **Call history fills gaps.** Run today's call-history path
   unchanged, but an already-populated-from-contest-history field
   is treated like any other `from_history` field — clobberable by
   call-history only if the current value is still `from_history`.

Clobber semantics (to make explicit):
- Manually typed (`from_history = false`) → never overwritten.
- `from_history = true` field → overwritten by either source if the
  source provides a value. Order within one `apply_history_lookup`
  call: contest history wins. Sequential call-field edits each rerun
  the full pass, so the freshest data always wins.

### LogAdapter-as-trait-object plumbing

Today, `reduce()` takes `&dyn DupeChecker` and `&dyn MultChecker`
separately, both satisfied by the same `LogAdapter`. We'd add
`&dyn ContestHistoryLookup` the same way. Callers already pass
`&log_adapter` for dupe/mult; they pass it again for contest history.
No new wiring in `bootstrap`, the event loop, or the CLI runner —
just threading one more reference through `reduce()`.

## Files to modify

1. **`logger-core/src/reducer.rs`**
   - Add `ContestHistoryLookup` trait and `NoContestHistory` stub.
   - Add `contest_history: &dyn ContestHistoryLookup` parameter to
     `reduce()`.
   - Extend `apply_history_lookup()` with the two-pass merge.
   - Update the test-helper `reduce()` wrapper to pass
     `&NoContestHistory`.
2. **`logger-core/src/lib.rs`** — re-export the new trait + stub.
3. **`logger-runtime/src/contest_history.rs`** — new file.
   - `ContestHistoryIndex`: `HashMap<String, Vec<(String, String)>>`,
     `rebuild(records)`, `on_inserted(call_norm, exchange_pairs)`,
     `lookup(call)`.
   - Blocklist: skip `nr`, `rst`.
4. **`logger-runtime/src/log_adapter.rs`**
   - Own a `ContestHistoryIndex`.
   - Call `rebuild` on `open_db`/`open_db_async` load and on
     `undo`/`redo`.
   - Call `on_inserted` after `scorer.on_inserted` in `insert`.
   - `impl ContestHistoryLookup for LogAdapter`.
5. **`logger-runtime/src/lib.rs`** — expose module if any types leak
   (probably not; `ContestHistoryIndex` stays module-private).
6. **`logger-tui/src/event_loop.rs`** — pass `&log_adapter` as the
   `ContestHistoryLookup` when calling `reduce()`. Same pattern as
   the existing dupe/mult wiring.
7. **`logger-cli/src/runner.rs`** — same threading. The existing
   `SerialContest` wrapper doesn't need updates under this design
   (no new `ContestEntry` methods).

Compared to the original plan, this drops: `ContestEntry::exchange_field_id_mapping`,
a top-level `Session::contest_history`, and the event-loop / runner
dispatch patches for `on_inserted` / `rebuild`.

## Decisions

- **Keying:** call-only, last non-voided QSO wins. State-QP rovers
  whose LOC changes between bands mis-populate on re-work (operator
  corrects manually) — acceptable v1 tradeoff; revisit if common.
- **Field blocklist:** `nr`, `rst`. `nr` replays a stale NR (the
  other station's counter has advanced). `rst` may carry a wrong
  mode's value (CW 599 into SSB field). Every other received field
  populates.
- **`from_history` tagging:** single flag shared by both sources.
  Contest-history runs first in the merge; call-history fills
  remaining gaps. Manually typed fields (`from_history = false`)
  are never overwritten.
- **Multi-op:** index is global. Any op's log entry populates for
  any op. Matches the dupe/score/log-view sharing model.
- **`block_dupes` interaction:** autopopulate fires regardless of
  dupe status. It's a display convenience; the block happens at
  ESM time, not at field-fill time. No code change required — the
  reducer already applies history unconditionally.
- **Void handling:** falls out for free. `rebuild` filters on
  `!flags.is_void`, so undo/void removes a call from the index
  and re-insert restores it. No special path needed.

## Verification

- `cargo test` — all existing tests pass (`NoContestHistory` means
  no behavioral change when not wired).
- Unit test on `LogAdapter`: insert two QSOs for same call with
  different names, assert `lookup(call)` returns the second name.
- Unit test on `LogAdapter`: insert + undo, assert `lookup(call)`
  returns `None` (or the prior QSO's data if any).
- Unit test: insert QSO with `nr` + `rst` fields, assert those
  keys don't appear in `lookup`.
- Golden script (CWT): log N3QE with name "TIM", clear entry,
  re-enter "N3QE", assert NAME field is "TIM" when `.ch` has "TINA".
- Golden script (CWT): log KB9RPG with name "STEVE" (no `.ch`
  entry), clear, re-enter, assert NAME is "STEVE".
- Golden script: after autopopulate, manually edit the name field,
  assert subsequent lookups do not overwrite it (`from_history`
  gate still holds).
