# Contest Definition Architecture

## Context

clogger currently has duplicate contest definitions: each spec-based contest
(`CqwwContest`, `CwtContest`) keeps a local copy of `cqww.json` / `cwt.json`
under `logger-core/specs/` that is byte-identical to the canonical copy in
`contest-engine/specs/`. Each is parsed separately by clogger and
contest-engine. This is a sync hazard — when contest-engine's spec changes,
clogger's copy silently drifts until someone notices.

More broadly, the boundary between clogger-owned concerns (UI, messaging,
integration) and contest-engine-owned concerns (scoring rules) isn't
enforced. Some things live in both places unnecessarily; some are ambiguous.

This plan establishes a long-term architecture where each piece of
contest-related information lives in exactly one place, adding a new
contest is a single-step operation, and the boundaries between layers are
clear and enforced.

## Principle: each concern has exactly one source of truth

| Concern | Source of truth | Why |
|---|---|---|
| Received exchange field definitions | contest-engine spec (JSON) | Contest rules |
| Sent exchange field definitions | contest-engine spec (JSON) | Contest rules |
| Multiplier rules | contest-engine spec (JSON) | Scoring engine |
| Scoring formula | contest-engine spec (JSON) | Scoring engine |
| Points per QSO | contest-engine spec (JSON) | Scoring engine |
| Dupe dimension | contest-engine spec (JSON) | Contest rules |
| Cabrillo contest string | contest-engine spec (JSON) | Export format |
| Display name ("CWops CWT") | contest-engine spec (`name`) | Contest metadata |
| Bands and modes allowed | contest-engine spec (JSON) | Contest rules |
| Config field declarations (my_zone, my_name) | contest-engine spec (JSON) | Scoring engine inputs |
| Form field widths | clogger registry | TUI display concern |
| Default CW macros (F1-F9) | clogger registry | UI messaging |
| Call history column → field mapping | clogger registry | N1MM `.ch` integration |
| `uses_serial` policy | clogger registry | UI / serial counter |
| `contest_instance_id` (u64) | clogger registry | qsolog DB key |
| Custom validators (Sweeps precedence) | clogger hand-coded impl | Escape hatch |
| Scorer implementation | logger-runtime | Consumer of spec |

**Nothing appears in two places.** Display name, for example, lives in the
contest-engine spec's `name` field and is read by the TUI — but the TUI
doesn't store its own copy.

## Architecture

Three layers:

### contest-engine (catalog of contest rules)

- One JSON spec file per contest under `specs/`
- `embedded::spec_by_id(id)` is the single lookup API (already implemented)
- Every contest clogger supports has a spec here — including ones currently
  hand-coded in clogger, if feasible
- Adding a new contest starts with a new JSON file here

### logger-core (trait + dispatcher + clogger metadata)

New layout:

```
logger-core/src/contest/
├── traits.rs       — ContestEntry trait (unchanged, plus new contest_name())
├── mod.rs          — contest_from_id dispatcher
├── spec_driven.rs  — NEW: generic SpecDrivenContest implementing ContestEntry
├── registry.rs     — NEW: SpecContestMeta + static SPEC_CONTESTS table
├── sweeps.rs       — hand-coded Sweepstakes (unchanged)
└── mst.rs          — hand-coded MST (unchanged, for now)
```

