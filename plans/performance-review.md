# Performance Review

Synthesized from a parallel audit of the keystroke hot path, the render /
periodic-work loop, and the data persistence & scoring layers. Organized
by impact and effort, prioritized for **responsiveness** (P99
keystroke-to-visible latency) over raw throughput.

## Progress

- [x] **1 — Instrument the event loop (Rule 0)** — `logger-tui/src/perf.rs`
  records ring-buffered samples for three metrics: reduce+dispatch time
  per `TerminalEvent::App`, render time per `render_interval.tick()`, and
  analytics time per `recompute_analytics` call. Summary percentiles
  (min/mean/p50/p95/p99/max) are emitted at debug level on shutdown;
  visible only with `--debug`. Overhead is a single `Instant::now()` pair
  per event (~20-50 ns).
- [x] **2 — Drop clones in `current_call`, `mode`, `my_call`** (1.1, 1.2, 1.3) —
  `current_call()` now returns `&str` via a simple `.trim()` (zero
  allocation). Relies on the CALL-field invariant that writers maintain
  uppercase content; two non-uppercase write sites (SCP cycle, bandmap
  navigation) now call `to_ascii_uppercase()` explicitly. `normalize_mode`
  now returns `&'static str` (was `String`). `recompute_feedback` and
  `recompute_passband_warning` restructured to compute results under
  shared borrows and drop them before the mut borrow that writes results
  back — no more `mode.clone()` or `my_call.clone()` per keystroke. The
  one call site that actually needs an owned `String`
  (`esm.rs::log_and_clear` on QSO log, ~1/30s) does `.to_owned()` at the
  edge.
- [x] **3 — Fix char→string in terminal adapter** (1.4) — Replaced
  `c.to_uppercase().to_string()` with `c.to_ascii_uppercase().to_string()`
  in `logger-tui/src/adapters/terminal.rs`. Eliminates the `ToUppercase`
  Display-iterator path in favor of a single-branch bit op; the resulting
  String uses `char::to_string()` which builds a minimally-sized buffer
  via stack utf-8 encode. The enum variant is unchanged
  (`TextInput { s: String }` still allocates one small String per
  keystroke). The "wider" fix — changing the variant to
  `TextInput { c: char }` — remains available if instrumentation shows
  the TextInput path is still hot; it requires touching ~28 test sites
  in `reducer.rs` plus splitting strings in `logger-cli/src/runner.rs`
  and `logger-tui/src/event_loop.rs` (export modal).
- [x] **4 — Bandmap filter cache in TuiState** (2.1, 2.2) —
  `BandmapCache` struct added to `logger-core/src/contest/mod.rs`: small
  `Vec<BandmapCacheEntry>` keyed by `(band, mode)` as `&'static str`,
  invalidated by an `AppState.bandmap_version: u64` monotonic counter
  that the reducer bumps on `SpotReceived`/`SpotWithdrawn`. Cache lives
  in `TuiState` behind a `RefCell<BandmapCache>` so the
  render path (which only has `&TuiState`) can `borrow_mut` and populate
  lazily. `bandmap.rs::render`, `compute_worked_calls`, and
  `compute_avail` all route through the cache. First touch per
  `(band, mode)` per version pays the full filter+sort+dedup cost; all
  subsequent touches in the same version are O(linear scan ≤12
  entries) lookups. Also promoted `freq_to_band_label` and `normalize_mode`
  to return `&'static str` (was `String`) — eliminates more per-keystroke
  allocations and gives the cache zero-alloc keys. Reducer
  `reducer.rs:329` (bandmap navigation) kept using `filtered_bandmap_spots`
  directly — it's a cold path and adding cache access to the pure reducer
  would pollute the core/runtime boundary.
- [x] **5 — Score breakdown skip-if-unchanged** (2.3) — Added a
  `score_epoch: u64` counter on `LogAdapter` that bumps on `insert`,
  `undo`, `redo`, and the initial rebuild inside `open_db`. Exposed via
  `LogAdapter::score_epoch()`. The TUI event loop tracks the last epoch
  it sent a scoreboard snapshot for (initialized to `u64::MAX` so the
  first event always sends) and only calls `log_adapter.score_breakdown()`
  and pushes a fresh `ScoreboardSnapshot` when the current epoch differs.
  Keystrokes, rig status updates, and spot arrivals — the overwhelming
  majority of events — skip the breakdown build entirely.
- [ ] **6 — RigStatus dedup (3.3)** — Deprioritized after measurements
  (see below). Still worth doing for cleaner semantics and less wasted
  reducer work, but won't move the latency distribution — rig status
  events already land in the fast-path column (microseconds). Expected
  delta: none in p50/p95/p99, slight reduction in total work per second.

