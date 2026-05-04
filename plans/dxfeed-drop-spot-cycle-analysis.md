# DX feed DROP/SPOT cycle — analysis

## Context

In the dxfeed pane, operators routinely see a `DROP` for a callsign immediately
followed by a `SPOT` for what looks like the same call on the same frequency,
even for stations that are actively running and being heavily spotted. This
analysis is a record of *why* that happens — it is not (yet) a fix proposal.

The investigation covered: clogger's dxfeed adapter, the dxfeed crate's
aggregator and spot table, the filter pipeline, and the GUI's activity-row
rendering.

## Quick mental model (corrected)

The dxfeed library, configured by clogger, is the source of every `SPOT` and
`DROP` line in the pane:

- A cluster line that lands on a **known** `(call, band, mode, freq_bucket)`
  refreshes `last_seen` in the aggregator's spot table and (by default)
  silently updates state — no event emitted.
- A cluster line that lands on a **new** key emits `SpotEventKind::New`.
- The aggregator's periodic `tick` evicts entries whose `last_seen` is older
  than `spot_ttl` (default **900 s**) and emits `SpotEventKind::Withdraw`.
- A `SpotEventKind::Withdraw` is also emitted as part of **QSY detection**
  whenever a `New` arrives for a different freq bucket of the same
  `(call, band, mode)` that is already known.

Clogger's adapter (`logger-runtime/src/dxfeed_adapter.rs:97-117`) collapses
`New | Update` into `AppEvent::SpotReceived` and `Withdraw` into
`AppEvent::SpotWithdrawn`. The GUI renders these as `SPOT` and `DROP` rows
(`logger-gui/src/panes/dxfeed.rs:103-104`).

## What does *not* explain it

Theories ruled out by reading the dxfeed source:

- **`max_age_secs` (filter)**: this is purely an *ingestion gate*
  (`dxfeed/src/filter/evaluate.rs:35-38`). It rejects cluster lines whose
  *timestamp* is already older than the threshold. It does not drive
  eviction of state already in the aggregator.
- **`lookback_window_secs` (skimmer quality engine)**: governs the window
  for counting skimmer corroborations
  (`dxfeed/src/skimmer/quality.rs:253-273`). Does not produce `Withdraw`.
- **Filter rejection causing TTL to lapse**: `spot_table.ingest()` runs
  *before* the skimmer gating and the general filter
  (`dxfeed/src/aggregator/core.rs:216` vs. `:262, :274`). `last_seen` is
  always refreshed, even when the line is later filtered out. So a
  rejected line still keeps the spot alive in the table.
- **Aggregator key including spotter / source**: the SpotKey is just
  `(dx_call_norm, band, mode, freq_bucket_hz)`
  (`dxfeed/src/model.rs:29-36`). Different spotters reporting the same
  `(call, band, mode, bucket)` map to one entry.

## What likely *does* explain it

**QSY detection between adjacent CW frequency buckets.**

Key facts:

- The default CW freq bucket is **10 Hz**
  (`dxfeed/src/aggregator/core.rs:42`).
- Different CW skimmers routinely disagree by 20–50 Hz on the same signal
  due to per-skimmer calibration / measurement jitter.
- QSY detection (`dxfeed/src/aggregator/core.rs:285-308`) fires whenever a
  `New` is emitted for a `(call, band, mode)` that already had an
  emitted entry at a *different* freq bucket — it withdraws the old key,
  then emits `New` for the new key, in that order.

So skimmer A reports W1AW @ 14025.000 Hz, skimmer B reports the same
signal at 14025.030 Hz → buckets `14025000` and `14025030` → continuous
ping-pong:

```
T   : New 14025000        -> SPOT
T+s : Withdraw 14025000 + New 14025030  -> DROP, SPOT
T+s': Withdraw 14025030 + New 14025000  -> DROP, SPOT
...
```

This matches the observed pattern: DROP and SPOT arrive **as a pair**
(not as a long gap), and it happens to **well-corroborated** stations
(more skimmers ⇒ more chance of disagreement) rather than to quiet ones.

## Why DROP and SPOT visually appear at "the same frequency"

Even when the underlying buckets differ by 30 Hz, the GUI display can
make them look identical:

1. `AppEvent::SpotWithdrawn` only carries the **callsign** — the adapter
   throws away `freq_hz` from the underlying dxfeed event
   (`logger-runtime/src/dxfeed_adapter.rs:110-116`).
2. The GUI reconstructs the freq for the DROP row by **looking up the
   call in the bandmap** (`logger-gui/src/main.rs:875-888`).
3. The `Withdraw` arrives **before** the new `New` (per the QSY emission
   order). So the bandmap still holds the old freq when the DROP row is
   built — the row gets the *correct* old freq.
4. The freq column then renders both rows at whatever precision the GUI
   uses; a 30 Hz delta on a CW spot may be invisible to the operator.

End result: an operator sees `DROP W1AW 14025.0` followed by
`SPOT W1AW 14025.0` and concludes "same call, same frequency" — but in
the aggregator they were two different keys.

## Confidence

- That **filter rejection is not the cause** — high (read directly
  from `aggregator/core.rs` and `spot_table.rs`).
- That **QSY-bucket-jitter is the dominant cause** — moderate. The
  mechanism is fully consistent with the observed behavior, but I have
  not captured a real DROP/SPOT pair from a running session and shown
  the underlying freqs differ. Verification path: see below.

## Verification path (not yet done)

Add a one-line debug log in `logger-runtime/src/dxfeed_adapter.rs`,
just above the existing `tx.send(...)` calls in the `Spot` branches:

```rust
// Spot branch (New | Update)
tracing::debug!(
    call = %spot_event.spot.dx_call,
    freq = spot_event.spot.freq_hz,
    kind = ?spot_event.kind,
    "dxfeed event"
);
// Withdraw branch
tracing::debug!(
    call = %spot_event.spot.dx_call,
    freq = spot_event.spot.freq_hz,
    "dxfeed withdraw"
);
```

Run with `RUST_LOG=logger_runtime=debug` during a contest, grep for a
call known to have cycled, and check whether consecutive
`Withdraw` / `New` lines for the same call are at freqs differing by
10–50 Hz (confirms QSY-bucket-jitter) or are literally identical
(falsifies the theory and indicates something else is at play).

## Possible mitigations (for later, not part of this analysis)

- **Widen `cw_bucket_hz`** in the dxfeed `AggregatorConfig`
  (e.g., 50–100 Hz) so skimmer jitter falls inside one bucket. Tradeoff:
  loses the ability to distinguish stations that genuinely sit
  ~30–50 Hz apart on CW.
- **GUI-side coalescing**: when an `AppEvent::SpotReceived` arrives for
  a `call` within N seconds of an `AppEvent::SpotWithdrawn` for the
  same `call`, suppress the prior DROP row instead of appending a new
  SPOT row. Cosmetic only — does not change underlying state.
- **Adapter-side coalescing**: turn a `Withdraw` immediately followed
  by a `New` for the same `call` into a single `Move`/`Update`-style
  `AppEvent`. Would also need the SpotWithdrawn event to carry freq so
  consumers can disambiguate.

## Files referenced

- `logger-runtime/src/dxfeed_adapter.rs:97-117` — event translation
- `logger-gui/src/main.rs:865-895` — `record_dx_activity`, DROP freq lookup
- `logger-gui/src/panes/dxfeed.rs:100-125` — DROP/SPOT row rendering
- `dxfeed/src/aggregator/core.rs:42` — `cw_bucket_hz: 10` default
- `dxfeed/src/aggregator/core.rs:68` — `emit_updates: false` default
- `dxfeed/src/aggregator/core.rs:94` — `spot_ttl: 900s` default
- `dxfeed/src/aggregator/core.rs:216` — `spot_table.ingest` (pre-filter)
- `dxfeed/src/aggregator/core.rs:262, :274` — gating / filter rejection
- `dxfeed/src/aggregator/core.rs:285-308` — QSY detection
- `dxfeed/src/aggregator/spot_table.rs:155-193` — `ingest` and `last_seen`
- `dxfeed/src/aggregator/spot_table.rs:257-281` — `evict_expired`
- `dxfeed/src/model.rs:29-36` — `SpotKey` shape
- `dxfeed/src/freq.rs:130-152` — `freq_bucket` formula
- `dxfeed/src/filter/evaluate.rs:35-38` — `max_age_secs` ingestion gate
- `clogger/filter.json` — current operator filter config
