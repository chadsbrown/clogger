# UI domain validation limitation — analysis and options

## The limitation in precise terms

`SpecDrivenContest::validate_field` (`logger-core/src/contest/spec_driven.rs:198`) matches on `FieldType::Rst` and `FieldType::Int` but has a catch-all `_ => Validation::Valid`. That catch-all swallows `FieldType::Enum` and `FieldType::Text`, which are the types used for state/province/country/county/section/name fields across every spec-driven contest. Any non-empty string passes.

Consequences per contest type:

| Contest  | Enum/Text field      | Current UI behavior                   |
|----------|----------------------|---------------------------------------|
| NAQP     | LOC (state/province) | Any 1-10 char string accepted         |
| Sweeps   | SECT (ARRL section)  | Any string accepted                   |
| State QPs| LOC (county or state)| Any string accepted                   |
| CWT      | NAME (free text)     | Any string accepted — intentional     |

## Important nuance: where the domain lives

Contest-engine's `ExchangeField` has an optional `domain: Option<DomainRef>`. Two shapes:

1. **Field-level domain set** — e.g., Sweeps `sect` field points directly at `ss_sections` in its own definition. UI can look it up and check membership trivially.
2. **Field-level domain `null`, domain implied by multipliers** — the **state QP pattern**. FLQP's `loc` field has `domain: null`. The "valid" set depends entirely on which multiplier variants are active, which depends on config predicates (`my_is_fl`). A rover's county is valid out-of-state; a US state is valid in-state; both have to be typed into the same UI field.

This distinction matters because the easy fix (option 1 below) covers case 1 but does **nothing** for case 2 — and the state QPs are case 2, which is the whole reason this conversation started.

## What the user experiences today

1. Types "DAB" instead of "DAD" as a Florida county.
2. No red flag, no beep, no underline. ESM advances.
3. QSO logs with `loc = "DAB"`.
4. Scoreboard still reads N mults (silent loss of credit).
5. Typo survives until Cabrillo export → contest sponsor's log-checker flags it post-contest.

No in-session feedback at all. The user has to notice that the scoreboard didn't increment or look carefully at the log tail.

## Options

### Option 1 — field-level enum check only

Extend `validate_field` to handle `FieldType::Enum` + `FieldType::Text` when `spec_field.domain` is `Some`. For `DomainRef::List`, do an inline membership check; for `DomainRef::External { name }`, look up via `contest_engine::spec::embedded::domain_by_name`. ~30 LOC in one file.

**Helps NAQP, Sweeps, and any spec that attaches a field-level domain. Does nothing for state QPs** because their LOC fields have `domain: null`.

### Option 2 — contest-engine API for "effective field domain"

Add something like `ContestSpec::effective_domain_for_field(field_id, &config) -> Option<HashSet<String>>` that resolves the union of domains from every multiplier variant whose `when` predicate is satisfied by the current config. In clogger, call it at session bootstrap (config is static for the session) and store the HashSet on `SpecDrivenContest`. Validation then checks membership against the cached set.

Handles state QPs correctly: an out-of-state FLQP op gets `{ALC, BAK, ...}`; an in-state op gets `{states ∪ provinces ∪ DXCC}`. Moderate contest-engine work + 50-80 LOC in clogger.

### Option 3 — warn, don't block

Independent axis: whether "not in domain" returns `Invalid` (red flag, ESM blocks) or a new `Warning` variant (yellow flag, ESM advances). Safer when the embedded domain list goes stale (contest-engine release lag vs new counties added by the sponsor). Can layer on top of 1 or 2.

Small core change — `Validation` enum gains a third variant, reducer treats it like `Valid` for ESM purposes but the UI renders differently.

### Option 4 — post-log diff warning

Don't touch input validation. Instead, after logging each QSO, check whether the claimed_score increased by the expected amount (QSO points + any new mults). If not, flash a "⚠ no mult credit" hint in the log tail row for 3-5 seconds.

Doesn't help typing correctness directly but provides the "something felt wrong" feedback loop without blocking. Maybe 40 LOC; requires snapshotting score before/after insert.

### Option 5 — multi-value-sep awareness (FLQP rover)

If any of 1/2/3 land, they need to split on `multi_value_sep` before validating each piece, otherwise `ALC/BAK` fails the domain check as a single token. Trivial once the base is in place.

### Option 6 — autocomplete/candidate popup

Show a small dropdown with domain entries that match the current prefix. Closer to SCP for callsigns. Biggest UX win but most implementation work; justifies its own task rather than being bundled into "validation."

## Recommendation

If you care mostly about NAQP and Sweeps typos: **Option 1** — one afternoon, low risk, covers the cases where contest-engine specs already carry field-level domains.

If you care about state QPs (which is why this analysis exists): **Option 2 + Option 5 + Option 3 (as warning, not block)**. The warning disposition is important — embedded domain files drift against contest sponsors in practice, and hard-blocking a valid-but-not-yet-in-the-list county would be worse than the current silent acceptance.

**Option 4** as an orthogonal tool is genuinely attractive and cheap. It doesn't replace 1/2 but it's the only option that catches *both* domain typos *and* other silent scoring failures (wrong dupe key, busted sent exchange, etc.) without touching the entry path. Worth landing independently.

**Option 6** — park unless you want to invest in it as a standalone feature.
