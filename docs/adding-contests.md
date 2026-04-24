# Adding a New Contest to clogger

clogger supports two paths for adding a new contest. Most contests use the
**spec-driven path**, which requires no new Rust code in clogger. Contests
with custom validation logic that the spec schema can't express use the
**hand-coded path**.

## Architecture Overview

Contest knowledge is split across three layers:

| Layer | Owns | Examples |
|-------|------|----------|
| **contest-engine** | Scoring rules, exchange definitions, multiplier logic | `specs/cqww.json`, `specs/cwt.json` |
| **logger-core** | UI metadata (field widths, macros, history mapping), form/validation/draft logic | `contest/registry.rs`, `contest/spec_driven.rs` |
| **logger-runtime** | Scoring integration, export | `scoring/spec_scorer.rs` |

Each piece of information lives in exactly one place.

## Path A: Spec-Driven (common case)

Used for: CQWW, CWT, NAQP, ARRL DX, and most standard contests.

### Step 1: Write the contest-engine spec

Create a JSON spec file in the `contest-engine` repository under `specs/`:

```
contest-engine/specs/your_contest.json
```

The spec defines:
- `id` — unique string identifier (e.g. `"cqww"`, `"naqp"`)
- `name` — human-readable display name
- `cabrillo_contest` — Cabrillo contest ID string
- `exchange.received_variants` — what fields the other station sends you
- `exchange.sent_variants` — what you send them
- `multipliers` — multiplier rules for scoring
- `points` — QSO point rules
- `config_fields` — required configuration (my_zone, my_name, etc.)

See existing specs (`cqww.json`, `cwt.json`, `naqp.json`, `arrl_dx.json`)
for working examples.

### Step 2: Register the spec in contest-engine

Add the new spec to the embedded module in
`contest-engine/src/spec.rs`:

1. Add a `const` for the included JSON:
   ```rust
   const YOUR_CONTEST_JSON: &str = include_str!("../specs/your_contest.json");
   ```

2. Add the ID to `SPEC_IDS`:
   ```rust
   pub const SPEC_IDS: &[&str] = &["cqww", "cqww_cw", "cwt", "arrl_dx", "naqp", "your_contest"];
   ```

3. Add a match arm in `spec_by_id()`:
   ```rust
   "your_contest" => YOUR_CONTEST_JSON,
   ```

Commit and push contest-engine, then run `cargo update -p contest-engine`
in clogger.

### Step 3: Add a registry entry in clogger

Add one entry to the `SPEC_CONTESTS` table in
`logger-core/src/contest/registry.rs`:

```rust
SpecContestMeta {
    contest_id: "your_contest",      // must match the spec's id field
    contest_instance_id: 7,          // unique u64, never reuse
    field_widths: &[                 // (field_id, display_width)
        (1, 12),                     // CALL is always field_id 1
        (2, 8),                      // first exchange field
        (3, 5),                      // second exchange field
    ],
    default_macros_fn: your_macros,  // factory function (see below)
    history_mapping: &[              // .ch column name → field_id
        ("Name", 2),
    ],
    uses_serial: false,              // true if contest sends serial numbers
    exchange_schema_id: 7,           // unique u16 for QsoDraft
    auto_toggle_mode: false,         // true for NS-Sprint-style Run↔S&P flip on log
},
```

**Cabrillo IDs come from the contest-engine spec** (the `cabrillo_id` or
`cabrillo_ids` field in the JSON), not from the registry. No function
pointer is wired up here.

And the macro factory function:

```rust
fn your_macros() -> Macros {
    Macros {
        f1: "CQ TEST {MYCALL}".to_string(),
        f2: "{RST_SENT} {MYZONE}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}
```

### Step 4: Add a scorer entry (if using contest-engine scoring)

If the contest has an embedded spec, scoring is automatic — the existing
`SpecScorer` in `logger-runtime/src/scoring/spec_scorer.rs` handles it via
the `scorer_for_contest()` function in `logger-runtime/src/scoring/mod.rs`.
No changes needed.

### That's it

Both front-ends (GUI and TUI) pick up the new contest automatically when
the user sets `contest = "your_contest"` in their TOML config. Form fields,
validation, scoring, dupe checking, mult indicators, and ADIF export all
work.

### Field ID assignment

- Field ID `1` is always the CALL field
- Received exchange fields from the spec get sequential IDs starting at `2`
- The order follows `exchange.received_variants[0].fields` in the spec JSON

### Field width guidelines

