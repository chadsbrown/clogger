# Bandmap mult classification using call history

**Status:** Planned. No urgency. Not a bug — an accuracy improvement for
contests where the mult is tied to a known exchange field (state QPs,
sections-based contests, zone-based contests on well-known DX).

## The gap

Today the bandmap classifies each spot as worked / mult / plain based on
`LogAdapter::is_dupe` and `is_new_mult`, which delegate to
`SpecScorer::is_dupe` / `would_be_new_mult`
(`logger-runtime/src/scoring/spec_scorer.rs:329, 368`), which delegate to
contest-engine's `classify_call_lite_with_mode`.

The **unknown-call branch** at spec_scorer.rs:374-379 returns `true`
optimistically when the call has never been logged (not in
`resolved_calls`). This means every unknown call in the bandmap is
rendered **green (new mult)** — which is right for DX contests where an
unknown call could be any zone/country, but wrong for contests where the
mult is derived from a specific exchange field (state/section/name/…)
and we have pre-contest knowledge of that field via the `.ch` file.

### Concrete example

Contest rule: working a state on a band is a mult (e.g., a state QP, MIQP,
etc.). Operator logs W9ABC on 40m who sends "IN". IN-on-40m is now in the
logged mult set. A new spot appears: **N9XYZ**. The `.ch` file lists
N9XYZ's state as "IN".

Current behavior: N9XYZ is unknown to `resolved_calls`, unknown-call
branch returns `true`, bandmap renders N9XYZ **green**. Misleading —
working N9XYZ almost certainly contributes no new mult on 40m.

Desired behavior: consult the call-history record for N9XYZ, find the
state field "IN", recognize IN-on-40m is already logged, render N9XYZ
as **worked-gray** (or a distinct intermediate color to flag "probable
non-mult per call history").

## Design

### Plumb `CallHistoryLookup` into `SpecScorer`

`SpecScorer` is constructed in `logger-runtime/src/scoring/mod.rs` and
consumed via the `LogAdapter`. It doesn't currently see the call-history
source.

- Extend `SpecScorer` to hold an `Option<Box<dyn CallHistoryLookup>>` (or
  `&dyn` borrowed from the `LogAdapter`'s caller; lifetime choice TBD —
  the reducer-side `CallHistoryLookup` trait lives in `logger-core`, and
  `LogAdapter` in `logger-runtime` already takes a
  `CallHistoryLookup` at construction, or could).
- Extend construction plumbing in `logger-runtime/src/bootstrap.rs` and
  wherever `LogAdapter::new` is called in `logger-tui/src/main.rs` and
  `logger-cli/src/runner.rs` to pass the call-history handle.

### Extend the unknown-call branch

In `spec_scorer.rs:368-391` (`would_be_new_mult`), when `resolved_calls`
doesn't contain the call:

1. Look up the call in call history. If no hit → keep today's optimistic
   `true`.
2. If hit → map the .ch column values to the contest's mult-relevant
   exchange fields (using the same `contest.history_field_mapping()` that
   the reducer uses for auto-population at `reducer.rs:684-689`).
3. Synthesize a candidate exchange. Ask contest-engine whether this
   candidate would grant any *new* mults given the current logged mult
   set.
4. Return `true` only if it would — otherwise `false`.

The contest-engine API needed: an equivalent of
`classify_call_lite_with_mode` that accepts a *hypothetical exchange*
rather than requiring the call to already be resolved from the log.
Check whether contest-engine already exposes this (it's likely needed
internally for the existing workflow); if not, this becomes a
contest-engine PR.

### Mirror the treatment in `is_dupe`

A call-history-based dupe prediction is weaker than a logged dupe —
operator intent is to log these — but we can still lean the classifier.
Simpler and safer to leave `is_dupe` alone, since the "already worked"
set should come from the log only. The mult side is where the green/gray
distinction matters visually.

### Optional: third color for "probable non-mult per call history"

Today the palette is white (plain) / green (mult) / dark gray (worked).
A call-history-downgraded spot is *not* worked — we haven't logged them
— but it's not a needed mult either. Options:

- Render as **white** (plain QSO point, not highlighted). Minimal color
  change, consistent with "it's a point, not a mult."
- Render as a distinct fourth color (yellow? dim green?) to convey
  "probably not a mult, but call-history data is uncertain." More
  information for the operator.

Recommend white for simplicity. If ambiguity matters, revisit after
initial implementation.

### Safety rule

Call-history data can be wrong (stale, rover, operator moved). Use it
only as a **downgrade** signal — never to upgrade an unknown call from
plain to green-mult. The current optimistic unknown→mult already covers
the "we don't know, assume valuable" case; call-history evidence that
a call is *probably not a mult* is a strict refinement on that.

## Scope / non-goals

- Not needed for DX contests where the mult isn't derivable from a `.ch`
  column (CQWW uses zone, which CQ-zone is sometimes in `.ch`, but
  country mults are rig-frequency-derived so this plan doesn't help
  CQWW country mults).
- Not a correctness bug. Current behavior is pessimistic — it over-highlights
  potential mults. Operator still makes the right call by working the
  station. The fix is an accuracy/noise improvement.
- No performance concern expected: `compute_worked_calls` runs on
  bandmap updates (not every render), iterates only the filtered
  per-band/per-mode spot list, and a hash-based call-history lookup per
  spot is cheap.

## Files touched (anticipated)

- `logger-runtime/src/scoring/spec_scorer.rs` — new unknown-call branch
  in `would_be_new_mult`
- `logger-runtime/src/scoring/mod.rs` — add call-history field to
  `SpecScorer`, thread through `ScoreBreakdownSink` or equivalent
- `logger-runtime/src/log_adapter.rs` — pass call-history handle into
  `SpecScorer`
- `logger-runtime/src/bootstrap.rs` — thread call-history into scoring
  construction
- `logger-tui/src/main.rs`, `logger-cli/src/runner.rs` — construction
  site updates
- Possibly `contest-engine` — if the hypothetical-exchange mult check
  isn't already exposed

## Test surface

- New unit test in `spec_scorer.rs`: state-QP contest, log one IN station,
  ask `would_be_new_mult` for an unresolved call whose `.ch` row says
  "State=IN", expect `false`.
- Snapshot/golden test at `logger-cli` level: a state-QP script that
  seeds call history, works one in-state mult, verifies a subsequent
  unresolved call is classified correctly.
- Regression check: DX contest with no call-history hit — behavior
  unchanged, unknown calls still return `true`.

## Context and references

- Discussion date: 2026-04-15
- Related work: `plans/contest-history-autopopulate.md` (scope: log data
  prioritized over `.ch` for exchange autopopulation in the entry line
  — separate concern from this plan).
- Related code:
  - `logger-runtime/src/scoring/spec_scorer.rs:368-391` — the unknown-call
    branch to extend
  - `logger-core/src/reducer.rs:684-689` — existing call-history lookup
    pattern to mirror
  - `logger-runtime/src/call_history.rs` — `.ch` file loader
  - `logger-tui/src/ui/bandmap.rs:61-69` — rendering site; may need
    palette update if fourth color is added
