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
- [x] **7 — Hardware I/O into dedicated per-device tasks (expanded 3.1)** —
  Five landed sub-steps:
  1. **Persist task** — `logger-runtime/src/persist_task.rs`. `LogAdapter`
     gained a `PersistBackend` enum with `None` / `Sync(SqliteOpSink)` /
     `Async(mpsc::UnboundedSender<Vec<StoredOp>>)` variants. Production
     uses Async via `open_db_async`, tests still use Sync via `open_db`
     so existing round-trip tests keep working. Unbounded channel —
     losing QSO writes is unacceptable; we'd rather grow memory.
  2. **SO2R task** — `logger-runtime/src/so2r_task.rs`. `So2rCmd::SetRx`
     handled by a dedicated task over a bounded channel. The task owns
     an `Arc<dyn So2rSwitch>` clone; the keyer task holds another clone
     for cross-radio CW. OTRSP's internal actor serializes commands, so
     concurrent access is safe.
  3. **Rig control task** — added inside the existing
     `spawn_rig_adapter` (one per rig, returns `mpsc::Sender<RigCmd>`).
     Runs alongside the existing poll and subscription tasks, sharing
     `Arc<dyn Rig>` — riglib-icom's internal actor serializes all
     commands, so poll+control concurrency is fine. The event loop
     no longer holds `Arc<dyn Rig>` at all, only the command sender.
  4. **Keyer task** — `logger-runtime/src/keyer_task.rs`. Owns
     `Box<dyn Keyer>` + `Option<Arc<dyn So2rSwitch>>`. Handles
     `KeyerCmd::Send { radio, text }` and `KeyerCmd::Abort`. Cross-radio
     CW switch is atomic inside the task: `abort → set_tx → sleep(50ms)
     → send_raw` — the 50 ms sleep covers OTRSP's relay propagation
     delay. `keyer.abort()` uses winkey's internal priority channel
     so it preempts pending sends within ~1 ms; no cooperative
     cancellation needed. `build_contest_message` (the wire-format
     encoder) moved into the task.
  5. **Error event variants** — `AppEvent::KeyerDisconnected`,
     `KeyerError`, `So2rDisconnected`, `So2rError`, `PersistError`.
     Tasks emit these via a cloned `app_tx: Sender<AppEvent>`. Reducer
     treats them as no-ops; event_loop updates status indicators and
     the error banner. Bootstrap now takes `app_tx` in `SessionConfig`
     and forwards it into `LogAdapter::open_db_async`.

  `dispatch_effects` now has **zero `.await`s on hardware**. Every
  previously-blocking effect (`CwSend`, `CwAbort`, `RigSet`,
  `So2rFocusChanged`, `LogInsert`) is a non-blocking `try_send` to the
  relevant task. `event_loop::run`'s signature dropped `keyer`,
  `so2r_switch`, and `rigs`, replacing them with the three command
  channel senders. Initial OTRSP routing moved from `event_loop::run`
  into `main.rs` (pre-loop, one-shot). All 28 tests still pass with no
  behavior change.

  **Expected validation** (to be re-measured with the same
  messing-around workload as the 2026-04-11 baseline): p99 of
  reduce+dispatch should drop from ~100 ms to <100 µs, max from
  ~124 ms to <1 ms. p50/p95 unchanged (already microsecond).

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

**Status:** Landed alongside the bandmap call-history mult downgrade.
`SpecScorer::classify_cache: Mutex<HashMap<(call, band, mode),
ClassifyVerdict>>` memoizes the combined `(is_dupe, is_new_mult)`
verdict. Both `is_dupe` and `would_be_new_mult` go through the cache;
`on_inserted` and `rebuild` call `clear_classify_cache()`. Loose-dupe
short-circuit in `is_dupe` is still outside the cache (it's a
constant-time set lookup on `loose_dupes`). Unit test:
`classify_cache_invalidates_on_insert` in `log_adapter.rs`.

*Historical — original plan:*

**Location:** `logger-runtime/src/scoring/spec_scorer.rs:289-333`

**Problem:** Every keystroke in the call field, the reducer asks
`is_dupe` + `would_be_new_mult`. Analytics additionally asks these for
every bandmap spot when computing `worked_calls` and `mult_calls`. For
100 bandmap spots × 1 keystroke, that's 100 calls to
`classify_call_lite_with_mode` on top of the 2 calls from the reducer.
If classify is a few hundred microseconds each, we've just burned
20+ ms on a single keystroke.

