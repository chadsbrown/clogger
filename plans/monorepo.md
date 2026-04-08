# Monorepo Consolidation Plan

## Context

Several crates that clogger depends on live as separate GitHub repositories,
currently referenced via local `path = "../../..."` dependencies in
`Cargo.toml` files. These need to become real (non-path) dependencies, and
the question is whether to consolidate the crates into a single monorepo or
keep them as separate repositories with proper git/crates.io dependencies.

External crates currently referenced by path:
- `qsolog` — in-memory QSO store with SQLite journaling
- `contest-engine` — spec-driven contest scoring engine
- `riglib` — rig CAT control (Icom, Yaesu, Elecraft, Kenwood, FlexRadio)
- `winkey` — WinKeyer CW protocol
- `dxfeed` — DX cluster feed parser
- `otrsp` — OTRSP SO2R switch protocol
- `station-data` — station/zone reference data

All are authored by the same developer (sole maintainer). clogger is
currently the only consumer of these crates.

## Options Considered

### Option 1: Full monorepo (workspace)

Pull all crates into the clogger repo as workspace members. Single git
history, single CI, single `cargo test`. Cross-crate API changes land in
one commit. No version skew ever. Publishing to crates.io from a
workspace still works via `cargo publish -p <crate>`.

### Option 2: Separate repos, git/crates.io deps

Keep each crate as its own repo. Switch `path = "..."` to
`git = "..."` (rev-pinned) or crates.io versions. Each repo has its own
README, issues, contributor flow, CI. Cross-crate changes require lockstep
PRs across multiple repos.

### Option 3: Hybrid

Two repos grouped by concern:
- `ham-stack` (or similar): the reusable libraries (qsolog, contest-engine,
  station-data, riglib, winkey, dxfeed, otrsp)
- `clogger`: the application crates that depend on ham-stack

Best of both worlds for the cross-project reusability case, at the cost of
still having cross-repo coordination when ham-stack APIs change.

## Decision: Option 1 (Full Monorepo)

Rationale:
- Single developer, single consumer (clogger is the only project using
  these crates).
- Tightly-coupled crate family — ham radio support libraries serving one
  logger application.
- Velocity is more valuable than independent discoverability for this
  project.
- The Rust ecosystem pattern for tightly-coupled crate families is monorepo
  workspaces (tokio, bevy, axum, embassy, diesel all do this).
- Can always split a crate out later if it picks up external users.
- Publishing to crates.io is still possible from a workspace.

## Migration Plan

Plain file copy, no git history preservation. The original repos will be
archived read-only on GitHub, preserving their history there as a historical
record. Day-to-day work is normal commits in the new monorepo.

### Steps

1. **Pick the umbrella repo name.** Either rename `clogger` → something
   broader like `ham-stack`, or keep `clogger` as the umbrella since it's
   already a workspace. Decision deferred; start with `clogger`.

2. **Copy each crate into the clogger repo.** Delete the `.git/`, `target/`,
   and `Cargo.lock` from each before committing.

   ```sh
   cd ~/src/clogger
   cp -r ../qsolog         qsolog
   cp -r ../contest-engine contest-engine
   cp -r ../riglib         riglib
   cp -r ../winkey         winkey
   cp -r ../dxfeed         dxfeed
   cp -r ../otrsp          otrsp
   cp -r ../station-data   station-data

   rm -rf qsolog/.git qsolog/target qsolog/Cargo.lock
   rm -rf contest-engine/.git contest-engine/target contest-engine/Cargo.lock
   rm -rf riglib/.git riglib/target riglib/Cargo.lock
   rm -rf winkey/.git winkey/target winkey/Cargo.lock
   rm -rf dxfeed/.git dxfeed/target dxfeed/Cargo.lock
   rm -rf otrsp/.git otrsp/target otrsp/Cargo.lock
   rm -rf station-data/.git station-data/target station-data/Cargo.lock
   ```

3. **Update root `Cargo.toml`** to add the new crates as workspace members:

   ```toml
   [workspace]
   members = [
       "logger-core",
       "logger-runtime",
       "logger-cli",
       "logger-tui",
       "qsolog",
       "contest-engine",
       "riglib",
       "winkey",
       "dxfeed",
       "otrsp",
       "station-data",
   ]
   resolver = "2"
   ```

   Note: `riglib` may have a nested structure
   (`../../riglib/crates/riglib`) — check and flatten if needed, or include
   both `riglib` and any sub-crates explicitly.

4. **Update path dependencies** in the consuming crates. The current paths
   reference `../../qsolog` (up two levels from `logger-runtime`, etc.).
   After consolidation they'll be at the sibling level:

   Before:
   ```toml
   qsolog = { path = "../../qsolog" }
   ```

   After:
   ```toml
   qsolog = { path = "../qsolog" }
   ```

   Files to update:
   - `logger-runtime/Cargo.toml` — qsolog, contest-engine, riglib, winkey,
     dxfeed, otrsp, station-data
   - `logger-cli/Cargo.toml` — qsolog
   - Any internal dependencies within the imported crates themselves

5. **Check for inter-crate dependencies inside the imported crates.** For
   example, if `contest-engine` has a dependency on `station-data` via path,
   that path will also need updating post-move.

6. **`cargo build && cargo test`** — verify everything still compiles and
   passes.

7. **Commit the whole import as one or a few logical commits.**

8. **Archive the original GitHub repos.** For each:
   - Edit README to note: "This crate has been merged into the clogger
     monorepo: https://github.com/.../clogger"
   - Archive the repo via GitHub settings (makes it read-only, still visible)

### Optional: use workspace dependencies

Rust 1.64+ supports `workspace.dependencies` to declare versions once at
the workspace root and reference them from members:

```toml
# root Cargo.toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["sync"] }
anyhow = "1"
tracing = "0.1"

# member Cargo.toml
[dependencies]
serde = { workspace = true }
tokio = { workspace = true }
```

This is worth doing at consolidation time since all the imported crates
likely use overlapping sets of common dependencies (serde, tokio, anyhow,
thiserror, tracing). Deduping them in one place reduces drift.

## Verification

- `cargo build --workspace` succeeds for all crates
- `cargo test --workspace` passes (all 29+ existing tests)
- The clogger TUI binary starts and runs the same as before
- `cargo tree` shows no duplicate versions of common dependencies (this is
  where the workspace.dependencies migration pays off)

## Rollback

Since the original GitHub repos are archived (not deleted), rollback is
possible by unarchiving and restoring path deps. The monorepo commit can be
reverted; the only "lost" work is the consolidation itself.

## Open Questions

- Repo name: keep `clogger`, rename to `ham-stack`, or something else?
- Should riglib's nested `crates/riglib` layout be flattened as part of the
  import, or preserved as-is?
- Are any of the imported crates currently consumed by anything other than
  clogger? If yes, that crate should probably stay separate and be consumed
  via git or crates.io rather than imported.
