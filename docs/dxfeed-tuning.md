# DX Feed Tuning Guide

clogger connects to DX cluster nodes (including the Reverse Beacon
Network) via the `dxfeed` library. Spots flow through two independent
filtering stages before reaching the bandmap:

1. **Skimmer quality engine** — corroboration, busted-call detection,
   and QSY tagging. Configured in `config.toml` via
   `[dxfeed.skimmer_quality]`.
2. **Filter pipeline** — band/mode, callsign, spotter, SNR/WPM, geo,
   enrichment, and more. Configured via a JSON file pointed to by
   `filter_file` in `config.toml`.

The quality engine runs first (in the aggregator, before the filter
pipeline). Spots that survive both stages are emitted to clogger's
bandmap.

This document walks through common tuning scenarios. For the full
reference on every field and enum value, see the dxfeed library's
[CONFIGURATION.md](https://github.com/chadsbrown/dxfeed/blob/master/doc/CONFIGURATION.md).

---

## Quick start: reducing noisy RBN spots

The most common complaint is busted callsigns from RBN skimmers that
aren't copying well. Three dials, in order of impact:

### 1. Require the call to be in SCP (highest leverage)

If `scp_file` is set in `config.toml`, clogger automatically provides
the SCP master database to the filter pipeline's enrichment layer. Add
this to your `filter.json`:

```json
{
  "enrichment": {
    "in_master_db": "RequireTrue"
  }
}
```

And point clogger at it:

```toml
[dxfeed]
filter_file = "/path/to/filter.json"
```

Any spotted call that isn't in the SCP file is dropped. Since busted RBN
copies are almost never valid contest callsigns, this is the single most
effective structural filter.

**Caveat:** calls not yet in SCP (new licensees, rare DX) will also be
dropped. If you're chasing rare DX rather than contesting, use `"Any"`
instead and rely on the other filters below.

### 2. Tighten skimmer corroboration

The quality engine requires 3 distinct skimmers to agree on a call
within 300 Hz and 180 seconds before marking it "Valid" (the only tag
that passes by default). Tighten these in `config.toml`:

```toml
[dxfeed.skimmer_quality]
valid_required_distinct_skimmers = 5   # default 3
valid_freq_window_hz = 150             # default 300
lookback_window_secs = 90              # default 180
```

More skimmers required = fewer weak-signal flukes getting through.
Shorter windows = stale corroborations expire faster. Trade-off: you'll
see fewer spots during slow propagation.

### 3. Set an SNR floor

RBN skimmer spots carry an SNR value. Low-SNR spots are the most likely
to be misdecoded. In your `filter.json`:

```json
{
  "skimmer": {
    "snr_db": [8, 80]
  }
}
```

This drops any RBN spot with SNR below 8 dB. Adjust the minimum up or
down based on band conditions — 6 dB is permissive, 12 dB is strict.

**Note:** SNR filtering only applies to skimmer-originated spots. Human
cluster spots don't carry SNR and pass through unaffected.

---

## Filtering by band and mode

Use the `rf` section in `filter.json` to restrict which bands and modes
reach the bandmap:

```json
{
  "rf": {
    "band_deny": ["B160", "B6"],
    "mode_allow": ["CW"]
  }
}
```

This drops all 160m and 6m spots, and only allows CW spots through.
Useful during a single-mode contest to avoid SSB clutter.

Band names: `B160`, `B80`, `B60`, `B40`, `B30`, `B20`, `B17`, `B15`,
`B12`, `B10`, `B6`.

Mode names: `CW`, `SSB`, `DIG`, `AM`, `FM`.

---

## Blocking specific spotters

If a particular skimmer or human spotter consistently produces bad data:

```json
{
  "spotter": {
    "callsign": {
      "deny": {
        "globs": ["KX3XX*"],
        "regexes": []
      }
    }
  }
}
```

Or block all human spots and only accept skimmers:

```json
{
  "spotter": {
    "allow_human": false,
    "allow_skimmer": true
  }
}
```

---

## Requiring CQ-only spots

If you only want to see stations calling CQ (useful in S&P mode during a
contest), set this in `filter.json`:

```json
{
  "skimmer": {
    "require_cq": true
  }
}
```

This drops skimmer spots that detected a station in QSO rather than
calling CQ.

---

## Geographic filtering (continents, zones, entities)

The `geo.dx.*` and `geo.spotter.*` sections in `filter.json` filter by
resolved geographic data — continent, CQ zone, ITU zone, DXCC entity.
These rules **require** a cty.dat file to be loaded; without it, every
spot's geo fields resolve to None, and any allowlist silently drops every
spot (dxfeed's default `unknown_policy` is `Neutral`, which treats
unknown-on-allowlist as a drop).

Configure the database in `config.toml`:

```toml
cty_file = "/path/to/cty.dat"
```

Get cty.dat from [country-files.com](https://www.country-files.com/).

Then use geo rules in `filter.json`:

```json
{
  "geo": {
    "dx": {
      "continent_allow": ["NA"]
    }
  }
}
```

If the filter uses any geo rule and no `cty_file` is configured, clogger
refuses to start — the silent-drop failure mode this prevents is
otherwise invisible.

Continent codes: `NA`, `SA`, `EU`, `AF`, `AS`, `OC`, `AN`.

**Fidelity caveats** (current limitations of the station-data backing):

- `entity_allow` / `entity_deny` match against the canonical DXCC prefix
  (e.g. `"W"`, `"JA"`) rather than the full entity name
  (`"United States"`). Use the prefix form until station-data exposes
  names on lookup.
- `country_allow` / `country_deny` and `state_allow` / `state_deny` have
  no data source wired — any spot passed to those rules resolves to None
  and drops under `Neutral` unknown policy.
- Zones not listed in cty.dat resolve to `0` rather than unknown, so a
  `cq_zone_allow: [5]` rule will reject stations whose zone cty.dat
  doesn't specify.

---

## Enrichment: LoTW, callbook, memberships

The `enrichment` section in `filter.json` can filter on metadata about
the spotted callsign:

| Signal | Filter values | Data source |
|---|---|---|
| `in_master_db` | `"RequireTrue"`, `"RequireFalse"`, `"Any"` | SCP file (automatic when `scp_file` is set) |
| `lotw` | `"RequireTrue"`, `"RequireFalse"`, `"Any"` | Not yet wired — returns unknown |
| `in_callbook` | `"RequireTrue"`, `"RequireFalse"`, `"Any"` | Not yet wired — returns unknown |
| `membership` | `[{"Require": "TAG"}, {"Deny": "TAG"}]` | Not yet wired — returns unknown |

When a signal returns "unknown" (no data source wired), the behavior
depends on `unknown_policy` at the top of `filter.json`:

- `"Neutral"` (default) — allowlists: drop (not proven in set);
  denylists: pass (not proven denied). This means `"RequireTrue"` on
  an unwired signal will drop every spot. Don't set `"RequireTrue"` on
  signals without a resolver.
- `"FailOpen"` — unknown always passes. Safer when some data sources are
  unavailable.
- `"FailClosed"` — unknown always drops. Maximum strictness.

### Example: awards chasing

Show only stations that are in SCP and confirmed LoTW users (once the
LoTW resolver is wired):

```json
{
  "enrichment": {
    "in_master_db": "RequireTrue",
    "lotw": "RequireTrue"
  }
}
```

### Example: CWT contest

Show only CWops members:

```json
{
  "enrichment": {
    "membership": [{"Require": "CWOPS"}]
  }
}
```

(Requires a CWops membership resolver to be wired.)

---

## Skimmer quality engine reference

The `[dxfeed.skimmer_quality]` section in `config.toml` controls the
quality engine that runs *before* the filter pipeline. Every field is
optional; omitted fields use the default.

### How it works

When RBN spots arrive, the engine accumulates observations per callsign
within a frequency/time window. Once enough distinct skimmers agree, the
call is marked **Valid**. Subsequent single-skimmer spots at the same
frequency that are one edit-distance away from a verified call are tagged
**Busted** and dropped.

### All knobs

| Field | Default | Effect of increasing |
|---|---|---|
| `valid_required_distinct_skimmers` | `3` | Fewer false positives, slower to confirm |
| `valid_freq_window_hz` | `300` | Wider = more forgiving frequency spread |
| `lookback_window_secs` | `180` | Longer = more time to accumulate skimmers |
| `busted_freq_window_hz` | `100` | Wider = catch busted copies further from verified freq |
| `similar_call_max_edit_distance` | `1` | Higher = catch more typos, but also more false positives |
| `qsy_freq_window_hz` | `400` | Wider = more tolerance before tagging a call as QSY |
| `apply_only_to_skimmer` | `true` | `false` = also gate human spots (aggressive) |

### Gating tags

| Tag | Default | Meaning |
|---|---|---|
| `allow_valid` | `true` | Corroborated by enough skimmers |
| `allow_busted` | `false` | One-character typo of a verified call |
| `allow_unknown` | `false` | Not enough data to classify |
| `allow_qsy` | `false` | Verified call heard far from its last freq |

### Example: aggressive noise reduction

```toml
[dxfeed.skimmer_quality]
valid_required_distinct_skimmers = 5
valid_freq_window_hz = 150
lookback_window_secs = 90
apply_only_to_skimmer = false
```

Combined with a `filter.json`:

```json
{
  "skimmer": { "snr_db": [10, 80] },
  "enrichment": { "in_master_db": "RequireTrue" }
}
```

This is maximal strictness: 5-skimmer corroboration, tight frequency
window, SNR floor of 10 dB, and every call must be in SCP. Human
spots are also gated. Your bandmap will be clean but sparse.

---

## Startup behavior

- **Bad `filter_file`** (missing file, invalid JSON, schema error) →
  clogger refuses to start. This is intentional: an operator who asked
  for a filter shouldn't unknowingly run a contest without one.
- **Bad `[dxfeed.skimmer_quality]` values** (zero corroborators, negative
  freq windows) → clogger refuses to start.
- **Typos in field names** → clogger refuses to start
  (`deny_unknown_fields` catches misspelled TOML keys).
- **Cluster unreachable** (network error, auth failure) → clogger starts
  normally. Connection status appears in the status bar (GUI and TUI). The
  dxfeed task retries with exponential backoff.
- **No `filter_file` or `[dxfeed.skimmer_quality]`** → dxfeed defaults
  apply (3-skimmer corroboration, busted-call rejection, no filter
  pipeline). This is the current behavior if you don't configure
  anything.
