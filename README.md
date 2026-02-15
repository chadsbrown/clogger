# Contest Logger Headless Core

This workspace contains:
- `logger-core`: pure reducer/state machine and contest entry providers.
- `logger-cli`: headless script runner with fake rig/keyer and `qsolog` in-memory log backend.

## Input Rules

- `Space` always advances focus to the next entry field and wraps.
- `Enter` always triggers ESM.
- In default policy, `RUN` uses two-step ESM (`Enter` sends exchange, next `Enter` logs + TU).
- In default policy, `S&P` logs on first `Enter` and does not send TU unless policy enables it.

## Run Scripts

Use:

```bash
cargo run -p logger-cli -- scripts/cqww_run_two_step.json
```

To run all checks:

```bash
cargo test
cargo run -p logger-cli -- scripts/cqww_run_two_step.json
cargo run -p logger-cli -- scripts/cqww_run_invalid.json
cargo run -p logger-cli -- scripts/cqww_sp_one_step.json
cargo run -p logger-cli -- scripts/cqww_sp_send_tu.json
cargo run -p logger-cli -- scripts/sweeps_run_two_step.json
cargo run -p logger-cli -- scripts/sweeps_invalid_focus.json
```