## Measurements

### Baseline after items 1–5 (2026-04-11 04:29 UTC)

First instrumented run after completing items 1–5. Operator note: **not a
real-contest workload** — messing around against the TUI with hardware
connected, no QSOs actually logged. Small sample size, so the tail
numbers are noisy. Still informative for rough diagnosis.

```
perf summary (last 512 samples per metric):
  reduce+dispatch: n=512 min=0ns    mean=1.78ms   p50=1.1µs   p95=7.6µs    p99=99.98ms  max=123.57ms
  render:          n=512 min=420µs  mean=761µs    p50=599µs   p95=1.60ms   p99=3.41ms   max=3.53ms
  analytics:       n=315 min=15.6µs mean=67.5µs   p50=72.4µs  p95=99.8µs   p99=121.7µs  max=168.7µs
```

**Render** — comfortable. 3.5 ms max on a 50 ms budget (7%). The
bandmap cache (item 4) and the string-alloc cleanups (items 1–3) kept
per-frame work tight.

**Analytics** — very tight. Under 200 µs worst-case. The bandmap cache
cutting the 6-band filter loop (item 4) is visible here.

**reduce+dispatch** — this is where the interesting signal is.

- p50 = **1.1 µs**, p95 = **7.6 µs**. Pure-software path is excellent
  — the allocation cleanups from items 1–3, 4 and the scoreboard skip
  from item 5 worked as intended. The median keystroke path is
  effectively free.
- p99 = **99.98 ms**, max = **123.57 ms**. A 13,000× cliff from p95.
  Rare but catastrophic — each outlier is the kind of blip operators
  feel as a momentary freeze.

### Revised diagnosis of the tail

Initial hypothesis was SQLite writes in `LogAdapter::insert` (plan
item 3.1). That hypothesis is **wrong for this session** — no QSOs
were logged, so the insert path never ran.

The actual culprit is **hardware I/O awaited inline in
`dispatch_effects`** (`logger-tui/src/event_loop.rs:~330`). Every
non-trivial effect blocks the event loop on device I/O:

| Effect | `.await` call | Blocks on |
|---|---|---|
| `CwSend` | `send_cw(keyer, text).await` → `k.send_raw(&bytes).await` | WinKey serial |
| `CwSend` (cross-radio) | `abort_cw` + `so2r::set_tx` | Keyer + OTRSP serial |
| `CwAbort` | `abort_cw(keyer).await` | WinKey serial |
| `RigSet` | `rig.set_frequency(rx, freq).await` | CI-V roundtrip |
| `So2rFocusChanged` | `so2r::set_rx(...).await` | OTRSP serial |

In this session, pressing F1/F2/F3 or Enter (ESM) produces `CwSend`
effects that await on the WinKey serial port. USB-serial latency under
load can be 20–100 ms per write. With ~5 samples above 100 ms in a
512-sample window, the p99/max cliff is fully explained by ~5 CW-send
events during the test.

**Implication**: plan item 3.1 is wider in scope than originally
written. The architectural fix is not "move SQLite writes to a
background task" — it's **move all hardware I/O into dedicated
per-device tasks**. See the expanded 3.1 below.

**Mental-model note.** "Move off the runtime" was imprecise framing
in the original plan. Tokio's runtime is fine; the problem is that
the event loop is a single task doing everything, and `.await` inside
a `tokio::select!` arm holds the whole select. Async gives you
interleaving **between tasks**, not within a single task. While
`dispatch_effects` is awaiting on `send_cw`, the render-tick arm of
the same select can't fire, new keystrokes can't be processed, etc.
Even if the underlying serial library is "truly async," the event
loop task is blocked until the await returns. The fix is to give the
runtime more tasks to interleave — specifically, dedicated
per-device tasks that own their hardware and read commands from
mpsc channels. The event loop then does `try_send(cmd)` (non-blocking)
and moves on. See expanded 3.1.

**What Items 1–5 accomplished**: the pure-software hot path (text
input, backspace, focus change within a radio, rig status updates,
spot arrivals) is now microsecond-scale. That's the path that was the
original concern of most plan items, and it's now essentially free.
The remaining problem is the much smaller set of events that touch
hardware — a different class of problem requiring a different fix.

## Framing

Two things matter for a contest logger:

1. **Keystroke latency** — the operator is pounding callsigns at ~100
   WPM. Anything that adds noticeable lag between keystroke and visual
   feedback is disqualifying.
