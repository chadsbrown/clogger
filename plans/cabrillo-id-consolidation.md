# Plan: Consolidate contest naming around `cabrillo_contest`

Status: **in progress**. Step 1 queued for contest-engine.

## Principle

One name per (contest, mode) pair, sourced from the contest-engine spec's
`cabrillo_contest` (top-level) and `variants.<mode>.cabrillo_contest`
(per-mode). Both Cabrillo export and RTC upload read from it. Drift
between outputs becomes impossible because there's only one place to
edit.

## What this replaces

Today the same string lives in up to four places:

| Location | Field |
|---|---|
| contest-engine spec | `cabrillo_contest` |
| contest-engine spec | `variants.<mode>.cabrillo_contest` |
| clogger registry | `cabrillo_id_fn` closure |
| clogger registry | `rtc_id_fn` closure |

And for **CWT** they've already drifted — spec says `"CWT"`, clogger's
Cabrillo closure says `"CW-OPS"`, and the RTC closure says `"CW-Ops"`.

## Steps

### Step 1 — contest-engine (first to land)

Lives at `~/src/contest-engine`.

- Fix `specs/cwt.json`: `"cabrillo_contest": "CWT"` → `"CW-OPS"`.
- Audit every other `specs/*.json` for `cabrillo_contest` and
  `variants.<mode>.cabrillo_contest` against the canonical list at
  https://www.contestcalendar.com/cabnames.php. Fix mismatches.
- Add `contest-engine/docs/cabrillo-contest-names.md` with a dated
  mirror of the cabnames list and a source link.
- Optional: add a one-line comment at the top of each spec referencing
  the docs file so contest authors have the reference nearby.

User commits + pushes when step 1 is done; clogger's `Cargo.lock`
refreshes to pull in the new contest-engine rev before step 2.

### Step 2 — drop `rtc_id_fn` in clogger

- Delete `ContestMeta.rtc_id_fn` and the MST `rtc_id_fn` entry.
- Delete `ContestEntry::rtc_id(mode)` trait method and its
  `SpecDrivenContest` impl.
- `compose_rtc` in both UIs switches to `contest.cabrillo_id(mode)`
  for the RTC adapter's identifier.
- Rename `RtcSpawnConfig.contest_rtc_id` to e.g.
  `contest_cabrillo_id`, or collapse the field entirely if the
  RTC adapter ends up reading from the same place the scoreboard
  adapter does.
- Gate behavior is unchanged: no Cabrillo name → adapter is skipped.

Net effect: one fewer concept in clogger, and RTC works for every
Cabrillo-named contest immediately.

### Step 3 — drop `cabrillo_id_fn` in clogger

- Delete `ContestMeta.cabrillo_id_fn`.
- Rewrite `SpecDrivenContest::cabrillo_id(mode)` to look up the
  matching `variants.<mode>.cabrillo_contest`, falling back to the
  spec's top-level `cabrillo_contest` when no variant matches.
- Scoreboard XML golden tests catch any byte-level regression.

## Out of scope

- Renaming `spec.id` to cabnames-style identifiers. `spec.id` stays as
  short lowercase slugs (`mst`, `cwt`, `ss`) — it's the qsolog foreign
  key and the user-facing handle in `contest.toml`; changing it would
  trigger a data migration and make per-mode disambiguation harder
  (one spec covers both CW and SSB variants of a dual-mode contest).

## Verification

- `cargo build -p logger-core -p logger-runtime -p logger-cli -p logger-tui -p logger-gui`
- `cargo test --workspace` — all 164 current tests pass
- Manual: start clogger on MST with `[rtc] enabled = true`, confirm
  the RTC badge appears and `rtc.log` receives `<contest>CW-OPS</contest>`
  (or equivalent per the spec fix)
- Spot-check Cabrillo export for a CWT log: header should say
  `CONTEST: CW-OPS`, not `CONTEST: CWT`