Deleted:
- `logger-core/specs/cqww.json` (was a duplicate of contest-engine's copy)
- `logger-core/specs/cwt.json` (same)
- `logger-core/src/contest/cqww.rs` (replaced by registry entry)
- `logger-core/src/contest/cwt.rs` (replaced by registry entry)

`logger-core` gains a dependency on `contest-engine`. See
"Architectural decision" below for the rationale.

### logger-runtime (scorer, unchanged)

Already uses `contest_engine::spec::embedded::spec_by_id` after the previous
refactor. No changes.

## The ContestEntry trait

```rust
pub trait ContestEntry {
    fn contest_id(&self) -> &str;
    fn contest_name(&self) -> &str;      // NEW — human-readable name
    fn contest_instance_id(&self) -> u64;
    fn default_macros(&self) -> Macros;
    fn form_spec(&self) -> EntryFormSpec;
    fn validate_entry(&self, input: &EntryState, ctx: &EntryContext)
        -> EntryValidation;
    fn build_qso_draft(&self, input: &EntryState, ctx: &EntryContext)
        -> Result<QsoDraft, EntryError>;
    fn history_field_mapping(&self) -> Vec<(&str, u16)>;
    fn uses_serial(&self) -> bool;
}
```

Only change from today: the new `contest_name()` method.

## Two implementation strategies

### A. Spec-driven (the common case — CQWW, CWT, NAQP, ARRL DX, ...)

A single generic `SpecDrivenContest` type combines clogger metadata with a
contest-engine spec:

```rust
pub struct SpecContestMeta {
    pub contest_id: &'static str,              // matches contest-engine spec ID
    pub contest_instance_id: u64,               // clogger-assigned
    pub field_widths: &'static [(u16, u16)],    // (field_id, width)
    pub default_macros_fn: fn() -> Macros,
    pub history_mapping: &'static [(&'static str, u16)],
    pub uses_serial: bool,
}

pub struct SpecDrivenContest {
    meta: &'static SpecContestMeta,
    spec: ContestSpec,  // loaded once from contest_engine::embedded
}

impl ContestEntry for SpecDrivenContest {
    fn contest_id(&self) -> &str { self.meta.contest_id }
    fn contest_name(&self) -> &str { &self.spec.name }   // from contest-engine
    fn contest_instance_id(&self) -> u64 { self.meta.contest_instance_id }
    fn default_macros(&self) -> Macros { (self.meta.default_macros_fn)() }
    fn form_spec(&self) -> EntryFormSpec {
        derive_form_spec(&self.spec, self.meta.field_widths)
    }
    fn validate_entry(&self, input: &EntryState, ctx: &EntryContext)
        -> EntryValidation
    {
        validate_against_spec(input, &self.spec, ctx)
    }
    fn build_qso_draft(&self, input: &EntryState, ctx: &EntryContext)
        -> Result<QsoDraft, EntryError>
    {
        build_draft_from_spec(input, ctx, &self.spec, self.meta.contest_instance_id)
    }
    fn history_field_mapping(&self) -> Vec<(&str, u16)> {
        self.meta.history_mapping.iter().copied().collect()
    }
    fn uses_serial(&self) -> bool { self.meta.uses_serial }
}
```

The registry is a static table:

```rust
pub const SPEC_CONTESTS: &[SpecContestMeta] = &[
    SpecContestMeta {
        contest_id: "cqww",
        contest_instance_id: 1,
        field_widths: &[(1, 12), (2, 3), (3, 3)],
        default_macros_fn: cqww_macros,
        history_mapping: &[("CqZone", 3)],
        uses_serial: false,
    },
    SpecContestMeta {
        contest_id: "cwt",
        contest_instance_id: 3,
        field_widths: &[(1, 12), (2, 10), (3, 6)],
        default_macros_fn: cwt_macros,
        history_mapping: &[("Name", 2), ("Exch1", 3)],
        uses_serial: false,
    },
    // Add new spec-based contests here.
];

fn cqww_macros() -> Macros { Macros::default() }

fn cwt_macros() -> Macros {
    Macros {
        f1: "CQ CWT {MYCALL}".to_string(),
        f2: "{MYNAME} {MYXCHG}".to_string(),
        f3: "TU {MYCALL}".to_string(),
        ..Macros::default()
    }
}
```

### B. Hand-coded (Sweeps, MST — escape hatch)

Stays as today: individual `.rs` files with a struct implementing
`ContestEntry` directly. Used when the contest needs logic that the
contest-engine JSON schema can't express (Sweepstakes check digit format,
precedence code enum, custom section validation).

These files look exactly like they do today. The trait gains one method
(`contest_name`), so those impls add one line.

## Dispatcher

```rust
// logger-core/src/contest/mod.rs
pub fn contest_from_id(id: &str) -> Option<Box<dyn ContestEntry>> {
    // Spec-driven contests first
    if let Some(meta) = registry::find_spec_contest(id) {
        return SpecDrivenContest::new(meta)
            .map(|c| Box::new(c) as Box<dyn ContestEntry>);
    }
    // Then hand-coded
    match id.to_ascii_lowercase().as_str() {
        "sweeps" => Some(Box::new(sweeps::SweepsContest)),
        "mst" => Some(Box::new(mst::MstContest)),
        _ => None,
    }
}
```

`SpecDrivenContest::new` returns `Option` because it may fail to find the
corresponding spec in contest-engine's embedded specs. That's the runtime
safety net: if someone adds a registry entry for a contest ID that has no
spec in contest-engine, `contest_from_id` returns `None` gracefully instead
of panicking.

## Adding a new contest — the experience

### Spec-driven path (the common case)

1. Write `contest-engine/specs/foo.json` with the scoring rules
2. Add `"foo" => FOO_JSON` arm in `contest_engine::spec::embedded::spec_by_id`
3. Commit and push contest-engine
4. `cargo update -p contest-engine` in clogger
5. Add one entry to `SPEC_CONTESTS` in `logger-core/src/contest/registry.rs`
6. Done. TUI automatically picks up the new contest. Scoring works.