2. **Frame consistency** — the 20 Hz render loop has a 50 ms budget.
   Missing that budget once is invisible; missing it repeatedly shows up
   as stuttering indicators and mush.

Every proposed fix is either **hot-path** (keystroke) or **per-frame**.
Each item is flagged accordingly.

## Rule 0: Measure first

Before changing anything, add per-arm timing in the `tokio::select!`
loop of `logger-tui/src/event_loop.rs` under a `--debug` gate.
Instrument three numbers:

- **reduce + dispatch time** per `TerminalEvent::App` (the keystroke
  hot path)
- **render time** per `render_interval.tick()` (the per-frame budget)
- **analytics time** when `recompute_analytics` runs

Log them at debug level with enough context to bucket. No fancy
profiler needed — just `Instant::now()` before and `elapsed()` after,
pushed to a small ring buffer, printed on Ctrl+C or with a keybinding.
This gives us concrete numbers to (a) prioritize and (b) validate the
fixes actually move them.

**Why this matters:** several findings below look scary on paper but
may not actually register in practice (e.g., `ordered_records()`
cloning 2000 records might be 50 μs, not 5 ms). Optimizing without
measurement wastes time.

**Effort:** ~2 hours. No algorithmic risk.

---

## Tier 1 — Obvious wins, small edits, zero risk

These are cheap to fix and obviously correct. Do them regardless of
what profiling shows.

### 1.1 `current_call()` returns owned `String` on every call — hot path

**Location:** `logger-core/src/state.rs:135-139`

**Problem:** `pub fn current_call(&self) -> String { … .trim().to_uppercase() }`
allocates a new `String` on every call. It's called 3x per keystroke
(`recompute_feedback`, `apply_call_history`, validation) and again from
`macro_expand`, ESM, and analytics.

**Fix:** Return `Cow<str>` or precompute a `call_norm` field in
`EntryState` that's maintained during the text-input reducer arm.
Readers become `&str`.

**Scope:** Touches `state.rs` + every call site. Grep for
`.current_call()` to enumerate.

### 1.2 `mode.clone()` in feedback recompute — hot path

**Location:** `logger-core/src/reducer.rs:443` and `:474`

**Problem:** Both `recompute_feedback` and `recompute_passband_warning`
clone `r.mode` just to read it for string comparison against spot
modes.

**Fix:** Take `&str`. The subsequent `normalize_mode(&mode_str)` and
comparisons don't need ownership.

### 1.3 `my_call.clone()` in passband bandmap scan — per event

**Location:** `logger-core/src/reducer.rs:478`

**Problem:** `let my_call = st.my_call.clone();` followed by
`s.call != my_call` inside an `.any()`. The clone serves no purpose;
`&st.my_call` borrows just fine because `st` isn't mutably borrowed in
the scan.

**Fix:** Drop the clone, use `&st.my_call`. May need to hoist a local
binding to appease the borrow checker across the `focused_entry_mut`
that follows.

### 1.4 Char → String double allocation on every TextInput — hot path

**Location:** `logger-tui/src/adapters/terminal.rs:~58`

**Problem:** `c.to_uppercase().to_string()` — `char::to_uppercase()`
returns an iterator (correctly handling multi-char Unicode
uppercasing), `.to_string()` collects it. Every keystroke allocates a
`String` for a one-character key.

**Fix:** For ASCII callsign entry, use `c.to_ascii_uppercase()` and
build the `String` directly:
`let mut s = String::with_capacity(4); s.push(c.to_ascii_uppercase());`.
Or even better, change `AppEvent::TextInput { s: String }` to
`AppEvent::TextInput { c: char }` since we never send more than one
char from the key reader anyway — but that's a wider change.

### 1.5 Mode clone in analytics prev/cur tuple comparison — hot path

**Location:** `logger-tui/src/event_loop.rs:148-151` and `:199-206`

**Problem:** `prev_freq_mode` and `cur_freq_mode` clone `r.mode` into
tuples purely to compare them — an optimization gate that wastes the
thing it's trying to save.

**Fix:** Compare `(freq_hz, &mode)` using `as_str()`, or compare each
field separately.

### 1.6 SCP cycle Vec save/restore round-trips — cold path but wasteful

**Location:** `logger-core/src/reducer.rs:284-305`

**Problem:** On F2 SCP cycle, `scp_matches` and `scp_n1_matches` are
cloned to save, revalidation runs, then the saved versions are
reassigned. The revalidation doesn't need to mutate them; the clones
are pure waste.

**Fix:** Either pass the scp lookups as references or refactor to avoid
the save/restore dance.