The bandmap call-history plan added a *second* classify call on the
same hot path (hypothetical-exchange eval for every unknown spot),
which raised the cache's expected payoff enough to justify landing it
now rather than waiting for measurement.

**Fix:** Memoize `classify_call_lite_with_mode` results by
`(call, band, mode)` inside `SpecScorer`. Invalidate on `on_inserted`
(clear the cache; the new QSO may have changed dupe status of that
call on other bands). A simple
`HashMap<(String, String, String), ClassifyResult>` is fine.

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

---

#### Investigation findings (2026-04-11)

Before locking in the design, I spawned three parallel exploration
agents against the source checkouts of the `winkey`, `riglib`, and
`otrsp` crates. Condensed findings:

**`winkey` — `/home/cbrown/.cargo/git/checkouts/winkey-0857052349758022/6587b70/`**

- `Keyer` trait at `src/keyer.rs:38` is `pub trait Keyer: Send + Sync`,
  object-safe, all methods `async fn` taking `&self`.
- Concrete `WinKeyer` (`src/winkeyer.rs:20`) holds no `Rc`/`RefCell`;
  `Box<dyn Keyer + Send + Sync + 'static>` constructs fine.
- The crate **already uses an internal actor pattern** (`src/io.rs`):
  a spawned task owns the serial port and drains two mpsc channels —
  `rt_tx` (priority, used for `abort()`) and `bg_tx` (used for
  `send_raw`/`send_message`). The internal select is `biased;` with RT
  ahead of BG.
- **Abort semantics** (the critical question): `keyer.abort()` queues
  `0x0A` (the WinKey wire-format abort byte) onto the RT channel. The
  IO task processes RT before BG. If a `send_raw` is currently awaiting
  `write_all` on the serial port, the abort waits for that write to
  finish — but for a single message that's <1 ms, imperceptible. Once
  `0x0A` lands on the wire, the WinKey **device** stops mid-character.
  **Net: treat `abort()` as effectively preemptive from clogger's
  perspective.** No fancy cancellation needed.
- `send_raw` is tokio-cancel-safe but **not** protocol-safe: dropping
  a `send_raw` future mid-flight could leave partial bytes on the wire
  and confuse the device. **Don't cancel send futures externally.**
- Events: `CharacterSent(char)`, `StatusChanged(busy)`, `Connected`,
  `Disconnected`, etc. already exposed via `subscribe()`.

**`riglib` — `/home/cbrown/.cargo/git/checkouts/riglib-4f4928004d11f4e1/0a37107/`**

- `Rig` trait at `crates/riglib-core/src/rig.rs:30` is
  `pub trait Rig: Send + Sync`, object-safe, methods `async fn` taking
  `&self`. Exposes `subscribe()` returning `broadcast::Receiver<RigEvent>`.
- `IcomRig` (`crates/riglib-icom/src/rig.rs:57`) also uses an actor
  pattern — the transport is owned exclusively by a spawned IO task
  (`crates/riglib-icom/src/io.rs`) that drains two mpsc channels
  (`rt_tx`/`bg_tx`, biased-select loop).
- **Concurrent-access answer:** poll and control are strictly
  serialized internally. Task A calling `rig.get_frequency` and task B
  calling `rig.set_frequency` at the same time just queue and execute
  in order. No serial-port interleaving, no data corruption. Proven in
  practice by clogger's existing `rig_adapter.rs`, which already runs a
  poll task + subscription task against `Arc<dyn Rig>` concurrently.
- Broadcast events for mode/frequency/PTT changes are emitted by the
  IO task whenever it reads a new value (whether the read was caused
  by a `get_*` call, a `set_*` call, or a rig-initiated transceive
  frame). So the poll task is not strictly necessary for broadcast —
  it's a heartbeat fallback.

**`otrsp` — `/home/cbrown/.cargo/git/checkouts/otrsp-4a15e45e103652b2/b8e150a/`**

- `So2rSwitch` trait at `src/switch.rs:31` is
  `pub trait So2rSwitch: Send + Sync`, object-safe.
- `OtrspDevice` (`src/device.rs:14`) also uses an actor pattern — a
  single IO task owns the serial port.
