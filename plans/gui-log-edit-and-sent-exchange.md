# Plan: Editable log pane + sent exchange display (GUI)

Status: **research captured, not ready to implement**. Open questions
below need answers first.

## Context

The GUI log pane (`logger-gui/src/panes/log.rs`) currently shows a
static list of the last 12 QSOs with columns `#`, `CALL`, `BAND`,
`MODE`, `FREQ`. Two related improvements are wanted:

1. **Show the sent exchange alongside the received exchange**, per QSO.
2. **Edit the other station's call and received exchange inline**,
   spreadsheet-style, with changes persisted to qsolog.

Firm constraints from discussion:

- **No modal.** A modal edit dialog was explicitly rejected as too slow
  for a contest operator. Editing happens in-place in the table.
- **Displayed data must be what was actually logged** — not
  reconstructed from current `AppState`. "It should always show what
  was actually logged, and not be derived from config."
- **Editable fields: only CALL + received exchange.** BAND, MODE,
  FREQ, timestamp, flags are read-only.
- **GUI-only feature.** TUI is out of scope — a TUI log editor would
  be impractical.
- One contest per db (so columns are homogeneous within a log).

## Investigation findings

### qsolog already supports edits

- `qsolog::QsoStore::patch(id, QsoPatch)` —
  `/home/cbrown/src/qsolog/src/core/store.rs:143-150`.
- `QsoPatch` is sparse: every field `Option<T>` at
  `/home/cbrown/src/qsolog/src/qso.rs:80-106`. Includes
  `callsign_raw`, `callsign_norm`, `exchange` (ExchangeBlob), `band`,
  `mode`, `freq_hz`, `ts_ms`, and flags.
- Patches automatically push the inverse onto the undo/redo stack.
- Persisted as `Op::Patch { id, patch, prev }` in the append-only
  events table — crash-safe, SQLite-journaled.
- Call and contest indices auto-update.
- Async variant on the handle:
  `/home/cbrown/src/qsolog/src/runtime/handle.rs:305-321`.

### LogAdapter needs a thin wrapper

Current methods at `logger-runtime/src/log_adapter.rs`:

| Method | Lines | Scorer | Contest history | score_epoch |
|---|---|---|---|---|
| `insert()` | 134-176 | `on_inserted()` incremental | `on_inserted()` incremental | bumped |
| `undo()` | 186-196 | full `rebuild()` | full `rebuild()` | bumped |
| `redo()` | 198-208 | full `rebuild()` | full `rebuild()` | bumped |

Need to add `patch(qso_id, QsoPatch)` mirroring `undo()` / `redo()`
(full rebuild — matches existing precedent; incremental edit could be
a future optimization). ~30 lines.

### Undo/redo bypasses the reducer — edit should too

Searched for `AppEvent::Undo`, `Effect::Undo`: **none exist**. Undo and
redo are called directly on `LogAdapter` from the UI without routing
through the shared reducer. Edit follows the same pattern: a
side-channel data mutation, not an `AppEvent`. This keeps state-machine
events separate from persistence mutations.

### Sent-exchange persistence approach

Today at `logger-core/src/entry/esm.rs:133-139`, the serial is appended
to the QSO draft:

```rust
if let Some(serial) = st.focused_entry().assigned_serial {
    for draft in &mut drafts {
        draft.exchange_pairs.push(("serial".to_string(),
                                   serial.to_string()));
    }
}
```

Extend this site to also append sent fields with a **`sent_`-prefixed
key convention**:

- CQWW: `("sent_rst", "599")`, `("sent_zone", "4")`
- MST: `("sent_name", "CHAD")`, `("sent_serial", "1")` (or keep the
  existing `"serial"` key for sent serial — decide)
- Sweeps: `("sent_prec", "A")`, `("sent_call", "N9UNX")`,
  `("sent_check", "85")`, `("sent_section", "IL")`
- etc., contest-aware.

Values drawn from `AppState` at log time: `my_call`, `my_zone`,
`rst_sent`, `my_exchange` map, `assigned_serial`.

**No qsolog schema change** — `exchange_pairs` serializes to an opaque
JSON blob; new keys are forward-compatible. Old records without
`sent_*` keys gracefully render as "unknown" (or leave the SENT
column blank for that row).

### Decoder path on display

- `logger-runtime::decode_exchange_pairs(&rec.exchange)` already exists
  and returns `Vec<(String, String)>`.
- Log pane separates `sent_*` keys from the rest; maps the remaining
  keys to the contest's received-field schema using the same
  `form_spec()` the entry pane uses.

### Validation reuse on edit

`logger-core/src/entry/validation.rs` drives entry-pane validation. On
an inline edit commit, run the same validation on the changed field
and reject invalid values with the same visual cue (border color).
No new validation code needed.

### Stable IDs already exist

`QsoRecord.id` is always populated and survives across renders
(`/home/cbrown/src/qsolog/src/qso.rs:26-27`). The log pane already
iterates `LogAdapter::ordered_records()` which returns the IDs inline.

## Proposed workflow (spreadsheet-style inline edit)

### Columns per contest

Derived from the contest's `form_spec()` — same shape the entry pane
uses.

- Common: `#`, `CALL` (editable), per-contest received fields
  (editable), `SENT` (read-only), `BAND`, `MODE`, `FREQ`.