Widths in the registry control TUI column widths (the GUI uses them as
hints but lays out with proportional fonts). Guidelines:
- CALL: 12 (handles most callsigns)
- RST: 3
- Zone/serial: width of the max value (e.g., zones 1-40 → 2)
- Name: 10
- State/section: 4-6
- Free-form exchange: 6-8

If a field_id is not in the registry's `field_widths`, a default width is
derived from the spec's `field_type`:
- `Rst` → 3
- `Int` with range → digits in max value
- Everything else → 8

### Exchange field validation

The generic `SpecDrivenContest` validates fields based on their spec type:

| field_type | Validation |
|------------|------------|
| `Rst` | 2-3 ASCII digits |
| `Int` | Numeric, within `Range { min, max }` if specified |
| `String` | Non-empty if required |
| `Enum` | Non-empty if required |
| Other | Always valid |

## Path B: Hand-Coded (escape hatch)

Used for contests that need validation logic the spec schema can't express
(e.g., Sweepstakes with its precedence code enum and check digit format).

### Step 1: Create the contest module

Create `logger-core/src/contest/your_contest.rs` with a struct that
implements the `ContestEntry` trait:

```rust
pub struct YourContest;

impl ContestEntry for YourContest {
    fn contest_id(&self) -> &str { "your_contest" }
    fn contest_name(&self) -> &str { "Your Contest Name" }
    fn contest_instance_id(&self) -> u64 { 7 }
    fn default_macros(&self) -> Macros { /* ... */ }
    fn form_spec(&self) -> EntryFormSpec { /* ... */ }
    fn validate_entry(&self, input: &EntryState, ctx: &EntryContext)
        -> EntryValidation { /* ... */ }
    fn build_qso_draft(&self, input: &EntryState, ctx: &EntryContext)
        -> Result<QsoDraft, EntryError> { /* ... */ }
    // ... optional: history_field_mapping, uses_serial, cabrillo_id
}
```

See `mst.rs` and `sweeps.rs` for working examples.

### Step 2: Register in the dispatcher

Add a match arm in `logger-core/src/contest/mod.rs`:

```rust
"your_contest" => Some(Box::new(your_contest::YourContest)),
```

And register the module:
```rust
pub mod your_contest;
```

### Step 3: Add a scorer (if not spec-based)

If the contest doesn't have a contest-engine spec, add a scorer
implementation in `logger-runtime/src/scoring/`. See `mst.rs` or
`unique_call.rs` for the pattern. Register it in `scorer_for_contest()`
in `scoring/mod.rs`.

If the contest does have a spec (for scoring only, with hand-coded UI),
the `SpecScorer` handles it automatically.

## Currently Supported Contests

Major contests (spec-driven unless noted):

| Contest | contest_id | Spec |
|---------|------------|------|
| CQ World-Wide DX | `cqww` | `cqww.json` |
| CWops CWT | `cwt` | `cwt.json` |
| NAQP | `naqp` | `naqp.json` |
| ARRL DX | `arrl_dx` | `arrl_dx.json` |
| ICWC Medium Speed Test | `mst` | `mst.json` |
| NCCC NS Sprint | `ns_sprint` | `ns_sprint.json` |
| ARRL Sweepstakes (hand-coded) | `ss` (alias `sweeps`) | none |

State / Province QSO parties — thirteen wired up (`flqp`, `gaqp`,
`inqp`, `miqp`, `moqp`, `ndqp`, `nhqp`, `nmqp`, `neqp`, `neqsop`,
`onqp`, `qcqp`, `deqp`). All spec-driven, all share the same
CALL+RST+LOC shape, configured per-contest in `contest.toml`'s
`[station]` section. See `docs/contests.md` for the per-contest
config fields.

The authoritative list is `SPEC_CONTESTS` in
`logger-core/src/contest/registry.rs` — consult it if a new contest
has landed since this doc was last refreshed.

## Key Files Reference

| File | Purpose |
|------|---------|
| `contest-engine/specs/*.json` | Contest rules (scoring, exchange, multipliers) |
| `logger-core/src/contest/registry.rs` | Clogger metadata table (widths, macros, history) |
| `logger-core/src/contest/spec_driven.rs` | Generic ContestEntry impl for spec-based contests |
| `logger-core/src/contest/mod.rs` | `contest_from_id()` dispatcher |
| `logger-core/src/contest/traits.rs` | `ContestEntry` trait definition |
| `logger-runtime/src/scoring/mod.rs` | `scorer_for_contest()` — scorer selection |
| `logger-runtime/src/scoring/spec_scorer.rs` | Generic scorer for spec-based contests |
