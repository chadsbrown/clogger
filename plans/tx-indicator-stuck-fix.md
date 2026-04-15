# Fix: Stuck TX indicator (winkey watch channel)

**Status:** Planned. Do NOT implement before CWT contest (2026-04-15).
**Scope:** Narrow bug fix only. Does not include broader rig-sourced TX indicator work (see "Deferred / out of scope" below).

## The bug

Reported scenario: before a contest, the "TX" indicator was visible for one
radio in the TUI even though the radio was not transmitting. The indicator
stayed on with no ongoing transmission and no clear trigger.

## Root cause analysis

`TuiState.cw_transmitting: bool` (logger-tui/src/main.rs:41) drives the "TX"
badge rendered at logger-tui/src/ui/entry_line.rs:39. It is set and cleared
in exactly two places today:

- **Set to `true`** at logger-tui/src/event_loop.rs:371 when `Effect::CwSend`
  is dispatched. This is optimistic — it fires immediately on keypress,
  before the keyer task has started transmitting.
- **Set to `false`** at logger-tui/src/event_loop.rs:293-295 when a
  `KeyerEvent::StatusChanged { busy: false }` arrives from winkey's
  broadcast channel.

The clearing path is fragile in several ways, but the most likely cause of
the reported stuck-on behavior is:

**Broadcast channel `Lagged` silently drops the terminal `busy=false` event.**
`recv_keyer_event` at logger-tui/src/event_loop.rs:560-571 swallows
`broadcast::RecvError::Lagged` on the keyer event stream and continues. At
high WPM the `CharacterSent` events share the same broadcast channel as
`StatusChanged` and can flood it, so the final `busy=false` transition can
be dropped. With no further status events arriving, TX stays lit forever.

Other contributing paths that do NOT clear `cw_transmitting`, each of which
can compound a stuck-on state:
- `Effect::CwAbort` (ESC) at event_loop.rs:439-442 — only enqueues
  `KeyerCmd::Abort`; relies on the subsequent winkey status byte.
- `KeyerDisconnected` at event_loop.rs:177-179 — only sets
  `keyer_connected = false` and shows a banner.
- `KeyerError` at event_loop.rs:181-183 — only shows a banner.

Per the safety requirement agreed during design: these handlers SHOULD NOT
clear TX optimistically. In SO2R operation the operator watches the TX
indicator on one radio to know when it's safe to key the other. A
false-negative (indicator clears while the keyer/radio is still
transmitting) is dangerous — it can cause overlapping transmissions,
double-logging, or RF conflicts with shared antennas/amps. A false-positive
(stuck on) is annoying but non-destructive — the operator can see it's
wrong and investigate. All of the above handlers are correct to leave TX
alone; only hardware-reported state should clear it.

## Recommendation

Replace winkey's broadcast-based `StatusChanged` event with a dedicated
`tokio::sync::watch::Sender<KeyerStatus>` inside winkey itself. Watch
channels have latest-value-wins semantics — consumers always see the
current state on their next poll, regardless of backlog. Structurally
immune to the `Lagged` drop that's causing the stuck-TX bug.

Keep the optimistic `cw_transmitting = true` on `Effect::CwSend` for
immediate keypress feedback. The watch signal is then the sole authority
for clearing and will re-assert `true` shortly after as hardware confirms.
`CwAbort`, `KeyerError`, `KeyerDisconnected` continue to not touch TX
state — the watch channel's latest value remains correct through all of
those paths.

Winkey is owned locally at `../winkey`, so the fix can be pushed down into
the source rather than adapted at the consumer end.

## Design

### Winkey changes (../winkey)

1. **Add a watch channel to `WinKeyer`** alongside the existing broadcast.
   In `winkey/src/winkeyer.rs:25` area, add:
   ```rust
   pub(crate) status_tx: watch::Sender<KeyerStatus>,
   ```
   Initialize with an idle `KeyerStatus::default()` or an
   explicit-idle value at `winkeyer.rs:341` where the broadcast channel is
   currently created.

2. **Extend the `Keyer` trait** (`winkey/src/keyer.rs:67` area) with:
   ```rust
   fn status(&self) -> watch::Receiver<KeyerStatus>;
   ```
   Implement on `WinKeyer` to return `self.status_tx.subscribe()`.