- **set_tx semantics — critical finding:** `set_tx` encodes
  `TX1\r` / `TX2\r` and calls `write_all`. It returns after the serial
  write completes (~1–5 ms at 9600 baud), **not after the physical
  relay has finished switching**. OTRSP has no ack for write commands.
  Real SO2R hardware (YCCC SO2R+, microHAM MK2R+, SO2RDuino) has
  **relay propagation delays of 10–50 ms** after receiving the byte.
  **This breaks the naive "serialize abort → set_tx → send_cw" plan:
  serialization gets you serial-port ordering, but not relay-switched
  ordering.** An explicit sleep is required between set_tx completion
  and starting CW on the new radio. Recommended guardrail: **50 ms**.
- Events: `TxChanged`, `RxChanged`, `Connected`, `Disconnected`.

---

#### Design decisions (resolved)

**D1. Send/Sync bounds.** ✅ No obstacle. All three trait objects work
as `Arc<dyn T + Send + Sync + 'static>`. Multiple tasks can share an
`Arc<dyn Rig>` or `Arc<dyn So2rSwitch>` — the crates serialize
internally.

**D2. One task per rig, merged poll + control + subscription.** Each
rig gets a dedicated task that owns `Arc<dyn Rig>` and runs a
`tokio::select!` over:

- `poll_interval.tick()` → call `get_frequency`/`get_mode`/`get_passband`
- `cmd_rx.recv()` → `RigCmd::SetFrequency(u64)` etc.
- `events.recv()` → broadcast events from the rig → forward to
  `app_tx` as `AppEvent::RigStatus`

Replaces both the current subscription task and the current poll task
in `rig_adapter.rs`. Cleaner, fewer moving parts.

**D3. Keyer task owns `Arc<dyn So2rSwitch>` for cross-radio CW.** The
cross-radio CW switch sequence (abort → set_tx → wait → send) is the
only place where keyer and OTRSP need coordinated ordering. Instead of
trying to sequence two independent tasks via channels, **give the
keyer task direct access to the OTRSP handle** and let it execute the
sequence internally, including the 50 ms relay-settle delay:

```text
KeyerTask::SendCw { target_radio, text }:
    if target_radio != current_tx_radio:
        keyer.abort().await          // <1ms via internal rt_tx
        so2r.set_tx(target).await    // ~5ms serial write
        sleep(50ms)                  // wait for relay to settle
        current_tx_radio = target
    keyer.send_raw(build_contest_message(text)).await
```

Both the keyer task and the so2r task share the same
`Arc<dyn So2rSwitch>`. Concurrent access is safe because OTRSP's
internal actor serializes commands. The keyer task is the sole source
of `set_tx` calls; the so2r task handles `set_rx` (focus changes) and
any future OTRSP commands that aren't CW-routing.

**D4. Abort is effectively preemptive.** Use `keyer.abort().await`
directly — the winkey crate's internal priority channel makes it fire
within ~1 ms. No cooperative cancellation, no separate abort channel,
no future-cancellation tricks. When the user presses Esc,
`dispatch_effects` sends `KeyerCmd::Abort` via `try_send`; the keyer
task picks it up next (after at most a sub-millisecond wait) and calls
the crate's abort. Good enough.

---

#### Concrete task layout

| Task | Count | Owns | Reads from | Writes to |
|---|---|---|---|---|
| `keyer_task` | 1 | `Box<dyn Keyer>`, `Arc<dyn So2rSwitch>` (for TX routing) | `mpsc<KeyerCmd>` | `mpsc<AppEvent>` (for errors, disconnects) |
| `rig_task` | 1 per rig | `Arc<dyn Rig>` | `mpsc<RigCmd>`, rig's broadcast receiver, poll timer | `mpsc<AppEvent>` (RigStatus, errors, disconnects) |
| `so2r_task` | 1 | `Arc<dyn So2rSwitch>` | `mpsc<So2rCmd>` | `mpsc<AppEvent>` (errors, disconnects) |
| `persist_task` | 1 | `SqliteOpSink` | `mpsc<Vec<LogOp>>` | `mpsc<AppEvent>` (persist errors only) |

Each task is a standard `tokio::spawn` with a `tokio::select!` loop
that terminates when its command channel closes (signalling shutdown).

---

#### Command protocol per device

```text
enum KeyerCmd {
    Send { radio: RadioId, text: String },   // full message, handles cross-radio
    Abort,                                    // immediate
    SetSpeed(u8),                             // future — not used today
}

enum RigCmd {
    SetFrequency(u64),
    // SetMode(...) — future
}

enum So2rCmd {
    SetRx { radio: RadioId, mode: So2rRxMode },
    // SetTx is NOT here — keyer_task owns that
}

// persist_task just takes Vec<LogOp> directly
```

