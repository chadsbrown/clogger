# Response to Project Review

## Context

A code review identified six findings. This plan documents the current
status of each, validates whether the finding still applies, and specifies
a concrete fix plan where action is needed.

Findings are listed in the original review's priority order, with a
recommended execution order at the bottom.

---

## #1 — SO2R rig state propagation (High) — partially addressed

### Review finding

> rig_adapter keeps a single LastRigState (freq/mode/ptt) and reuses it for
> all receiver events. That means a mode change on one receiver can be
> paired with frequency from another receiver when emitting
> AppEvent::RigStatus. Also, PTT events are hard-coded to radio: 1.

### Current status

The hardcoded `radio: 1` on PTT events was **fixed** during the SO2R
refactor (`logger-runtime/src/rig_adapter.rs`). All event variants
(`FrequencyChanged`, `ModeChanged`, `PttChanged`, `Disconnected`) now use
`radio: radio_id` pulled from `config.radio_id`.

For the user's stated SO2R setup (two separate physical rigs, each with
its own CAT connection, `[[rig]]` array in TOML), this works correctly:
each rig spawns its own adapter task with its own `LastRigState` and its
own configured `radio_id`.

### Remaining concern

The single `LastRigState` per adapter still creates a cross-contamination
risk if a rig has multiple receivers (K3+KRX3, FlexRadio with two slices,
IC-7610 with sub-RX). In that scenario all receivers share the same
`LastRigState`, so a `ModeChanged` on receiver B would overwrite `last.mode`
and the next `FrequencyChanged` on receiver A would ship a stale mode.

The current code already ignores `receiver` in the match arms
(`receiver: _`), which is correct for the single-receiver-per-adapter case
but silently wrong for multi-receiver-per-adapter.

### Fix plan

Ship a doc-only change for v1; defer the code fix until multi-receiver
hardware is actually requested.

- Add a `// NOTE:` comment in `rig_adapter.rs` above the match loop stating
  that this adapter assumes one receiver per rig, and that multi-receiver
  rigs (K3+KRX3, FlexRadio multi-slice) are not currently supported.
- Document in the TOML example that each `[[rig]]` entry represents one
  physical rig with one primary receiver.

When multi-receiver support is needed:
- Track per-receiver state: `HashMap<ReceiverId, LastRigState>`.
- Add a config mapping from `(rig, receiver_index)` to `radio_id`, e.g.:
  ```toml
  [[rig]]
  model = "K3"
  port = "/dev/ttyUSB0"
  [[rig.receivers]]
  index = 0
  radio_id = 1
  [[rig.receivers]]
  index = 1
  radio_id = 2
  ```
- Route events per-receiver to the right `radio_id`.

### Files touched (v1)

- `logger-runtime/src/rig_adapter.rs` — doc comment
- `logger-tui.example.toml` — clarifying note

---

## #2 — Undo/redo not persisted to SQLite (High) — real bug, easy fix

### Review finding

> insert() drains pending ops and appends to sink, but undo() and redo()
> don't persist drained ops afterward. If you restart, in-memory state
> and DB may diverge.

### Current status

Confirmed in `logger-runtime/src/log_adapter.rs`. `insert()` has:

```rust
if let Some(sink) = &mut self.sink {
    let ops = self.store.drain_pending_ops();
    if !ops.is_empty() {
        sink.append_ops(&ops)
            .map_err(|e| anyhow::anyhow!("persist failed: {e:?}"))?;
    }
}
```

But `undo()` and `redo()` just call `self.store.undo()`/`redo()` and return.
The underlying `QsoStore` presumably records undo/redo as ops in the
pending queue, but they're never drained to the sink.

After a restart, `SqliteOpSink::load_store()` replays only the ops that
were appended. An undo done before the crash is lost — the in-memory state
shows the QSO as voided, but the restored state shows it live again.

### Fix plan

1. Extract a private helper on `LogAdapter`:
   ```rust
   fn flush_pending_ops(&mut self) -> Result<()> {
       if let Some(sink) = &mut self.sink {
           let ops = self.store.drain_pending_ops();
           if !ops.is_empty() {
               sink.append_ops(&ops)
                   .map_err(|e| anyhow::anyhow!("persist failed: {e:?}"))?;
           }
       }
       Ok(())
   }
   ```
2. Call it at the end of `insert()`, `undo()`, `redo()`.
3. Add a unit test: insert → undo → reload from a temp file DB → verify the
   record is still voided. Then insert → undo → redo → reload → verify
   it's restored.

### Files touched

- `logger-runtime/src/log_adapter.rs`
- New test in the same file's `mod tests`

---

## #3 — Contest identity mismatch (Medium/High) — real conflation

### Review finding

> ContestEntry exposes contest_instance_id(), but LogAdapter::insert writes
> contest_instance_id from draft.exchange_schema_id. That conflates
> concepts unless intentionally identical forever.

### Current status

Confirmed in `logger-runtime/src/log_adapter.rs:56`:

```rust
let store_draft = StoreDraft {
    contest_instance_id: draft.exchange_schema_id as u64,
    ...
};
```

All four current contests happen to have matching values:

| Contest | contest_instance_id | exchange_schema_id |
|---------|---------------------|---------------------|
| CQWW    | 1                   | 1                   |
| Sweeps  | 2                   | 2                   |
| CWT     | 3                   | 3                   |
| MST     | 4                   | 4                   |

So it works today, but it conflates two distinct concepts:
- **contest_instance_id**: identifies a contest run ("CQWW 2024 CW")
- **exchange_schema_id**: identifies an exchange data format

Divergence scenarios:
- Two CQWW runs in the same DB would both use exchange_schema_id=1 but
  need different contest_instance_id values.