- Example MST: `#`, `CALL`, `NAME`, `NR`, `SENT`, `BAND`, `MODE`, `FREQ`.
- Example CQWW: `#`, `CALL`, `RST`, `ZONE`, `SENT`, `BAND`, `MODE`, `FREQ`.
- `SENT` composes `sent_*` keys from the record (e.g. MST: `CHAD 1`;
  CQWW: `599 04`).

### Edit triggers and commit

- Click any editable cell → becomes a `text_input` pre-selected to
  current value.
- `Enter`: commit, stay on row.
- `Tab`: commit, move to next editable cell (wraps across rows).
- `Shift+Tab`: reverse.
- `Esc`: abandon edit, restore original value.
- Click elsewhere: commit (cancel is explicit via Esc).
- Per-cell commit — each successful edit is one `LogAdapter::patch()`
  call and one entry on the qsolog undo stack.

### Scrolling

- Log gets scrollable rows; default to bottom (newest visible).
- **Pin scroll** while a cell is being edited so an incoming new QSO
  doesn't yank the operator out of their edit.

### Keyboard-mode gating

- While a log-pane cell has focus, global F-key / Enter / Space
  bindings suspend — typing `5` into `NR` must not fire CW.
- Implementation: `App.log_edit_focus: Option<(QsoId, field_id)>`.
  The existing keyboard subscription in `logger-gui/src/main.rs` gates
  on this before dispatching F-keys to the reducer.

### Visual / UX cues

- Editable cells: hover highlight.
- Active edit cell: border + caret (reuse the focused-field styling
  vocabulary from the entry pane).
- Invalid value: red border (same cue as the entry pane, matches the
  "quiet down the focused-field highlight" change in recent work).
- Sent column: muted styling to mark read-only.

## Open questions (need answers before implementing)

1. **Click-away: commit or cancel?** Sketched as commit. Cancel is
   safer against accidental clicks; commit is the spreadsheet norm.
2. **F-keys during edit.**
   - *Dead* while in edit mode (safe, explicit mode).
   - *Commit-and-fire* (fast but risks CW misfires if the operator
     forgot they were in a cell).
3. **BAND / MODE / FREQ editability.** Treating as firm ("only call +
   received exchange"). Worth confirming for the case of a misnoted
   band from a band-change transient.
4. **New-QSO arrival during active edit.** Pin scroll (default
   sketched) — correct?
5. **Sent-exchange persistence shape.** Confirmed direction: extend
   `exchange_pairs` with `sent_*` keys. Sub-question: keep the
   existing `"serial"` key as-is (it's the sent serial) or rename to
   `"sent_serial"` for consistency? Renaming touches existing
   golden-script expectations and the `max_sent_serial()` scanner at
   `log_adapter.rs:258-271`. Keeping as-is is simpler.
6. **Keyboard-only navigation path.** Mouse click is the primary edit
   trigger. Also add keyboard entry (e.g. `Ctrl+L` focuses log, arrow
   keys navigate cells, Enter enters edit, Esc leaves log)? Matches
   the "keyboard-native" preference elsewhere but adds notable UI
   work.

## Size estimate

Rough: **2-3 focused days** of work for the whole thing.

Breakdown:

- Core sent-exchange persistence in `esm.rs` — 0.5 day
- `LogAdapter::patch()` wrapper — 0.5 day
- GUI log pane refactor: per-contest columns, SENT + RCVD, scrollable
  with pin, contest `form_spec()` integration — 1 day
- Inline edit state machine: focus tracking, per-cell text_input,
  commit semantics, Tab/Shift+Tab navigation, keyboard-mode gating,
  validation integration — 1 day
- Polish + golden-script coverage for the persistence path — 0.5 day

## Files involved

| File | Change |
|---|---|
| `logger-core/src/entry/esm.rs` | Append `sent_*` keys to `QsoDraft.exchange_pairs` at log time. Contest-aware. |
| `logger-runtime/src/log_adapter.rs` | Add `LogAdapter::patch()` mirroring `undo()` / `redo()` shape. |
| `logger-gui/src/panes/log.rs` | Contest-driven columns. Add SENT + RCVD. Scrollable rows. Inline text_input cells. Focus/commit state machine. |
| `logger-gui/src/main.rs` | `log_edit_focus` state field. Keyboard subscription gating. Hand-off to log pane. |
| `scripts/sent_exchange_persisted.json` (new) | Golden: verify `QsoRecord.exchange` contains `sent_*` keys after a CQWW QSO. |
| Possibly `scripts/inline_edit_patches_log.json` (new) | Golden: verify edit flow through the reducer/CLI harness. (CLI harness may not cover mouse flows — inline edit is hard to golden-script, may be manual-only.) |

**No changes to:** TUI, qsolog, SQLite schema, Cabrillo/ADIF export
paths, reducer, `AppEvent`/`Effect` enums.

## Out of scope for this plan

- Cabrillo export using persisted sent fields (natural follow-on).
- TUI inline edit (explicitly rejected).
- Editing of band / mode / freq / timestamp / flags.
- Visible undo/redo UI binding (backend supports it; no UI today;
  separate feature).
- Incremental scorer updates on patch (full rebuild is the pragmatic
  default, matching undo/redo).