3. **Decode path**: in the IO task where status bytes are decoded to
   `KeyerStatus`, replace (or augment) the current broadcast
   `event_tx.send(KeyerEvent::StatusChanged(status))` with
   `status_tx.send_replace(status)`. Watch coalesces high-rate updates
   naturally — no buffer sizing concerns.

4. **Remove `KeyerEvent::StatusChanged` from the enum** at
   `winkey/src/event.rs:35-53`. One source of truth for status; prevents
   consumers from accidentally re-coupling to the broadcast path.

5. **Migrate the internal `wait_xoff` subscriber** at
   `winkey/src/winkeyer.rs:212-220` to use the watch channel. This is
   exactly the "wait for a specific state" use case watch is designed for.

6. **Update winkey examples/tests** that consumed
   `KeyerEvent::StatusChanged`:
   - `winkey/examples/tui.rs:704`
   - `winkey/examples/interactive.rs:53`
   - `winkey/examples/hwtest.rs:142,227`
   - `winkey/examples/monitor.rs:27`
   - `winkey/examples/contest_keyer.rs:33`
   - `winkey/tests/integration.rs:50,154`

   Most simplify: subscribe to the watch, `.changed().await`, `.borrow()`.

### Clogger changes

7. **`logger-runtime/src/lib.rs`**: re-export `KeyerStatus` and the watch
   receiver type if they aren't already reachable via the existing
   `pub use winkey::...` line at logger-runtime/src/lib.rs:50.

8. **`logger-tui/src/main.rs:225-228`**: today only subscribes to the
   keyer broadcast when `cw_echo` is enabled. Change to:
   - Always call `k.status()` to get a `watch::Receiver<KeyerStatus>` when
     a keyer is configured. This is the TX-indicator source.
   - Still call `k.subscribe()` only when `cw_echo` is enabled. The
     broadcast now carries only `CharacterSent` and other edge events.

9. **`logger-tui/src/event_loop.rs:30-31`**: replace the single
   `keyer_rx: Option<broadcast::Receiver<KeyerEvent>>` parameter with two:
   ```rust
   keyer_status_rx: Option<watch::Receiver<KeyerStatus>>,
   mut keyer_rx: Option<broadcast::Receiver<KeyerEvent>>,
   ```

10. **`logger-tui/src/event_loop.rs:283-298`**: split the current single
    keyer-event `select!` arm into two:
    - **Status arm**: `keyer_status_rx.changed()` → read
      `*keyer_status_rx.borrow()` → set
      `tui_state.cw_transmitting = status.busy`. Authoritative in both
      directions. This is what closes the bug.
    - **Edge-event arm**: `keyer_rx.recv()` → handle `CharacterSent` for
      echo as today. The `StatusChanged` match arm is deleted (variant
      removed in step 4).

11. **`logger-tui/src/event_loop.rs:560-571`**: delete `recv_keyer_event`.
    Add two helpers that return `std::future::pending()` when the
    receiver is `None`:
    ```rust
    async fn recv_keyer_status(rx: &mut Option<watch::Receiver<KeyerStatus>>) -> KeyerStatus { ... }
    async fn recv_keyer_event(rx: &mut Option<broadcast::Receiver<KeyerEvent>>) -> KeyerEvent { ... }
    ```

12. **Do NOT touch**:
    - `Effect::CwSend`'s optimistic `cw_transmitting = true` at
      event_loop.rs:371.
    - `Effect::CwAbort` handler at event_loop.rs:439-442.
    - `KeyerError` handler at event_loop.rs:181-183.
    - `KeyerDisconnected` handler at event_loop.rs:177-179.

    These remain conservative by design — hardware status (via the watch
    channel) is the sole authority for clearing TX.

## Semantics preserved

- **Immediate feedback**: the optimistic set on keypress means the TX
  badge lights on the frame the operator presses F1, as today.
- **Authoritative clear**: only a hardware-reported `busy=false` (via the
  watch channel, which cannot silently drop the transition) clears the
  badge.
- **Safety under failure**: disconnect, keyer error, abort, and any
  channel-overflow scenario leave the badge on rather than prematurely
  clearing it. A stuck-on badge is a visible failure mode the operator
  can recognize; a false-negative badge could cause them to key the other
  radio mid-transmission.

