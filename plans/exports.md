# ADIF and Cabrillo Export Plan

## Context

clogger needs to produce two standard contest/log export formats:

- **ADIF** — universal ham radio QSO interchange format. Used by every logging tool. Stable, well-defined, generic per-QSO format with no contest knowledge required.
- **Cabrillo** — contest log submission format. Each contest has its own QSO line format and required header fields (contest name, category, club, claimed score, operators, etc.). Cabrillo cannot be written without knowing which contest is being logged.

Currently `qsolog` has neither — it's purely an in-memory store with SQLite journaling.

## Recommended Placement

### ADIF → `qsolog::export::adif`

Why it belongs in qsolog:
- qsolog already owns `QsoRecord` and `ExchangeBlob`
- ADIF is the natural "export to interchange format" feature for any QSO store
- Reusable by any project using qsolog (not just clogger)
- Self-contained — no upward dependencies needed
- ADIF doesn't need contest knowledge

### Cabrillo → split between `logger-core` and `logger-runtime`

Per-QSO formatting belongs with the contest definition. Header assembly and orchestration belong with the layer that already knows about both contests and the QSO log.

**1. `logger-core::contest::traits::ContestEntry`** — add optional methods:

```rust
fn cabrillo_contest_name(&self) -> Option<&str> { None }
fn cabrillo_qso_line(&self, qso: &CabrilloQso) -> Option<String> { None }
```

Each contest module (CQWW, CWT, MST, Sweeps) implements these because they own the knowledge of their exchange field order and Cabrillo formatting rules.

`logger-core` does not depend on `qsolog`, so a small `CabrilloQso` struct lives in core:

```rust
pub struct CabrilloQso {
    pub freq_khz: u32,        // Cabrillo uses kHz
    pub mode: String,          // "CW", "PH", "RY", "DG"
    pub date: String,          // YYYY-MM-DD
    pub time_utc: String,      // HHMM
    pub my_call: String,
    pub their_call: String,
    pub exchange_pairs: Vec<(String, String)>,
}
```

The runtime layer translates `QsoRecord` → `CabrilloQso` before calling the contest's formatter.

**2. `logger-runtime::cabrillo`** — orchestration:

- Takes `&LogAdapter`, `&dyn ContestEntry`, and a `CabrilloHeader` struct (station metadata)
- Iterates records, translates each to `CabrilloQso`, calls `contest.cabrillo_qso_line()`
- Assembles header + QSO lines + `END-OF-LOG:` footer
- Returns the full Cabrillo text (or writes to a path)

```rust
pub struct CabrilloHeader {
    pub callsign: String,
    pub category_operator: String,    // SINGLE-OP, MULTI-OP, etc.
    pub category_assisted: String,    // ASSISTED, NON-ASSISTED
    pub category_band: String,        // ALL, 20M, etc.
    pub category_power: String,       // HIGH, LOW, QRP
    pub category_mode: String,        // CW, SSB, MIXED
    pub claimed_score: i64,
    pub club: Option<String>,
    pub operators: Vec<String>,
    pub name: Option<String>,
    pub address: Vec<String>,
    pub email: Option<String>,
    pub soapbox: Vec<String>,
}
```

## Boundary Summary

| Concern | Lives in | Why |
|---------|----------|-----|
| ADIF tag/value formatting | `qsolog::export::adif` | Generic, no contest knowledge |
| Cabrillo per-QSO line format | `logger-core::contest::*` | Contest-specific exchange knowledge already lives here |
| Cabrillo header / orchestration | `logger-runtime::cabrillo` | Needs `LogAdapter` (qsolog records) + `ContestEntry` together |
| Cabrillo station metadata | TUI config (TOML) → flow into `CabrilloHeader` | User-supplied per session |

## Alternative (faster, less clean)

If touching qsolog isn't desired and the cross-crate split feels heavy, put **both** in `logger-runtime`:

- `logger-runtime::export::adif` — operates on `Vec<QsoRecord>` from `LogAdapter`
- `logger-runtime::export::cabrillo` — same, with `match contest_id { ... }` for per-contest dispatch

**Tradeoff**: Faster to ship, keeps qsolog untouched, but contest-specific Cabrillo formatting lives away from the contest definition. Easy for them to drift out of sync as new contests are added.

## Recommended Order of Implementation

1. **ADIF in qsolog** first — generic, simpler, no contest changes needed. Validates the export pattern with no upstream coupling.
2. **`CabrilloQso` struct + trait methods in `logger-core`** — add to `ContestEntry` with default `None` so existing contests compile unchanged.
3. **Implement `cabrillo_qso_line` for each contest** — CQWW, CWT, MST, Sweeps. Each is ~20 lines.
4. **`logger-runtime::cabrillo`** — header struct + orchestration function.
5. **TUI wiring** — config fields for header metadata, CLI command or keybind to trigger export.

## Files Touched (Recommended Path)

| Step | File | Change |
|------|------|--------|
| 1 | `qsolog/src/export/adif.rs` (new) | ADIF writer for `&[QsoRecord]` |
| 1 | `qsolog/src/lib.rs` | `pub mod export;` |
| 2 | `logger-core/src/contest/traits.rs` | `CabrilloQso` struct, trait methods with default `None` |
| 3 | `logger-core/src/contest/cqww.rs` | implement `cabrillo_qso_line` |
| 3 | `logger-core/src/contest/cwt.rs` | implement `cabrillo_qso_line` |
| 3 | `logger-core/src/contest/mst.rs` | implement `cabrillo_qso_line` |
| 3 | `logger-core/src/contest/sweeps.rs` | implement `cabrillo_qso_line` |
| 4 | `logger-runtime/src/cabrillo.rs` (new) | `CabrilloHeader`, `write_cabrillo()` |
| 4 | `logger-runtime/src/lib.rs` | `pub mod cabrillo;` |
| 5 | `logger-tui/src/config.rs` | optional `[cabrillo]` TOML section |
| 5 | `logger-tui/src/event_loop.rs` | export trigger (keybind or CLI flag) |

## Open Questions

- Where should the export be triggered from? Keybind in TUI, CLI subcommand on `logger-cli`, or both?
- Should ADIF export include voided QSOs (with appropriate flags) or skip them entirely?
- Should the Cabrillo header metadata be persisted between sessions, or always read fresh from config?
- Does the project need ADIF *import* as well, or only export?