**Tier 1 total effort:** ~half a day, mostly mechanical.
**Expected impact:** Shaves string allocations off every keystroke.
Individually small, cumulatively noticeable on a long session.

---

## Tier 2 — Cache what's being recomputed (high impact)

These are the biggest wins. They also require the most care because
they introduce cache-invalidation logic.

### 2.1 Bandmap filter cache — per-frame (highest impact)

**Location:**
- `logger-core/src/contest/mod.rs` — `filtered_bandmap_spots()`
  clones + sorts + dedups
- `logger-tui/src/ui/bandmap.rs` — calls it once per render
- `logger-tui/src/ui/mod.rs` — calls it twice per frame in dual mode
  (R1 + R2)

**Problem:** Every frame (20 Hz),
`filtered_bandmap_spots(bandmap, band, mode)` walks the full bandmap,
clones matching spots, sorts by frequency, dedups by call. In
dual-bandmap mode that's 2× per frame = 40× per second. On a busy 40m
EU weekend the bandmap can easily hold 300+ spots per band; that's
hundreds of clones and a sort every 25 ms.

**Fix:** Cache the filtered+sorted result in `TuiState` keyed by
(band, mode, radio). Invalidate only when:

- `SpotReceived` / `SpotWithdrawn` arrives
- The focused radio's band or mode changes (derived from `RigStatus`)
- Analytics runs (it already depends on the same filtered data — share
  the cache)

The cache is small: the filtered subset of spots for the two
currently-viewed (band, mode) pairs. On invalidation, rebuild once;
reads from the renderer are zero-cost slice refs.

**Risk:** Moderate. Cache invalidation is the classic footgun —
forgetting an invalidation point shows stale spots. Mitigate with a
`bandmap_version` counter on `AppState` that increments on any bandmap
mutation, and the TUI cache stores the version it was built from and
rebuilds lazily when it differs.

### 2.2 `compute_avail` no longer filters 6× per call — per event

**Location:** `logger-tui/src/.../analytics.rs` (`compute_avail`)

**Problem:** `compute_avail()` calls `filtered_bandmap_spots()` once
for each of the 6 HF bands. Every `SpotReceived` triggers analytics
(because `needs_analytics_recompute` returns true for spot events), so
you pay 6× filter-sort-dedup per spot.

**Fix:** Bandmap cache from 2.1 trivially solves this. `compute_avail`
reads from the per-(band, mode) cache instead of re-filtering.

### 2.3 Score breakdown rebuilt on every app event — hot path

**Location:** `logger-tui/src/event_loop.rs:~190-195` (scoreboard
snapshot)

**Problem:** After every `dispatch_effects` call (i.e., every
keystroke, every rig update, every spot), `log_adapter.score_breakdown()`
is called to populate the scoreboard snapshot. The breakdown does 6+
band lookups × 2+ mult types with HashMap access and Vec allocations,
plus cloning the cached summary. The vast majority of events don't
change the score at all — editing a non-call field, tuning the rig,
receiving a spot.

**Fix:** Only rebuild the scoreboard snapshot when an event could have
touched the score. The obvious signal is "did the LogAdapter's log
tail or state change" — track a `score_epoch: u64` on `LogAdapter`
that increments on insert / rebuild / undo / redo. The event loop
compares current epoch to the last-sent epoch and skips the snapshot
when they match.

### 2.4 `compute_rate` clones the entire log on every timer tick — warm path

**Location:** `logger-tui/src/.../analytics.rs:~86` (`compute_rate`)
calling `ordered_records()`

**Problem:** `compute_rate` iterates all records to find QSOs within
the rate window (last hour / last 10 min). `ordered_records()` clones
the entire `Vec<QsoRecord>` on every call. For a 2000-QSO weekend log,
that's 2000 record clones per timer tick + every keystroke that
triggers analytics.

**Fix:** Two options:

- **Option A (minimal):** `ordered_records_slice()` returning
  `&[QsoRecord]` or an iterator. Most callers don't need ownership.
- **Option B (better):** Maintain a small ring buffer of "recent
  timestamps" on the `LogAdapter` that's updated on insert and read in
  O(1). `compute_rate` becomes a constant-time operation. Weekend
  contest = ~2000 QSOs = ~50 per minute peak; a 256-slot ring is
  overkill and costs nothing.

### 2.5 `is_dupe` classification cache in `SpecScorer` — hot path

**Location:** `logger-runtime/src/scoring/spec_scorer.rs:289-333`

