# Block Dupes ESM Policy

Add an `EsmPolicy.block_dupes` flag (off by default) that makes ESM refuse
to advance when the focused entry's callsign is flagged as a dupe. The
operator sees the familiar red **DUPE** badge; pressing Enter (or
triggering ESM) produces an error beep instead of sending the exchange or
logging the QSO. F1 (CQ) still works because `is_dupe` is only set when a
callsign is present.

## Why This Is Cheap — Mechanics Already in Place

`EntryState.is_dupe` is already a cached boolean computed by
`reducer::recompute_feedback` (`logger-core/src/reducer.rs:430-457`) on every
event that could change its value — text entry, rig status, focus change.
It reads through the `DupeChecker` trait, which in a real session is wired
to the contest-engine-backed `SpecScorer`, so band/mode/global dupe
dimensions are honored per-spec automatically. The UI already renders it
(`logger-tui/src/ui/entry_line.rs:139`).

All this feature does is add one guard that reads the existing flag.

## Where the Gate Goes

Single insertion point at the top of `handle_esm` in
`logger-core/src/entry/esm.rs:9`, after the existing `esm_enabled` check:

```rust
if st.esm_policy.block_dupes && st.focused_entry().is_dupe {
    return vec![Effect::Beep { kind: BeepKind::Error }];
}
```

Walk through the three cases:

- **Empty call** → `is_dupe` is forced to `false` by `recompute_feedback`
  (`reducer.rs:436-441`), so the gate is skipped and F1/CQ still fires.
- **Dupe call in Run** → gate fires before `handle_run`, blocking both the
  exchange-send (first Enter) and the log (second Enter). Operator sees
  the red badge, can hit Esc to clear or key an "SRI QSO B4" macro
  manually.
- **Dupe call in S&P** → gate fires before `handle_sp`, so even the
  MYCALL-send step is blocked. You don't accidentally announce yourself to
  a station you've already worked.

Notably, the gate does *not* go in `log_and_clear`, even though that's the
last chance. Blocking only at log means the exchange has already been sent
over CW by then, which defeats the "don't work dupes" intent.

## Files Touched

| File | Change | Size |
|---|---|---|
| `logger-core/src/state.rs` | Add `block_dupes: bool` to `EsmPolicy`, default `false` | ~2 lines |
| `logger-core/src/entry/esm.rs` | Add the gate at top of `handle_esm` | ~3 lines |
| `logger-core/src/reducer.rs` | Unit test: dupe call + policy → Enter produces beep, no `LogInsert` | ~40 lines |
| `logger-cli/src/script.rs` | Add `block_dupes: Option<bool>` to `EsmPolicyConfig` | ~1 line |
| `logger-cli/src/runner.rs` | Apply it to `st.esm_policy` | ~3 lines |
| `scripts/ss_block_dupes.json` | Golden: log K5ZD, type K5ZD again, Enter → no second QSO, beep_error_count=1 | ~40 lines |
| `logger-tui/src/config.rs` | Accept `block_dupes` in the TUI config (flat field or `[esm]` section) | ~5 lines |
| `logger-runtime/src/bootstrap.rs` | Apply config → `EsmPolicy` (good moment to also surface `run_two_step`, which the TUI currently can't configure either) | ~5 lines |
| `logger-tui.example.toml` | Document the option | ~3 lines |

Roughly 80 lines of real code + 40 of tests. Half a day of work, most of
it tests.

## Design Questions to Settle Before Implementing

1. **Naming.** `block_dupes` matches how operators describe it.
   Alternatives: `refuse_dupes`, `prevent_dupe_qsos`.

2. **Per-station config or also a runtime toggle?** Should there be a
   keyboard shortcut to flip it mid-contest (the "I really do want to log
   this one" case)? Default recommendation: config-file-only, no runtime
   toggle. An escape hatch like Shift+Enter adds complexity and invites
   muscle-memory errors. If you really want to work a dupe, you can clear
   the entry or edit the call.

3. **Does the gate also block F3 (TU) when pressed directly?** Recommend
   no — function-key macros are just "send this CW" and don't themselves
   create log entries. Gating only ESM cleanly separates "refuse to log a
   dupe" from "refuse to transmit to a dupe." The stronger behavior would
   be a separate feature.

4. **Should this tie into `EsmPolicy` or live as a separate top-level
   `AppState` field?** `EsmPolicy` is the right home — it's policy about
   how the ESM state machine behaves, and the struct is already threaded
   through reducer and tests.

5. **Default value.** Must be `false` for backwards compatibility with
   every existing test and config. New users opt in.

6. **Test runner scope.** Currently `EsmPolicyConfig` only exposes
   `run_two_step`. Worth adding both `block_dupes` and wiring up any other
   `EsmPolicy` fields while we're there, so we don't have to revisit this
   for the next policy knob.

## Risks

Low. The change is read-only on `is_dupe` (no new state, no new async
path, no new scorer logic), gated behind a default-off flag, with a
trivially testable behavior via the existing golden-script harness. The
only thing to be careful about is verifying the early-return for
empty-call-in-run happens before the gate would wrongly fire — and it
does, because `is_dupe` is forced to false whenever the call is empty, so
the gate is a no-op in that case regardless of ordering.
