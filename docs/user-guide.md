# clogger User Guide

clogger is a terminal contest logger built for one operator (N9UNX) and
shared without support guarantees. This guide is for that operator, and
for anyone curious enough to read the code.

## Related documents

- **[Configuration](configuration.md)** — every TOML field in `config.toml` and `contest.toml`, what it controls, and the defaults.
- **[Operating](operating.md)** — keybindings, ESM flow, CW macros, SO2R, and the TUI panels.
- **[Contests](contests.md)** — per-contest notes (exchange shape, mult types, quirks).
- **[DX feed tuning](dxfeed-tuning.md)** — dxfeed skimmer quality engine + filter pipeline.
- **[Adding contests](adding-contests.md)** — how spec-driven vs. code-driven contests are wired.

## What clogger does

- Logs contest QSOs to SQLite, with undo/redo.
- Drives one or two rigs (CAT) and an SO2R switch (OTRSP).
- Sends CW via WinKeyer.
- Ingests DX cluster / RBN spots and paints a bandmap that knows your
  dupe and multiplier state.
- Scores and exports Cabrillo / ADIF.
- Posts live score to Contest Online Scoreboard endpoints.
- Runs as a TUI (terminal UI) — no GUI, no mouse. Keyboard only.

## First-run checklist

1. **Build.** `cargo build --release` in the workspace root. The
   binaries land in `target/release/logger-tui` and `logger-cli`.

2. **Create `config.toml`.** Copy `config.example.toml` to a stable
   location (e.g. `~/clogger/config.toml`) and fill in:
   - `my_call` (required), `my_zone`, `rst_sent`.
   - `scp_file` — path to `master.scp` from
     [SCP collections](https://www.supercheckpartial.com/).
   - `cty_file` — path to `cty.dat` (Big CTY flavor) from
     [country-files.com](https://www.country-files.com/). Required
     if your dxfeed filter uses any `geo.*` rules.
   - One or more `[[rig]]` blocks for CAT control.
   - Optional `[keyer]`, `[so2r]`, `[dxfeed]`, `[condx]`,
     `[scoreboard]`, `[cabrillo]` sections.

   See [Configuration](configuration.md) for the full reference.

3. **Create a per-contest `contest.toml`.** Copy
   `contest.example.toml` for each contest you run. Set:
   - `contest` — the contest id (e.g. `cqww`, `miqp`).
   - `my_xchg` — your sent exchange (contest-specific; see
     [Contests](contests.md)).
   - `db_path` — SQLite log file (distinct per contest and per
     operating weekend).
   - `[station]` — typed config for state QPs and other contests that
     need it (e.g. `my_is_mi`, `my_county`, `my_power_class`).
   - Optional `[macros]`, `[category]`.

4. **Launch.**
   ```
   logger-tui --config ~/clogger/config.toml --contest ./miqp-2026.toml
   ```

   CLI flags override TOML paths — useful for one-off swaps. See
   [Operating — CLI flags](operating.md#command-line-flags).

5. **Log a QSO.** With a rig connected, tune to a station, type their
   call + exchange, hit Enter. See
   [Operating — ESM flow](operating.md#esm-enter-sends-message).

## Mental model

clogger is a strict **reducer + effects** design:

- **`logger-core`** is a pure state machine. It takes the current
  `AppState` and an `AppEvent` (keypress, rig status, spot received,
  timer tick) and produces a new state + a list of `Effect`s
  (`CwSend`, `LogInsert`, `RigSet`, etc.). It has no I/O, no disk, no
  threads.

- **`logger-runtime`** implements the hardware adapters and
  persistence (SQLite, rig serial, WinKeyer, OTRSP, HTTP to DX cluster
  and scoreboard).

- **`logger-tui`** is the ratatui front-end. Reads keyboard, drives
  the event loop, renders panels.

- **`logger-cli`** is a headless script runner for golden tests.

This layering matters for debugging: if a QSO logs wrong, you can
usually reproduce it by feeding the same sequence of events into the
reducer directly — no rig, no terminal needed.

## The log and its persistence

- QSOs are written to the SQLite file named by `db_path` (or `--db`).
- Undo/redo survives restart.
- Dupe and multiplier classification also survive restart: clogger
  rebuilds the scorer from the log on open.
- If you restart **without** a `db_path`, the log is in-memory only.
  Every launch is a fresh slate — the bandmap will paint every call
  as un-worked.
- `scp_file` and `cty_file` are read-only lookups. Point them at a
  single canonical copy and forget.

## Getting help

There is no support. Bugs should be reproducible from code inspection
or a golden-script replay. The `clogger.log` file (written in the
current working directory at startup) captures warn-level events by
default and debug when launched with `--debug`.

If something goes wrong mid-contest: your log is on disk. Close
clogger and reopen — scorer state rebuilds from the persisted QSOs.