`KeyerCmd::Send` carries the target `radio` so the task can decide
whether it needs to do the cross-radio switch dance or just fire the
send. The current `dispatch_effects` code that computes the switch
(via `tui_state.tx_radio`) moves into the keyer task.

`tui_state.tx_radio` has to stay in sync — the event loop optimistically
updates it when dispatching `KeyerCmd::Send { radio, ... }`, before
the keyer task actually performs the switch. That's fine because:
- `tui_state.tx_radio` is only used to drive UI (the `TX` badge on the
  entry line).
- If the switch fails, the error event comes back and the UI recovers.

---

#### Backpressure policy per device

- **Keyer** — bounded `mpsc::channel(32)`. On `try_send` full: log
  warning + drop. Operator will hear the missed CW and can re-key. 32
  is overkill for CW (typical F-key macros are 10–30 chars, WinKey
  sends ~3 chars/sec at 40 WPM); 32 slots is ~5 minutes of backlog.
- **Rig control** — bounded `mpsc::channel(32)`. On full: log + drop.
  User hitting arrow keys too fast, not critical.
- **OTRSP** — bounded `mpsc::channel(32)`. On full: log + drop.
- **Persist** — **unbounded** (`mpsc::unbounded_channel`). Losing QSO
  records is unacceptable. Unbounded means no backpressure; if the
  SQLite task falls catastrophically behind, memory grows until we run
  out, which is strictly better than losing QSOs. Emit a warning if
  channel backlog exceeds (say) 128 items — that means something is
  very wrong with disk I/O.

---

#### New `AppEvent` variants (error reporting)

```text
AppEvent::KeyerDisconnected                  // channel/serial closed
AppEvent::KeyerError { message: String }     // transient send/abort failure
AppEvent::So2rDisconnected                   // already needed for OTRSP disconnects
AppEvent::So2rError { message: String }
AppEvent::PersistError { message: String }   // SQLite write failed
```

`RigDisconnected { radio: RadioId }` already exists and is unchanged.
The reducer treats all of these as no-ops (they don't affect the
contest state machine); the TUI layer reads them to update status bar
indicators and show error toasts. The existing `RigDisconnected`
pattern in `event_loop.rs:~140` is the template.

---

#### Test infrastructure changes

The golden-script runner in `logger-cli/src/runner.rs` currently
constructs a `FakeKeyer` and passes it into `dispatch_effects`
directly. After the refactor, `dispatch_effects` no longer touches the
keyer — it sends to `keyer_tx`. The tests need to assert against what
was sent, not what the keyer received.

Minimal change: in `runner.rs`, spawn a lightweight "collector" task
that drains `keyer_rx` into a `Vec<KeyerCmd>` that the test assertions
can read. Same pattern for the rig, so2r, and persist channels. The
existing `FakeKeyer`/`FakeRig` become collector-task internals rather
than trait implementations.

The 28 existing tests should keep passing with no behavior change — we
just rewire how assertions observe effects. Tests that check
`cw_sent_contains` now read from the collected `KeyerCmd` list instead
of the fake keyer's recorded calls.

---

#### Startup wiring changes (`main.rs`)

New shape:

```text
// Channels created before spawning hardware tasks
let (keyer_tx,   keyer_rx)   = mpsc::channel::<KeyerCmd>(32);
let (so2r_tx,    so2r_rx)    = mpsc::channel::<So2rCmd>(32);
let (persist_tx, persist_rx) = mpsc::unbounded_channel::<PersistOp>();
let mut rig_txs = HashMap::<RadioId, mpsc::Sender<RigCmd>>::new();

// Spawn per-rig tasks (replaces current rig_adapter subscription+poll tasks)
for rig_config in &config.rigs {
    let rig = spawn_rig_adapter(rig_config, /*...*/).await?;
    let (rig_tx, rig_rx) = mpsc::channel(32);
    rig_txs.insert(rig_config.radio_id, rig_tx);
    spawn_rig_task(rig_config.radio_id, rig, rig_rx, app_tx.clone());
}

// Spawn SO2R task
if let Some(so2r) = so2r_switch {
    let so2r_arc: Arc<dyn So2rSwitch> = Arc::from(so2r);
    spawn_so2r_task(Arc::clone(&so2r_arc), so2r_rx, app_tx.clone());
    // Keyer task also needs so2r_arc for cross-radio CW
    let keyer_box = keyer.unwrap();  // or None handling
    spawn_keyer_task(keyer_box, Some(so2r_arc), keyer_rx, app_tx.clone());
} else {
    if let Some(keyer_box) = keyer {
        spawn_keyer_task(keyer_box, None, keyer_rx, app_tx.clone());
    }
}

// Spawn persist task (if DB is configured)
if let Some(sink) = sqlite_sink {
    spawn_persist_task(sink, persist_rx, app_tx.clone());
}

// Event loop gets only the senders
run_event_loop(
    /*...state...*/,
    keyer_tx,
    rig_txs,
    so2r_tx,
    persist_tx,
    /*...*/,
).await
```

