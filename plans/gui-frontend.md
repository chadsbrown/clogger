# Clogger GUI — Initial Discussion & Planning

> **Plan-file location**: plan-mode constrained me to this path. After
> approval, this should be moved to
> `/home/cbrown/src/clogger/plans/gui-frontend.md` to live alongside the
> project's other planning docs.

---

## Context

Clogger today has a TUI (`logger-tui`) that's feature-complete for actual
contesting (CQWW, CWT, Sweeps, MST, NAQP, ARRL DX, NS Sprint), and a
headless CLI runner (`logger-cli`) used for golden tests. The TUI inherits
terminal constraints: character-cell layout, no real graphics, weak mouse
semantics, limited typography, ASCII-bell audio.

A GUI opens space the TUI can't reach: a true frequency-axis bandmap with
mouse-tunable spots, font scaling for the contesting demographic,
multi-monitor-friendly window arrangements, and a path to broader adoption
since most contesters expect N1MM-style desktop ergonomics.

The hard constraint is that contesters live by keyboard muscle memory. The
GUI must remain keyboard-native; mouse is a bonus, never primary.

---

## Decisions captured (this session)

| Decision | Choice |
|---|---|
| GUI vs. TUI relationship | **Coexist as separate frontends** sharing `logger-core` + `logger-runtime`. No daemon split. |
| Target platforms | **Linux, Windows, macOS** |
| MVP scope | **TUI parity + GUI-native wins** (e.g. panadapter bandmap, mouse on spots) |
| Framework | **iced** (Elm-style, wgpu-backed, cross-platform) |
| Bandmap visualization | **Frequency-axis panadapter style** as primary view |
| Visual design language | **Modern, clogger-native** (greenfield aesthetic, contest-workflow-respecting) |
| Window model | **Single OS window with floating, draggable, resizable internal panes** (MDI-style, à la N1MM) |
| Audio | **Defer to post-MVP** |

---

## What clogger already has going for a GUI port

The architecture is unusually GUI-friendly:

- **Pure reducer**: `logger_core::reduce(state, event) -> Vec<Effect>` at
  `logger-core/src/reducer.rs:102-112` is UI-agnostic. The two
  "UI-flavored" effects (`UiSetFocus { field_id }`, `UiClearEntry` —
  `logger-core/src/effects.rs:24,26`) translate cleanly to any framework.
- **Runtime is UI-free**: `logger-runtime` (LogAdapter, RigAdapter,
  KeyerTask, DxFeedAdapter, CondXAdapter) has zero `ratatui`/`crossterm`
  dependencies. All hardware I/O is behind tokio mpsc channels.
- **CLI proves portability**: `logger-cli` calls `reduce()` directly
  with fake adapters. A GUI follows the same pattern, swapping fakes for
  real adapter tasks.
- **Single bootstrap**: `logger_runtime::bootstrap(SessionConfig)` returns
  a `Session` with state, contest, macros, log adapter, call history,
  SCP, CTY. One call, ready to render.
- **Single-threaded reducer**: no `Send`/`Sync` traps in the core path.
  GUI event loop owns the reducer; effects fan out to adapter tasks.

The new `logger-gui` crate sits beside `logger-tui`, depends on the same
core + runtime, and re-implements only rendering and input.

---

## What the TUI implements that the GUI must replicate

From `logger-tui/src/ui/` and `event_loop.rs`. This is the parity surface:

### Panels (today's 3-column TUI layout, becoming floating MDI panes)
- **Score** (band × QSO/MULT) — `ui/score.rs`
- **Available** (unworked-by-band) — `ui/available.rs`
- **Rate** (10 min, 100 min, hour, time-since-last) — `ui/rate.rs`
- **SO2R status** (FOCUS/RX/TX routing) — optional, when SO2R present
- **CondX** (SFI/A/K/X-ray/band ratings) — optional
- **Error banner**
- **Log tail** (~10 most recent QSOs in table) — `ui/log_tail.rs`
- **Entry boxes** (R1/R2 for SO2R) with cursor, dupe badge, SCP `✓` —
  the heart of the contesting workflow
- **CW echo + speed line** (live char-by-char from
  `KeyerEvent::CharacterSent` broadcast, optional pre-population)
