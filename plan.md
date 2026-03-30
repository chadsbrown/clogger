Here are two Markdown documents you can drop directly into your repo (e.g., `PLAN.md` and `STATUS.md`).

---

# Contest Logger – Architectural Plan

## Vision

Build a modern, high-performance, cross-platform amateur radio contest logger in Rust with:

* Deterministic, testable core logic
* Replaceable UI layer (GUI and/or TUI)
* Strong SO2R foundation
* Spec-driven contest behavior (via `contest-engine`)
* Durable, crash-tolerant logbook (`qsolog`)
* Headless simulation capability for regression testing

The system is intentionally layered so that **the UI is mostly wiring**.

---

## Architectural Overview

### 1. Core Design Principles

1. **Pure App Core**

   * Deterministic reducer (`reduce(state, event) -> effects`)
   * No GUI dependencies
   * No direct IO
   * Fully scriptable headless mode

2. **Spec-Driven Entry**

   * Entry fields defined by `contest-engine`
   * No hard-coded CALL/RST/etc assumptions
   * Validation comes from contest spec
   * Exchange payload encoded structurally (FieldId → Value)

3. **ESM (Enter Sends Message)**

   * Enter triggers state machine logic
   * Space always advances focus (no heuristics)
   * RUN mode: two-step (EXCH → TU+LOG)
   * S&P mode: configurable policy

4. **Logbook as Infrastructure**

   * `qsolog` is authoritative
   * In-memory primary, SQLite-backed durability
   * Undo/redo via compensating operations
   * Insertion order canonical

5. **Contest Consequences**

   * Dupes computed against log
   * New multiplier feedback via contest-engine
   * SO2R-aware (band/mode context per radio)

6. **Regression Armor**

   * Golden headless scripts
   * Effect-trace snapshot tests
   * Deterministic behavior required

---

## Major Components

### logger-core (Pure State Machine)

* `AppState`
* `EntryState` (spec-driven fields)
* ESM state machine
* Dupe + mult indicators
* Reducer
* ContestEntry adapter interface

### logger-cli (Headless Runner)

* Script runner (JSON-based)
* Fake adapters (keyer, rig, log)
* Real adapters (qsolog, contest-engine)
* Effect trace snapshot generation
* Optional live headless mode

### contest-engine (External Crate)

* Contest spec
* Entry validation
* Mult computation
* Projection/state

### qsolog (External Crate)

* Authoritative QSO storage
* Undo/redo
* Event journal
* Insertion order canonical
* SQLite persistence (optional in headless mode)

---

## SO2R Model

* Multiple radios tracked independently
* Focused radio determines:

  * band/mode context
  * dupe domain
  * mult domain
* Entry state tied to focused radio
* No cross-radio leakage of band logic

---

## Safety & Invariants

* Space always advances focus.
* Enter is the only ESM trigger.
* Editing any entry field resets `esm_step` to Idle.
* No callsign heuristics for navigation.
* Core must remain deterministic.
* All side effects emitted explicitly as `Effect`.

---

## Long-Term Direction

After headless core maturity:

1. Full contest-engine projection integration
2. SO2R hardware routing integration (OTRSP, riglib)
3. Real-time bandmap and rate calculations
4. GUI layer (GPUI or other)
5. Networked multi-op support
6. Snapshot persistence & crash-recovery tests

The design goal is that GUI work does not require architectural changes to core logic.

---

## Guiding Philosophy

The logger is treated as:

> A deterministic state machine with pluggable adapters.

Everything else — GUI, hardware, persistence — sits around it.

This allows:

* Safe refactoring
* Deep regression coverage
* Confident expansion to complex contests
* High-performance operation under contest pressure

---

# Contest Logger – Current Status & Next Phases

## Current Status (As Of Now)

The following are complete and working:

### Core

* Headless `logger-core` implemented
* Spec-driven entry fields (via real `contest-engine`)
* CQWW contest integrated using real spec
* ARRL Sweepstakes added to validate multi-field entry
* Enter = ESM
* Space = always advance field
* RUN two-step ESM implemented
* S&P configurable ESM policy

### Safety & UX Guarantees

* Editing any field resets `esm_step`
* No callsign heuristics for navigation
* Space never inserts literal spaces
* Deterministic reducer behavior

### Logbook

* Integrated with real `qsolog` (in-memory)
* Insertion order preserved
* Undo/redo supported
* Exchange payload round-trips correctly

### Feedback Loops

* Dupe indicator implemented
* New multiplier indicator implemented
* Both recompute on:

  * CALL edits
  * band/mode changes
  * radio focus changes

### SO2R Readiness

* Multiple radios tracked independently
* FocusRadio event implemented
* Dupe + mult computed per-band/per-radio
* Scripts verify band separation

### Regression Protection

* Headless scripts implemented
* Effect-trace snapshot tests
* Golden snapshot comparison under `cargo test`

This is now a real contest logger core without a GUI.

---

## What This Means Architecturally

We now have:

* Deterministic contest logic
* Real contest spec integration
* Real logbook integration
* SO2R-aware context separation
* Regression armor

The UI layer can now be built on top without destabilizing the core.

---

# Next Major Phases

## Phase 1 – Deeper Contest Consequences

* Surface multiplier type detail (not just boolean)
* Integrate full contest-engine projection state
* Add rate calculations (last 10/60 minutes)
* Add running/S&P state hints

## Phase 2 – Persistence & Crash Safety

* Use SQLite-backed qsolog in headless integration tests
* Add restart + replay test
* Snapshot/restore validation

## Phase 3 – Live Headless Mode

* Wire riglib → AppEvent::RigStatus
* Wire dxfeed → SpotReceived
* Terminal keystroke input
* Minimal TUI-style text display

Goal: Operate a contest from terminal before GUI.

## Phase 4 – Hardware Integration

* OTRSP routing
* Winkey integration
* Radio sync (VFO A/B tracking)
* SO2R switching policies

## Phase 5 – GUI Layer

* Dynamic field rendering from EntryFormSpec
* Bind Space/Enter correctly
* Visual dupe/mult indication
* Bandmap panel
* Rate window
* SO2R focus visualization

Because core is stable, GUI should be wiring + presentation only.

## Phase 6 – Networked Multi-Op

* Log replication model
* Event stream synchronization
* Conflict handling
* Operator identity tracking

---

# Immediate Next Step (When You Resume)

Pick one:

**A) Deepen contest projection**

* Expose richer multiplier metadata
* Add rate computation

**B) Live headless mode**

* Operate with real rig + dxfeed

**C) Begin GUI shell**

* Minimal window that renders entry fields dynamically

Recommended: Live headless mode first, then GUI.

---

## Key Reminder

The architecture is now strong.

Do not compromise:

* Determinism
* Pure reducer model
* Clear separation between core and adapters
* Snapshot regression coverage

Those are the foundations that make everything else safe.

When starting a new session, point the assistant to this file and `PLAN.md` for instant context.

---

If you'd like, I can also produce a very short `ARCHITECTURE.md` diagram-style summary (1 page) suitable for onboarding collaborators.