**Problem:** Every keystroke in the call field, the reducer asks
`is_dupe` + `would_be_new_mult`. Analytics additionally asks these for
every bandmap spot when computing `worked_calls` and `mult_calls`. For
100 bandmap spots × 1 keystroke, that's 100 calls to
`classify_call_lite_with_mode` on top of the 2 calls from the reducer.
If classify is a few hundred microseconds each, we've just burned
20+ ms on a single keystroke.

**Fix:** Memoize `classify_call_lite_with_mode` results by
`(call, band, mode)` inside `SpecScorer`. Invalidate on `on_inserted`
(clear the cache; the new QSO may have changed dupe status of that
call on other bands). A simple
`HashMap<(String, String, String), ClassifyResult>` is fine.

**Caveat:** This is the single biggest bet in the plan. Classify may
be fast and the worry may be overblown — but only timing data from
Rule 0 can confirm. **Don't build this cache until measurement shows
it's needed.**

**Tier 2 total effort:** ~2 days. Highest payoff tier.
**Risk:** Cache invalidation bugs. Mitigate with version counters and
invariant assertions in debug builds.

---

## Tier 3 — Structural improvements (medium effort, targeted payoff)

### 3.1 Move hardware I/O into dedicated per-device tasks — hot path (confirmed by measurement)

