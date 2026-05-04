# SO2R Routing Profiles

## Goal

Allow different SO2R operating styles, such as regular SO2R and 2BSIQ, without
putting radio or SO2R hardware commands into the macro system.

The macro system should remain responsible for contest messages. SO2R hardware
routing should be driven by application state and a small, testable routing
policy.

## Background

Direct SO2R commands in macros are risky:

- Timing-sensitive hardware operations can block or race with CW sends.
- Validation becomes difficult because macros become hardware-control scripts.
- Testing becomes broad and fragile.
- The current keyer task already owns the atomic TX switch sequence:
  abort in-flight CW, set TX, wait for relay settle, then send CW.

The existing design has a useful separation:

- Entry focus changes affect RX routing only.
- CW send target affects TX routing.
- TX routing is handled inside the keyer task so first dits do not go out the
  wrong radio during cross-radio sends.

## Proposed Shape

Introduce an SO2R routing policy/profile layer.

Instead of allowing macros to say "set RX/TX now", the runtime would evaluate
current app state, ESM state, focus, and transmit state, then decide the desired
SO2R routing.

Conceptually:

```rust
So2rPolicy::evaluate(&AppState, &So2rRuntimeState) -> DesiredSo2rState
```

Where `DesiredSo2rState` might include:

```rust
rx_radio: RadioId
rx_mode: So2rRxMode
```

TX should remain owned by CW send/keyer behavior. A policy may describe desired
operator behavior, but physical TX switching should still happen inside the
keyer task when a `CwSend { radio, text }` is processed.

## Example Profiles

Regular SO2R:

- RX follows focused radio.
- TX follows the CW target at send time.
- Focus changes do not preemptively move TX.

2BSIQ:

- TX still follows the CW target at send time.
- RX tends to favor the non-transmitting radio.
- If one radio is CQing and the other has a caller or exchange in progress, RX
  should favor the active exchange radio.
- Focus may be less important than which radio needs operator copy.

Possible config sketch:

```toml
[so2r]
port = "/dev/ttyUSB0"
profile = "regular"

[so2r.profiles.regular]
focus_rx = "focused"
tx = "cw_target"
rx_while_tx = "tx_radio"

[so2r.profiles.twobsiq]
focus_rx = "opposite_when_cqing"
tx = "cw_target"
rx_while_tx = "other_radio_stereo"
```

The exact names and knobs need design. Avoid a fully programmable profile
language unless there is a strong need; that risks recreating macro-controlled
hardware commands under another name.

## Needed State

The policy should not parse macro names or raw UI fields. It needs compact,
derived per-radio state, for example:

```rust
enum RadioEsmRole {
    Idle,
    Cqing,
    Calling,
    AwaitingExchange,
    ReadyToLog,
    Sending,
}
```

This role should be derived from reducer/ESM state and current in-flight CW
status.

## Suggested Implementation Path

1. Keep current behavior as built-in profile `regular`.
2. Add one built-in `twobsiq` profile in Rust, not as user scripting.
3. Derive a small per-radio ESM role suitable for policy evaluation.
4. Add focused reducer/runtime scenario tests:
   - focus changes,
   - CQ on one radio,
   - caller/exchange on the other radio,
   - cross-radio F-key sends,
   - in-flight CW while focus changes,
   - RX mode toggles.
5. Only then consider exposing a limited config surface for profile knobs.

## Constraints

- Do not put direct SO2R or radio commands into macros.
- Do not let RX routing block UI responsiveness.
- Do not move physical TX switching out of the keyer task.
- Preserve the current safe cross-radio sequence:
  abort, set TX, settle, send.
- Keep profile behavior deterministic and testable.