### Hand-coded path (rare, for complex validation)

1. (Optional) Add spec to contest-engine for scoring
2. Write `logger-core/src/contest/foo.rs` with a struct implementing `ContestEntry`
3. Add to `contest_from_id` dispatcher
4. (If custom scoring needed) add a scorer to `logger-runtime/src/scoring/`

In both paths: one logical change with clear steps. No duplicate JSON files.
No "edit X and Y and Z to keep them in sync." No new files in the spec-driven
path — just one row added to a table.

## Architectural decision: where does `SpecDrivenContest` live?

**Decision: `logger-core` depends on `contest-engine`** and `SpecDrivenContest`
lives in `logger-core/src/contest/spec_driven.rs`.

Reasoning:
- All contest definitions (spec-driven and hand-coded) live in one place
- `contest_from_id` has one obvious location
- Registry is near `ContestEntry` trait itself
- "Where do I add a new contest?" has a single answer: `logger-core/src/contest/`

Rejected alternative: put `SpecDrivenContest` in `logger-runtime`. This would
keep `logger-core` minimal but splits contest definitions across two crates,
which confuses the "adding a contest" workflow. The weight of adding
contest-engine as a dep on logger-core is small — contest-engine's
`embedded` module is pure compile-time data with no IO, async, or hardware,
so it doesn't violate logger-core's "no IO" intent.

## Minor deferred questions

These don't block the refactor and can be decided later:

1. **Multiple contest variants sharing one spec** — e.g., `cqww_cw` and
   `cqww_ssb` both using `cqww.json`. Add an optional `spec_id` field to
   `SpecContestMeta` defaulting to `contest_id` if needed. Not currently
   needed.

2. **Field widths as UI hints in contest-engine specs** — considered and
   rejected. Widths are TUI-specific; a GUI would want different values.
   Clogger ownership is correct.

3. **User-configurable default macros via TOML** — already supported.
   Registry provides the fallback; user TOML overrides win.

4. **Promoting MST to spec-based** — requires writing `mst.json` in
   contest-engine. Out of scope for this refactor. Do it later if desired.

5. **Contest instance IDs as stable hashes instead of clogger-assigned
   u64s** — would break existing DBs. Stay with current assignment.

## Implementation order

1. Add `contest-engine` dep to `logger-core/Cargo.toml`
2. Add `contest_name()` method to `ContestEntry` trait
3. Update `sweeps.rs` and `mst.rs` to implement `contest_name()`
4. Create `logger-core/src/contest/spec_driven.rs`:
   - `SpecDrivenContest` struct and impl
   - Helper functions `derive_form_spec`, `validate_against_spec`,
     `build_draft_from_spec`
5. Create `logger-core/src/contest/registry.rs`:
   - `SpecContestMeta` struct
   - `SPEC_CONTESTS` table with CQWW and CWT entries
   - `find_spec_contest(id)` lookup function
   - Macro constructor functions (`cqww_macros`, `cwt_macros`)
6. Update `logger-core/src/contest/mod.rs`:
   - `contest_from_id` dispatches to registry first, then hand-coded
   - Register the new submodules
7. Delete `logger-core/specs/cqww.json`
8. Delete `logger-core/specs/cwt.json`
9. Delete `logger-core/src/contest/cqww.rs`
10. Delete `logger-core/src/contest/cwt.rs`
11. Wire `contest_name()` through to TUI status bar display
    - Add `contest_name: String` field to `TuiState`
    - Populate from `contest.contest_name().to_string()` during init
    - Render in `status_bar.rs` next to `my_call`

## Verification

- `cargo build` succeeds with the new dep
- `cargo test` — all 32 existing tests pass
- Manual test: launch TUI with a CQWW config, verify contest name "CQWW DX
  Contest" (or whatever the spec `name` field holds) appears in the status
  bar
- Manual test: launch with `contest = "sweeps"` and `contest = "mst"`, verify
  they still work (the hand-coded path)
- File count change: `-2 JSON, -2 Rust, +3 Rust = net -1 file`

## What's out of scope

- Moving MST or Sweepstakes to spec-based (future, separate work)
- Extending contest-engine's spec schema to support Sweepstakes-style
  custom validators (future, requires schema design)
- Per-contest TOML override for metadata beyond what's already supported
  (macros) — not needed right now
- GUI display concerns — the architecture supports them; when the GUI is
  built, it reads the same registry / metadata