## Tradeoffs

- Watch coalescing collapses rapid busy→idle→busy→idle transitions into
  whatever the latest value is. For a TX indicator this is exactly what
  we want (current state, not event history). Nothing in clogger needs
  transition counts.
- Breaking change in winkey's public API: removing
  `KeyerEvent::StatusChanged` and adding `status()` to the trait. Since
  all winkey consumers are owned (winkey examples, winkey integration
  tests, clogger), this is acceptable. External consumers would need to
  migrate.

## Test surface

- **Winkey unit tests**: `KeyerStatus::from_status_byte` tests unchanged.
- **Winkey integration tests**: `tests/integration.rs:50,154` need small
  updates to read from the watch channel instead of pattern-matching on
  `KeyerEvent::StatusChanged`.
- **Clogger `logger-core` tests**: not affected. No dependency on keyer
  event types.
- **Clogger `logger-cli` golden tests**: not affected. Don't exercise the
  keyer event channel.
- **Clogger `logger-runtime` tests (9)**: audit for any dependency on
  `KeyerEvent::StatusChanged`. Expected: none — `StatusChanged` is only
  observed on the TUI side.

## Files touched

- `../winkey/src/keyer.rs` — trait method addition
- `../winkey/src/event.rs` — remove `StatusChanged` variant
- `../winkey/src/winkeyer.rs` — watch channel, decode path,
  `wait_xoff` migration
- `../winkey/examples/*.rs` — migrate consumers
- `../winkey/tests/integration.rs` — migrate test assertions
- `logger-runtime/src/lib.rs` — re-export (may be no-op if glob
  already covers)
- `logger-tui/src/main.rs` — acquire both receivers
- `logger-tui/src/event_loop.rs` — parameter change, split select arms,
  delete/replace helpers

## Deferred / out of scope

All of the following were discussed and explicitly deferred:

- **Rig-sourced TX indicator** (use `state.radios[n].is_ptt` from riglib
  CAT PTT events instead of winkey busy). Architecturally cleaner and
  mode-agnostic (would work for SSB, digital modes), but depends on
  significant riglib fixes:
  - Kenwood/Yaesu/Elecraft `enable_transceive()` doesn't propagate
    `ai_enabled` to the spawned IO task — AI frames are discarded even
    when the radio is pushing them.
  - Icom's `transceive.rs` lacks CI-V command `0x1C` (PTT) in its
    dispatch table — pushed PTT frames are ignored.
  - `logger-runtime/src/rig_adapter.rs` has the same broadcast-`Lagged`
    hole and doesn't poll `get_ptt()` as a backstop.
  - Only Flex (push-based SmartSDR status) works correctly today.
  - Even with all fixes, directly-wired footswitch and mic PTT buttons
    are generally not observable via CAT on any backend. N1MM+ has the
    same limitation — it's the state of the art.

- **Adding defensive clears** to `CwAbort`, `KeyerError`, or
  `KeyerDisconnected`. Rejected: winkey hardware may still be keying
  after a serial disconnect (has its own buffer). Clearing TX
  optimistically in these paths would create false-negatives.

- **TX-hold / relay-release visual state** (dimmed badge for ~50 ms
  after busy clears, to cover SO2R relay release window). Deemed
  unnecessary for current use.

- **Per-radio TX state** (replacing single-bool + `tx_radio` with
  `HashMap<RadioId, TxState>`). Not needed once watch channel fixes the
  ambiguity during cross-radio send.

## Context and references

- Discussion date: 2026-04-14
- Winkey is a local git checkout (`patch.crates-io` commented out in
  clogger/Cargo.toml:23-24; user has a working clone at `../winkey`).
- Riglib audit findings captured in conversation — useful if the
  deferred rig-sourced plan is picked up later. Key finding: AI mode is
  load-bearing for non-Flex backends, and the `enable_transceive()` API
  is currently broken such that AI frames are dropped even when the
  radio is emitting them.
- The existing cross-radio keyer sequence
  (`logger-runtime/src/keyer_task.rs:90-117`) with its 50 ms
  `SO2R_TX_RELAY_SETTLE` is correct and unaffected by this plan.