`run_event_loop` signature changes: drops `keyer: Option<Box<dyn Keyer>>`,
`rigs: HashMap<RadioId, Arc<dyn Rig>>`, `so2r_switch: Option<Box<dyn So2rSwitch>>`;
adds the sender handles above.

`LogAdapter::insert` changes: the in-memory `store.insert` stays
synchronous on the event loop; the `flush_pending_ops()` call becomes
`persist_tx.send(ops)` instead of awaiting `sink.append_ops(&ops)`.
The `SqliteOpSink` moves out of `LogAdapter` entirely — it's now owned
by the persist task.

---

#### Implementation order

Land incrementally, each step independently testable:

1. **`logger-runtime::spawn_persist_task`** + `LogAdapter` refactor to
   use the channel. Smallest blast radius — persist is already somewhat
   isolated. Verifies the channel/task pattern in a simple case.
2. **`logger-runtime::spawn_so2r_task`**, wire `Effect::So2rFocusChanged`
   through it. No cross-device coordination needed, single command type.
3. **`logger-runtime::spawn_rig_task`** (merged poll + control +
   subscription forwarding). Replaces most of the current
   `rig_adapter.rs`. Multi-step but scoped to one file.
4. **`logger-runtime::spawn_keyer_task`** (the hardest, because of
   cross-radio CW atomicity). The keyer task internally holds the
   `Arc<dyn So2rSwitch>` for TX routing. Once landed, the event loop
   has no remaining `.await`s on hardware.
5. **New `AppEvent` variants** + TUI error-toast plumbing. Can be
   done in parallel with any of 1–4.
6. **Re-run instrumented build**, compare p99/max against the
   baseline from 2026-04-11 04:29 UTC. Target: p99 < 100 µs, max < 1 ms.

Estimated total: **2–3 days**.

---

#### Open questions remaining

- **`keyer_task` shutdown coordination.** When the event loop exits, we
  should drain any remaining `KeyerCmd`s so the last CW message
  actually gets sent before the process quits. The channel close
  signal needs a small grace window. Design TBD when landing step 4.
- **Initial OTRSP routing at startup.** `main.rs` currently calls
  `set_tx`/`set_rx` before the event loop to put OTRSP in a known
  state. After the refactor, these become `so2r_tx.send(SetRx {...})`
  + keyer_task's responsibility for initial TX state. Startup ordering
  needs care — the so2r task must be running before the initial
  commands go out, or they pile up in the channel and fire later.
- **`FakeKeyer` abort behavior in tests.** The golden scripts have no
  tests for abort semantics today; if we add abort tests, the fake
  keyer task needs to honor them. Out of scope for this refactor.

---

#### Risk (post-investigation)

**Lower than the original write-up.** All three crates already use
actor patterns internally, so we're building on abstractions they
already support rather than forcing new concurrency semantics. The
main genuine risk is **cross-radio CW ordering** — getting the
abort/set_tx/sleep/send sequence wrong could cause the first dits of a
cross-radio message to route through the wrong radio. Mitigation:
unit-test the keyer task's sequence logic against a mock OTRSP that
records the order of its `set_tx` calls vs. the keyer's `send_raw`
calls, with timestamps.

Secondary risk: **channel saturation under pathological input**. A
stuck SQLite disk would grow the unbounded persist channel until OOM.
Mitigation: the warning threshold at 128 items and visible TUI alert.

---

#### Validation target

After landing, re-run the instrumented build against a similar
workload to the 2026-04-11 baseline (press F1/F2/F3 a bunch, log some
QSOs, cross-radio switch, abort) and expect:

- p99 of reduce+dispatch: **~100 ms → <100 µs** (three orders of
  magnitude)
- max of reduce+dispatch: **~124 ms → <1 ms**
- p50/p95: unchanged (already in the microsecond range)
- render/analytics: unchanged

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
