# Contest Logger

This workspace contains:
- `logger-core`: pure reducer/state machine and contest entry providers.
- `logger-cli`: headless script runner with fake rig/keyer and `qsolog` in-memory log backend.
- `logger-tui`: live terminal UI for real contest operation with rig control, CW keying, DX cluster, and SQLite persistence.

## logger-tui

### Quick start

```bash
cp logger-tui.example.toml logger-tui.toml
# Edit logger-tui.toml with your call, zone, and hardware settings
cargo run -p logger-tui -- -c logger-tui.toml
```

### Configuration

See `logger-tui.example.toml` for all options. Required fields:

```toml
my_call = "N9UNX"
my_zone = 4
contest = "cqww"    # or "sweeps"
```

Optional sections: `db_path` for SQLite persistence, `[rig]` for rig control, `[keyer]` for WinKeyer, `[[dxfeed.sources]]` for DX cluster spots. Each hardware section can be omitted — the TUI runs without it and logs a warning if a connection fails.

### Keybindings

| Key | Action |
|-----|--------|
| F1 | Send CQ |
| F2 | Send exchange |
| F3 | Send TU |
| Space | Advance to next field |
| Tab | Advance to next field |
| Enter | ESM (send exchange or log QSO) |
| Backspace | Delete last character |
| Esc | Clear focused field |
| Ctrl-C | Quit |

### ESM (Enter Sends Message)

- **RUN mode**: first Enter sends exchange, second Enter logs QSO + sends TU.
- **S&P mode**: Enter logs immediately on valid entry.

## Input Rules

- `Space` always advances focus to the next entry field and wraps.
- `Enter` always triggers ESM.
- In default policy, `RUN` uses two-step ESM (`Enter` sends exchange, next `Enter` logs + TU).
- In default policy, `S&P` logs on first `Enter` and does not send TU unless policy enables it.

## logger-cli

### Run Scripts

```bash
cargo run -p logger-cli -- scripts/cqww_run_two_step.json
```

To run all checks:

```bash
cargo test
```

All golden scripts also run under `cargo test` via `logger-cli` test harness.

Effect-trace snapshots are checked in tests. To intentionally refresh them:

```bash
UPDATE_SNAPSHOTS=1 cargo test -p logger-cli
```