- A contest with multiple sent/received variants would have one
  contest_instance_id but multiple exchange_schema_ids.

### Fix plan

Bind the contest to the `LogAdapter` at construction:

1. Change constructor signature:
   ```rust
   pub fn new(scorer: Box<dyn ContestScorer>, contest_instance_id: u64) -> Self
   pub fn open_db(scorer: Box<dyn ContestScorer>, contest_instance_id: u64, path: &Path) -> Result<Self>
   ```
2. Store `contest_instance_id: u64` as a field on `LogAdapter`.
3. In `insert()`, use `self.contest_instance_id` instead of
   `draft.exchange_schema_id`.
4. Update `logger-runtime/src/bootstrap.rs` to pass
   `contest.contest_instance_id()` when building the adapter.
5. Update `logger-cli/src/runner.rs` (script runner) the same way.
6. Leave `exchange_schema_id` on `QsoDraft` as-is — it's still useful for
   identifying the exchange format when decoding.

### Files touched

- `logger-runtime/src/log_adapter.rs`
- `logger-runtime/src/bootstrap.rs`
- `logger-cli/src/runner.rs`
- Any tests that construct `LogAdapter::new` directly

---

## #4 — Call history parser not CSV-safe (Medium)

### Review finding

> .ch parsing uses naive split(','), so quoted fields containing commas
> will break column alignment and values.

### Current status

Confirmed in `logger-runtime/src/call_history.rs:41`:

```rust
let fields: Vec<&str> = trimmed.split(',').collect();
```

No handling of quoted fields. A line like `K1ABC,"Smith, John",5,1234`
would split into 5 fields instead of 4, and subsequent columns would be
misaligned.

Most public `.ch` files I've seen don't use quoted fields (the ICWC-MST
reference file is all simple unquoted CSV), but N1MM's format does support
them. This will eventually bite someone.

### Fix plan

Bring in the `csv` crate and use it for parsing. `csv` is a well-maintained
crate with minimal dependencies that handles RFC 4180 quoting correctly.

1. Add `csv = "1"` to `logger-runtime/Cargo.toml`.
2. Rewrite `CallHistoryDb::parse` to use `csv::ReaderBuilder` with
   `has_headers(false)` (since we have our own custom `!!Order!!` header
   line and comment lines). Read line-by-line, skip comments/blanks, parse
   header line with the `csv` crate, then parse data lines the same way.
3. Add a test case with a quoted field containing a comma:
   ```rust
   let content = "!!Order!!,Call,Name,Exch1\nK1ABC,\"Smith, John\",5";
   ```

### Files touched

- `logger-runtime/Cargo.toml`
- `logger-runtime/src/call_history.rs`

---

## #5 — Workspace/runtime portability (Medium) — already planned

### Review finding

> logger-runtime depends on several sibling path crates outside this repo
> (../../contest-engine, ../../winkey, etc.), making standalone builds/tests
> fail in this repo-only context.

### Current status

This is the same issue addressed by `plans/monorepo.md`. The plan is to
consolidate all external path-dependency crates into a single monorepo
workspace. Once that's done, all `path = "../../..."` references become
workspace-local (`path = "../qsolog"`) and standalone builds work.

### Fix plan

No new work here. Execute `plans/monorepo.md` when ready.

---

## #6 — Connection indicators misleading (Low/Medium UX)

### Review finding

> Status bar intentionally avoids showing red "disconnected" states when
> not connected because configured-vs-not-configured is not tracked. That's
> fine for avoiding noise, but ambiguous during troubleshooting.

### Current status

Confirmed in `logger-tui/src/ui/status_bar.rs:42-47`:

```rust
if tui.rig_connected {
    right_spans.push(Span::styled("RIG", Style::default().fg(Color::Green)));
} else if st.radios.is_empty() && !tui.rig_connected {
    // Only show red if rig was configured (we detect this by checking rig_connected is false
    // but we don't track "configured"; omit if not connected and no radio state exists)
}
```

The comment explicitly acknowledges the limitation: there's no way to
distinguish "configured but disconnected" from "not configured."

### Fix plan

1. Add `configured` fields to `TuiState` for each hardware backend:
   ```rust
   pub rig_configured: bool,
   pub keyer_configured: bool,
   pub dxfeed_configured: bool,
   pub so2r_configured: bool,
   ```
2. In `logger-tui/src/main.rs`, set these during startup based on whether
   the corresponding TOML section was present (regardless of whether
   connection succeeded).
3. Update `status_bar.rs` to render tri-state per indicator:
   - **green** if `configured && connected`
   - **red** if `configured && !connected`
   - **omitted** if `!configured`
4. Consider per-radio rig indicators for SO2R: `R1` green/red, `R2` green/red
   instead of a single `RIG`.

### Files touched

- `logger-tui/src/main.rs` — TuiState field init
- `logger-tui/src/ui/status_bar.rs` — render logic

---

## Recommended Execution Order

1. **#2 — undo/redo persistence** (High correctness). Silent data divergence
   after restart is the worst kind of bug. Small, self-contained fix with
   a clear test.
2. **#3 — contest identity** (Medium/High correctness). Prevents a latent
   bug when contests with multiple schemas are added. Clean refactor.
3. **#4 — CSV parser** (Medium correctness). Edge case today, but a
   two-hour fix that pays for itself.
4. **#6 — connection indicators** (Low/Medium UX). Low risk, low priority
   polish. Bundle with SO2R UI work if other status-bar changes are made.
5. **#1 — multi-receiver rig note** (doc only). One-line comment + TOML
   note. Can be done opportunistically.
6. **#5 — workspace portability**. Tracked separately in
   `plans/monorepo.md`. Execute on its own schedule.

Tasks 1–3 can be done in a single afternoon. 4 is another short session.
6 is polish.
