# Cabrillo Export

Add Cabrillo log file export to clogger, triggered from the existing
Ctrl+E export modal (currently shows `[C] Cabrillo` greyed out).

## What's Already in Place

- `[category]` config with all Cabrillo class enums (power, assisted, transmitter, operator, bands, mode, overlay)
- `[cabrillo]` config with header metadata (club, operators, name, address, email, soapbox)
- `cabrillo_id()` on `ContestEntry` trait, implemented for all four contests
- `CategoryConfig` types with `as_str()` methods that produce spec-compliant values
- Ctrl+E export modal with format selection UI (Cabrillo option present but greyed out)
- `ScoreBreakdown` / `score_summary()` for claimed score

## Implementation Steps

### 1. `CabrilloQso` struct + trait method in `logger-core`

Add to `logger-core/src/contest/traits.rs`:

```rust
pub struct CabrilloQso {
    pub freq_khz: u32,
    pub mode: String,         // "CW", "PH", "RY", "DG"
    pub date: String,         // YYYY-MM-DD
    pub time_utc: String,     // HHMM
    pub my_call: String,
    pub their_call: String,
    pub exchange_pairs: Vec<(String, String)>,
}
```

Add to `ContestEntry` trait:

```rust
fn cabrillo_qso_line(&self, qso: &CabrilloQso) -> Option<String> { None }
```

### 2. Implement `cabrillo_qso_line` for each contest

Each contest formats its own QSO line per the Cabrillo spec. Examples:

- **CQWW**: `QSO: 14000 CW 2024-01-15 1430 N9UNX         599 04     K1ABC         599 05`
- **CWT**: `QSO: 14000 CW 2024-01-15 1430 N9UNX         CHAD 2187   K1ABC         BOB 1234`
- **MST**: `QSO: 14000 CW 2024-01-15 1430 N9UNX         CHAD 001    K1ABC         BOB 042`
- **Sweeps**: `QSO: 14000 CW 2024-01-15 1430 N9UNX         001 A 73 STX K1ABC         042 B 58 CT`

Each implementation is ~20 lines of field-width formatting.

### 3. `logger-runtime::cabrillo` orchestration

New module `logger-runtime/src/cabrillo.rs`:

```rust
pub fn export_cabrillo(
    records: &[QsoRecord],
    contest: &dyn ContestEntry,
    category: &CategoryConfig,
    cabrillo_cfg: &CabrilloConfig,
    my_call: &str,
    my_zone: u8,
    rst_sent: &str,
    claimed_score: i64,
    path: &Path,
) -> Result<usize>
```

Assembles:
- `START-OF-LOG: 3.0`
- Header tags: CONTEST, CALLSIGN, CATEGORY-*, CLAIMED-SCORE, CLUB, OPERATORS, NAME, ADDRESS, EMAIL, SOAPBOX
- QSO lines (skip voided, translate `QsoRecord` → `CabrilloQso` → `contest.cabrillo_qso_line()`)
- `END-OF-LOG:`

### 4. Wire into Ctrl+E modal

- Enable the `[C] Cabrillo` option in `export_modal.rs`
- Require `[category]` config when Cabrillo is selected (show error if missing)
- Default file path: `{call}-{contest}.log`
- Call `logger_runtime::cabrillo::export_cabrillo()` with config data from TuiState/main

## Files Touched

| Step | File | Change |
|------|------|--------|
| 1 | `logger-core/src/contest/traits.rs` | `CabrilloQso` struct, `cabrillo_qso_line` trait method |
| 1 | `logger-core/src/lib.rs` | Export `CabrilloQso` |
| 2 | `logger-core/src/contest/cqww.rs` | Implement `cabrillo_qso_line` |
| 2 | `logger-core/src/contest/cwt.rs` | Implement `cabrillo_qso_line` |
| 2 | `logger-core/src/contest/mst.rs` | Implement `cabrillo_qso_line` |
| 2 | `logger-core/src/contest/sweeps.rs` | Implement `cabrillo_qso_line` |
| 3 | `logger-runtime/src/cabrillo.rs` (new) | Header + QSO orchestration |
| 3 | `logger-runtime/src/lib.rs` | `pub mod cabrillo;` |
| 4 | `logger-tui/src/ui/export_modal.rs` | Enable Cabrillo option, add path input flow |
| 4 | `logger-tui/src/event_loop.rs` | Pass category/cabrillo config to modal handler |