- **SCP suggestions** (footer when entry unfocused)
- **Bandmap** — one or two, classified per-radio (each bandmap colors
  spots against *its* radio's band+mode, not the focused radio)
- **Status bar** — callsign + contest, UTC clock, tri-state connection
  indicators (RIG1/RIG2/KEY/DXF/SO2R/SCRBD)
- **Export modal** wizard

### Behaviors
- Per-radio independent bandmap worked/mult classification (subtle, important)
- Live CW echo streaming (character broadcast → buffer per radio)
- Dupe badge in entry; SCP `✓` indicator; QRM badge in status bar
- Theme system: ~15 built-in themes + TOML — needs full GUI redesign
- Speed adjust PageUp/PageDown (±2 WPM); RX mode toggle on backtick;
  Insert toggles Run/S&P; Alt+Enter QuickLog; Ctrl+E export; F1-F12
  macros; Ctrl+Alt+F1-12 secondary macros; arrows for radio focus;
  Ctrl+Up/Down bandmap nav (R1), Ctrl+Alt for R2; bandmap nav skip-worked;
  bandmap spot blocking by station call

### TUI-local state the GUI will recreate
- `echo_per_radio: HashMap<RadioId, String>` — CW echo buffers
- `tx_radio` — which radio is keying (independent of focus)
- Connection mirrors (`rigs`, `keyer_connected`, …)
- `bandmap_cache` — render-time filtered spot lists per (band, mode),
  invalidated by `AppState.bandmap_version`
- Analytics snapshots (`worked_calls`, `mult_calls`, `avail`, `rate`,
  `score`)
- Modal state (`export_modal`)
- Theme

---

## Recommended approach

### New crate: `logger-gui`

```
clogger/
├── logger-core/       (unchanged)
├── logger-runtime/    (unchanged)
├── logger-cli/        (unchanged)
├── logger-tui/        (unchanged)
└── logger-gui/        (NEW)
    ├── Cargo.toml     (deps: logger-core, logger-runtime, iced, tokio,
    │                          wgpu indirectly via iced, anyhow, tracing)
    └── src/
        ├── main.rs            (bootstrap + iced::Application launch)
        ├── app.rs             (top-level iced Application)
        ├── runtime_bridge.rs  (translates iced Messages → AppEvents,
        │                       dispatches Effects to adapter tasks)
        ├── subscriptions.rs   (tokio mpsc → iced Subscription bridges)
        ├── mdi/               (floating-pane manager)
        │   ├── pane.rs        (Pane trait + drag/resize state)
        │   ├── workspace.rs   (z-order, focus, layout persistence)
        │   └── frame.rs       (chrome: title bar, drag handle, resize)
        ├── panes/             (one file per pane; each is a Pane impl)
        │   ├── entry.rs
        │   ├── log_tail.rs
        │   ├── bandmap.rs     (Canvas-based panadapter)
        │   ├── score.rs
        │   ├── available.rs
        │   ├── rate.rs
        │   ├── so2r.rs
        │   ├── condx.rs
        │   └── status.rs
        ├── modals/
        │   └── export.rs
        ├── theme.rs           (GUI theme: colors, typography, spacing)
        ├── input.rs           (keyboard map: iced KeyEvent → AppEvent / Key)
        └── state.rs           (GuiState: TUI-local-equivalent state)
```

### Event-loop architecture (iced ↔ clogger)

iced's `Application` model maps onto clogger as follows:

- **Model** — `app::App { core: Session, gui: GuiState, mdi: Workspace }`
- **Message** — `app::Message`, an enum that includes:
  - `KeyPressed(iced::keyboard::Event)` — translated to `AppEvent` in update
  - `Tick` (1 Hz, drives `AppEvent::TimerTick`)
  - `RenderTick` (~50ms cadence, no-op for now since iced is retained-mode)
  - `RigStatus`, `SpotReceived`, `SpotWithdrawn`, `KeyerEvent`,
    `CondXSnapshot`, `ScoreboardStatus` — produced by Subscriptions
    that wrap the runtime's mpsc channels
  - `MdiPaneDragStart`, `MdiPaneDragMove`, `MdiPaneResize`, `MdiPaneRaise`
    (workspace-internal)
  - `ExportModal(...)`, `OpenModal`, `CloseModal`
- **update(msg)** — for any clogger-domain Message, build an `AppEvent`,
  call `reduce(...)`, then `dispatch_effects(effects)`. Returns
  `iced::Command` chains for any effects that need async dispatch
  (CW send, log persist).
- **subscription()** — composes one Subscription per runtime channel:
  - `iced::time::every(1s)` → `Tick`
  - `Subscription::run` over each `mpsc::Receiver` (rig, keyer, dxfeed,
    condx, scoreboard, terminal-equivalent input is just iced events)
- **view()** — renders the MDI workspace (z-ordered floating panes) plus
  any modal overlay.

Reuse `logger-runtime::bootstrap()` exactly as `logger-cli` and `logger-tui`
do; no fork. The `RunArtifacts`/`Session`-equivalent gets handed to the
iced `Application::new` constructor.

### MDI workspace (the floating-panes piece)

Not built into iced. We build it on top of:

- **`iced::widget::Stack`** — overlapping children with explicit ordering.
- **`iced::widget::MouseArea`** — captures mouse-down on title bar, mouse
  motion for drag, mouse-down on resize handle.
- **Per-pane state**: `{ id, kind, position: (f32,f32), size: (f32,f32),
  z: u32, focused: bool }`.
- **Workspace** owns `Vec<PaneState>`, handles z-order, drag math, focus,
  serialization to disk (so layouts persist across runs).
- **Pane chrome**: thin title bar with a drag affordance, optional close /
  collapse buttons, an SE-corner resize grip. Clicking inside a pane
  routes input to that pane and brings it to front.

Each pane (entry, bandmap, log_tail, …) implements a small `Pane` trait:
```rust
pub trait Pane {
    fn title(&self) -> &str;
    fn view(&self, app: &App, theme: &GuiTheme) -> Element<'_, Message>;
    fn min_size(&self) -> (f32, f32);
}
```

### Bandmap as iced `Canvas`

The panadapter view is a custom `Canvas` widget. Inputs:

- Visible band span (e.g. 14.000 – 14.350 MHz; configurable / scrollable)
- Spots from `AppState.bandmap_spots` (filtered via
  `bandmap_cache` equivalent against this radio's band+mode)
- Per-spot classification: worked / mult-needed / new / blocked, drawn
  from the same `LogAdapter` traits the TUI uses (DupeChecker /
  MultChecker / CallHistoryLookup)
- Cursor freq (where the rig is parked)
- Mouse position → freq tooltip; click → emit `RigSet` for that radio

Future: VFO indicators, click-and-drag passband, IF center marker. Out of
scope for MVP but the Canvas approach makes them straightforward.

### Theme system

Rebuild from scratch as `logger-gui::theme::GuiTheme` with fields for
typography (font family, sizes, weights), color palette
(background/surface/border/text/accent/danger/success/dupe/mult/new),
and pane chrome styling. Translate the existing TOML theme schema or
ship a fresh GUI-native one — do not try to reuse ratatui's `Style`.

### Files to reuse as-is from `logger-runtime`

These need no changes; the GUI consumes them like the TUI does:

- `logger-runtime/src/bootstrap.rs:101-238` — `bootstrap(SessionConfig)`
- `logger-runtime/src/log_adapter.rs:35-54` — LogAdapter (DupeChecker,
  MultChecker, ContestHistoryLookup, persistence)
- `logger-runtime/src/rig_adapter.rs:64-80` — Rig task + RigCmd channel
- `logger-runtime/src/keyer_task.rs:73-80` — Keyer task + KeyerCmd /
  KeyerEvent channels
- `logger-runtime/src/dxfeed_adapter.rs:21-80` — DX cluster spots
- `logger-runtime/src/condx_adapter.rs:59-80` — Solar/CondX snapshots
- `logger-runtime/src/config.rs` — RigConfig, KeyerConfig, DxFeedConfig

### Files to reference but not reuse from `logger-tui`

These are the conceptual blueprint; the GUI re-implements them in iced:

- `logger-tui/src/event_loop.rs` — overall orchestration shape
- `logger-tui/src/main.rs:30-94` — `TuiState` shape (informs `GuiState`)
- `logger-tui/src/adapters/terminal.rs:50-240` — keyboard mapping
  (informs `input.rs`)
- `logger-tui/src/ui/*.rs` — per-pane render logic
- `logger-tui/src/perf.rs` — latency instrumentation pattern (worth
  carrying over to detect render-loop regressions)

---

## Risks / things to watch

- **iced multi-window not needed yet**, but if SO2R operators later push
  for detached bandmap windows, the MDI workspace abstraction keeps that
  door open without a rewrite.
- **Floating-pane drag/resize is fiddly** — accuracy of hit-testing,
  z-order edge cases, persisted-layout schema migrations. Budget a
  spike specifically for the MDI manager early.
- **iced custom Canvas perf** — bandmap could have hundreds of spots; profile
  re-render cost on a busy contest dataset.
- **Keyboard semantics mismatch** — iced's keyboard event model isn't
  identical to crossterm's. Modifiers, repeat behavior, IME interaction
  on Windows/macOS all need explicit testing.
- **Cross-platform packaging** — `cargo dist` or similar for releases
  (separate concern, not blocking architecture).
- **Theme schema divergence from TUI** — accept this; GUI themes are
  fundamentally different from terminal color codes.

---

## Verification plan

- **Build**: `cargo build -p logger-gui` on Linux first, then Windows
  and macOS in CI (set up new GitHub Actions matrix).
- **Reducer reuse**: existing `cargo test` should pass unchanged
  (the new crate adds no surface to the core's golden/snapshot tests).
- **Manual smoke**: spin up `logger-gui` against a real contest config
  (e.g. CQWW), enter a few QSOs, verify dupe/mult indicators, log
  persistence, CW send (with a real or fake keyer), bandmap spot click
  → rig QSY.
- **MDI scrub**: drag panes around, resize, close/reopen, restart app,
  verify layout persists.
- **Side-by-side**: run `logger-tui` and `logger-gui` against the *same*
  database (different sessions / processes) to confirm coexistence
  produces no surprises (read-only views from one while the other writes,
  reload semantics).
- **Snapshot tests for input mapping**: a new
  `logger-gui/tests/input_map.rs` confirming iced KeyEvents translate to
  the expected `AppEvent` / `Key` enums (parity with terminal mapping).

---

## Open items (defer past MVP, surfaced here so they're not lost)

- Audio cues (deferred this session)
- Detached / multi-OS-window mode (architecture leaves the door open)
- Pluggable panes / user-defined custom panes
- Themes import/export tooling
- High-DPI / font-scale settings UX
- Accessibility (screen readers, high-contrast mode)
- Settings UI (currently config is TOML-only)