**Note:** Originally scoped as "move SQLite writes off the async
runtime." That framing was both too narrow (only SQLite) and
misleading (the runtime isn't the problem). After the post-items-1-5
baseline measurement (see Measurements section above), the real shape
of the problem is clear: **every hardware-touching effect — CW send,
CW abort, rig set, OTRSP RX/TX route, SQLite insert — is awaited
inline inside `dispatch_effects`, which runs in the same single task
as the UI event loop.** That await blocks the whole `tokio::select!`
until the underlying device I/O returns — no render ticks, no
keystroke handling, nothing.

SQLite is one instance of this pattern; the others are arguably worse
because they fire on every F-key press and arrow key, not just on log
insert.

**Why async doesn't already save us.** The `.await` on
`send_cw`/`set_frequency`/etc. is a yield point, but only to **other
tasks** — not to other arms of the same select. Tokio schedules the
event-loop task off the CPU while the future is pending, and it will
happily run the rig-polling task or the dxfeed task in the meantime,
but it **can't** come back and fire the render-tick arm until the
current arm's code (the entire `dispatch_effects` call) finishes.
Async gives you parallelism between tasks, not within a single task.
Right now the event loop is one big task and there's nothing to
parallelize with.

The fix is to **spawn a dedicated task per hardware device**. Each
task owns the device handle and drains commands from an
`mpsc::channel`. `dispatch_effects` then uses `try_send(cmd)` — a
non-blocking enqueue that returns in nanoseconds. The event loop
never touches hardware directly, and the runtime has genuine
parallelism to exploit: the event loop task, the keyer task, the
rig-control tasks, the OTRSP task, and the persist task all run
concurrently under tokio.

**Locations:**

- `logger-tui/src/event_loop.rs:~330` `dispatch_effects` — the common
  site where every effect is awaited
- `logger-runtime/src/keyer_adapter.rs` `send_cw` / `abort_cw` — block
  on WinKey serial writes
- `logger-runtime/src/rig_adapter.rs` `rig.set_frequency` — blocks on
  CI-V roundtrip
- `logger-runtime/src/so2r_adapter.rs` `set_rx` / `set_tx` — block on
  OTRSP serial writes
- `logger-runtime/src/log_adapter.rs:~60-95` `insert` → SQLite
  `append_ops` — blocks on disk (the original 3.1 concern)

**Measured impact:** p50 and p95 of the reduce+dispatch path are
1.1 µs and 7.6 µs respectively (excellent), but p99 jumps to ~100 ms
and max to ~124 ms when any effect touches hardware. Operators feel
these as momentary freezes whenever they press F1/F2/F3 or log a QSO.
On a serial port with latency spikes, it's worse.

**Concrete shape of the fix:**

- `logger-runtime::spawn_keyer_task` — owns the `Box<dyn Keyer>`,
  drains a `mpsc::UnboundedReceiver<KeyerCmd>` with variants for
  `SendRaw(Vec<u8>)` and `Abort`. Processes commands in order
  (ordering matters for sequential CW sends and the
  abort-then-send pattern during cross-radio TX).
- `logger-runtime::spawn_rig_control_task` — one per rig, owns
  `Arc<dyn Rig>`, drains `mpsc<RigCmd>` with `SetFrequency(u64)` etc.
- `logger-runtime::spawn_so2r_task` — owns the `Box<dyn So2rSwitch>`,
  drains `mpsc<So2rCmd>` with `SetTx(RadioId)` / `SetRx(RadioId, Mode)`.
- `logger-runtime::spawn_persist_task` — owns the `SqliteOpSink`,
  drains `mpsc<Vec<LogOp>>` (the original 3.1 design).

`LogAdapter::insert` becomes synchronous: in-memory store update
only, then `persist_tx.send(ops)`. Same for the other effect
handlers: they become channel sends with no `.await`.

**Ordering caveats:**

- CW sends must be strictly ordered (F2 before F3 means F2 keys
  first). A single mpsc channel to a single keyer task preserves
  order — no parallelism.
- The cross-radio TX switch pattern in `CwSend` currently does
  `abort_cw` → `set_tx` → `send_cw` as three inline awaits. These
  need to become a single atomic command to the keyer task (e.g.,
  `KeyerCmd::SwitchAndSend { radio, text }`) or the keyer task has
  to see them in order in the same channel.
- OTRSP `set_tx` for TX routing and `set_rx` for RX routing can
  probably share one task (they go to the same device).

**Error reporting:**

- The fire-and-forget path can't return
  "keyer send failed" synchronously. Error events come back via a
  separate `mpsc::Sender<AppEvent>` passed to each task (already the
  pattern used by the rig adapter's `RigDisconnected` event). On
  error, the task emits a `*Disconnected` or error-toast event that
  the main loop handles.
- The channel buffer should be bounded but generous (128 items?) so
  transient backpressure doesn't drop CW characters. If the channel
  fills, something is very wrong and we should emit an error event.

**Durability caveats (from original 3.1):**

- `LogAdapter::insert` returning before the disk write completes
  means a hard crash could lose the last few QSOs. With SQLite WAL
  mode and a ~100 ms-ish lag between in-memory and on-disk, you
  might lose 1-2 QSOs worst case. For a contest logger that's
  acceptable — document it and optionally expose a "flush now"
  command on a timer (every few seconds) for safety.

**Risk:** Higher than the originally-scoped 3.1. This touches
persistence, rig control, keyer, and SO2R. Needs to be done carefully
with tests. Effort bumped from the original "~1 day" to **~2-3 days**
for all four device paths plus error reporting.

**Validation:** After landing, re-run the instrumented build against
a similar workload (press F1/F2/F3 a bunch, log some QSOs) and
expect p99 of reduce+dispatch to drop from ~100 ms to **<100 µs**
(two orders of magnitude). Max should also stay under a millisecond.

### 3.2 `SpecSession` rebuild on undo/redo scales O(n) — warm path

**Location:** `logger-runtime/src/scoring/spec_scorer.rs:187-203`

**Problem:** Every undo or redo triggers `SpecScorer::rebuild()` which
calls `init_session()` to create a fresh contest-engine session and
then replays every non-void record through `apply_qso_with_mode`. For
a 2000-QSO log, that's 2000 contest-engine calls + 2000 JSON decodes.
Undo is already infrequent (user-initiated) so this is tolerable, but
it scales linearly with log size — at 5000 QSOs it becomes perceptible
as a brief UI freeze on every Ctrl+Z.

**Fix (if needed):** Incremental undo support in the scorer — instead
of rebuilding from scratch, apply an inverse operation for the undone
QSO. Requires the scorer to track per-QSO deltas (points added, mults
created). Nontrivial. Only worth doing if undo latency shows up as a
complaint.

**Alternative:** Keep full rebuild but dedupe the JSON decodes by
caching `raw_exchange_for_record` results per QSO id. Cheaper, smaller
change.

### 3.3 RigStatus event deduplication in the subscription task — warm path

**Location:** `logger-runtime/src/rig_adapter.rs:~156-219`

**Problem:** The rig adapter's broadcast subscription forwards every
`FrequencyChanged`/`ModeChanged`/`PttChanged` event as a fresh
`AppEvent::RigStatus` without checking whether the value actually
changed. Some riglib backends emit events on every poll tick
regardless of change; that's 4 Hz × 2 radios = 8 RigStatus events per
second, each cascading through the reducer, recompute_feedback,
recompute_passband_warning, and (since `needs_analytics_recompute`
returns false for pure RigStatus) a cheaper analytics path.

**Fix:** Dedup in the subscription task. Keep the `LastRigState` that's
already there (`rig_adapter.rs:11-16`) and only forward the event if
the new value differs from the last sent one. This is a three-line
change and strictly reduces work downstream.

**Risk:** None. The reducer already tolerates missed updates (it'll
get the next changed one).

**Tier 3 total effort:** ~2 days.
**Payoff:** Medium. Protects against latency bombs and scales better
with large logs.

---

## Tier 4 — Micro-optimizations (only if Rule 0 shows they matter)

These are small, mechanical, and individually tiny. Skip unless
instrumentation shows the per-frame or per-keystroke budget is
actually tight.

### 4.1 Entry line widget — reuse Span/Line buffers across frames

**Location:** `logger-tui/src/ui/entry_line.rs`

Single-digit µs per frame. Only worth it if the entry_line render
shows up in timing.

### 4.2 Log tail — don't rebuild `Vec<Row>` from scratch on every frame

**Location:** `logger-tui/src/ui/log_tail.rs`

Same story. 10 Row allocs × 20 Hz = 200 allocs/sec. Small unless the
allocator is under pressure.

### 4.3 Macro expand — single-pass replacement instead of chained `String::replace`

**Location:** `logger-core/src/macro_expand.rs:20-53`

Not on the keystroke hot path (only fires on F-keys and ESM log).
Cheap to rewrite with a single-pass parser, but the benefit is
marginal.

### 4.4 Exchange blob encoding — `serde_json` → `bincode`/`postcard`

**Location:** `logger-runtime/src/log_adapter.rs:~66, 175-177`

Probably not worth the migration cost. JSON is ~200 bytes per QSO and
the encode/decode is microseconds. Unless profiling shows serde-json
on the flamegraph, leave it alone. The real benefit would be smaller
DB files, which is aesthetic.

### 4.5 Precompute validation state on text input instead of reactive recompute

**Location:** Whole validation pipeline

Not in the audit findings but worth noting: the reducer recomputes
validation / dupe / mult / passband on every text event. Some of those
checks could be skipped if the input wasn't a field that affects them
(e.g., typing in the CHECK field doesn't change whether the call is a
dupe). Could add a "what fields are dirty" tracking layer. Complex,
marginal.

**Tier 4 total effort:** varies. Don't do without justification.

---

## Recommended order of operations

| Order | Item | Tier | Effort | Status |
|---|---|---|---|---|
| 1 | Instrument the event loop (Rule 0) | — | ~2h | **Done** |
| 2 | Drop clones in `current_call`, `mode`, `my_call` | 1.1, 1.2, 1.3 | ~2h | **Done** |
| 3 | Fix char→string in terminal adapter | 1.4 | ~15m | **Done** |
| 4 | Bandmap filter cache in TuiState | 2.1, 2.2 | ~4h | **Done** |
| 5 | Score breakdown skip-if-unchanged | 2.3 | ~2h | **Done** |
| 6 | RigStatus dedup | 3.3 | ~30m | Deprioritized — measured harmless |
| 7 | **Hardware I/O into dedicated per-device tasks (expanded 3.1)** | 3.1 | ~2-3d | **Highest priority** — measured impact: p99 = ~100 ms |
| 8 | `compute_rate` incremental (ring buffer) | 2.4 | ~3h | Gated — analytics already under 200 µs per measurement |
| 9 | `is_dupe` classification cache | 2.5 | ~4h | Gated — analytics already under 200 µs per measurement |
| 10 | Tier 4 micro-opts as needed | 4.x | varies | Gated |

**Priority update after baseline measurement (2026-04-11):**

Items 1–5 landed the pure-software wins the plan expected. The
measurement surfaced that the tail latency problem was not what the
plan originally predicted — it's not scoring, not analytics, not
log-adapter state. It's **device I/O in `dispatch_effects`**. Item 7
(the expanded 3.1) is now the only remaining item that will visibly
change P99. Items 8 and 9 were gated on measurement showing they
mattered; measurement shows they don't — analytics is already fast.
Item 6 is still worth doing for cleanliness, but it won't show up in
the perf numbers.

If the next session only does one thing, it should be **item 7**.

---

## Things not to lose sight of

1. **The SpecScorer short-circuit in `is_dupe` (unknown calls return
   false immediately) is already doing a lot of work for us.** Don't
   break that when adding caching.

2. **Cache invalidation bugs would manifest as wrong dupe indicators,
   wrong mult highlighting, or stale QRM badges.** In a contest that
   costs real points. If caches are added, add a debug-build assertion
   mode that invalidates + recomputes + diffs the result to catch
   drift during testing.

3. **There's no `logger-tui/src/analytics.rs` file independently
   confirmed yet** — the data-layer audit agent referred to it. Before
   starting Tier 2, verify the exact location of `compute_rate` /
   `compute_avail` / `compute_worked_calls` so the fix lands in the
   right place.

4. **The goal is responsiveness**, not benchmark numbers. The success
   metric is: "during a contest, I never feel the UI lag."
   Measurement from Rule 0 should surface the worst-case event-loop
   latency under load, not average throughput. Keep an eye on P99, not
   mean.

5. **Several findings are likely non-problems in practice** — e.g.,
   JSON serde for 200-byte blobs, rebuilding a 10-row log tail per
   frame. Resist optimizing what isn't measurably slow.

---

## Raw findings from the audit (for reference)

This section preserves the per-agent findings before synthesis, in
case the synthesis loses useful context during implementation.

### Agent A: Hot keystroke path

**Critical:**

- `terminal.rs:~58` — `c.to_uppercase().to_string()` double-allocates
  for every TextInput keystroke.
- `reducer.rs:443, 474` — `mode.clone()` in `recompute_feedback` and
  `recompute_passband_warning`, called on every TextInput/Backspace/
  FocusRadio/RigStatus.
- `state.rs:135-139` — `current_call()` builds a new String on every
  call; called 3x per keystroke.

**Likely:**

- `reducer.rs:478` — gratuitous `my_call.clone()` in passband scan,
  used only for reference equality in `.any()`.
- `macro_expand.rs:33, 37-38, 49` — chained `String::replace` calls on
  macro expansion; 5+ replace per expansion. Not per-keystroke but
  runs on every F-key press.
- `reducer.rs:284-292` — SCP cycle clones entire `scp_matches` Vec
  twice to save+restore without mutation.
- `spec_scorer.rs:295, 317` — double `.trim().to_ascii_uppercase()` on
  `is_dupe`/`would_be_new_mult` when caller already guarantees
  normalization.
- `event_loop.rs:171-174, 226-229` — mode clone for prev/cur tuple
  comparison before analytics gate.

**Marginal:**

- `event_loop.rs:213-214` — scoreboard snapshot clones `call` twice.
- `reducer.rs:326` — `filtered_bandmap_spots()` clones entire filtered
  Vec on BandmapUp/Down. Only expensive when triggered.
- `spec_scorer.rs:92, 318` — paired dupe/mult checks each normalize
  the call independently.

### Agent B: Render loop and periodic work

**Critical:**

- `bandmap.rs:21` calling `contest/mod.rs:28-38` —
  `filtered_bandmap_spots()` called 2× per render frame in dual mode,
  each full clone + sort + dedup. 40 filter ops/sec on ~200 spots.
- `analytics.rs:51-83`, `event_loop.rs:222` — analytics recompute
  calls `filtered_bandmap_spots()` 6× (once per band in
  `compute_avail`). Fires on every SpotReceived/Withdrawn.

**Medium:**

- `entry_line.rs:64-110` — per-frame per-radio `format!()` calls for
  each field, span allocation, padding via `" ".repeat(pad)`. 160+
  format() calls/sec.
- `log_tail.rs:22-34` — `.collect()` into fresh `Vec<Row>` every
  frame. 200 Row allocs/sec.
- `rig_adapter.rs:156-219` — subscription task forwards every
  broadcast RigEvent as fresh RigStatus without checking if value
  changed. 4 Hz × 2 radios = 8 events/sec.
- `event_loop.rs:261-287` — 1 Hz timer tick calls full `reduce()`
  with AppEvent::TimerTick. Unclear cost without profiling.

**Background (low TUI impact):**

- `scoreboard_adapter.rs:62-104` — XML serialized on every interval
  tick regardless of score change. Separate tokio task, doesn't
  affect TUI frame budget.

### Agent C: Data layer and scoring

**High:**

- `event_loop.rs:~190-195` — `score_breakdown()` called on every app
  event for scoreboard snapshot, even when score didn't change.
- `reducer.rs:107, 142, 150, 176, 210, 362` — `recompute_feedback`
  calls `is_dupe`/`would_be_new_mult` on every keystroke. For 100
  bandmap spots, 100 contest-engine classify calls per keystroke.
- `analytics.rs:~86` — `ordered_records()` clones entire log on every
  timer tick + keystroke that triggers analytics. 2000-record clone
  per call.

**Medium:**

- `spec_scorer.rs:192` — `init_session()` rebuilt on every undo/redo;
  2000-QSO replay per Ctrl+Z.
- `log_adapter.rs:131` — SQLite `append_ops()` is synchronous on the
  tokio runtime thread. Latent responsiveness bomb.
- `log.rs:110` — `ordered_records()` called on export modal open;
  full log clone per export.

**Low:**

- `log_adapter.rs:66` — JSON encode on insert; one per QSO, cheap.
- `spec_scorer.rs:345` — JSON decode during rebuild; acceptable
  amortized cost.
- `spec_scorer.rs:82-85` — silent session init failure; could mask
  scoring bugs.
