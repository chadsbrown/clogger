# Performance Improvements

## Bandmap Navigation Lag

### Problem
Ctrl-Up/Down bandmap navigation has noticeable lag. Each keystroke triggers
expensive computations that are unnecessary for selecting a known spot.

### Root Cause
The bandmap handler in `reducer.rs` (line ~244) runs this sequence on every
cursor move:

```
revalidate_after_edit(st, contest);              // 1st validation
recompute_feedback(st, dupe_checker, mult_checker);
apply_call_history(st, contest, call_history, scp);  // SCP + N+1 + history
revalidate_after_edit(st, contest);              // 2nd validation
```

The expensive operations:

1. **`scp.partial_matches()`** — prefix search across 50k+ callsign SCP database.
   Unnecessary during bandmap nav because the call is already complete/known.

2. **`scp.n_plus_one_matches()`** — edit-distance search against every entry in
   the SCP database. Most expensive single operation. Completely unnecessary for
   a known spot callsign.

3. **First `revalidate_after_edit()`** — validates after setting the call field,
   but `recompute_feedback` doesn't depend on validation state (it reads the raw
   call value). Can be removed.

Then in the event loop after `reduce()` returns, `recompute_worked_calls`,
`recompute_avail`, and `recompute_rate` also run on every event.

### Proposed Fixes

**Fix 1: Skip SCP during bandmap navigation**
- Pass a flag into `apply_call_history` to skip SCP/N+1 lookups, or
- Split `apply_call_history` into two functions: call history lookup vs SCP
- SCP suggestions are useless for a complete callsign from a spot

**Fix 2: Remove redundant first revalidation**
- Drop the first `revalidate_after_edit()` call in the bandmap handler
- The second call (after call history populates exchange fields) is sufficient
- Same optimization applies to the `Key::Equal` (SCP cycle) handler

**Fix 3: Evaluate event loop recomputations**
- `recompute_worked_calls` and `recompute_avail` iterate all bandmap spots and
  check dupe/mult for each — triggered on every event, not just bandmap changes
- Consider gating these behind a dirty flag or only running on relevant events

### Impact
Fixes 1 and 2 are straightforward and should eliminate the perceived lag.
Fix 3 is a broader optimization that would help with high spot counts.
