# Supported Contests

Every contest is identified by a short string you set in `contest.toml`
as `contest = "..."`. This doc lists what clogger supports today, the
exchange shape, and any contest-specific config needed in `[station]`.

For the mechanics of adding a new contest, see
[Adding contests](adding-contests.md).

---

## Major contests

### CQWW DX

- **id:** `cqww`
- **Exchange:** RST + CQ zone
- **Needs:** `my_zone` in `config.toml`
- **Macros (default F2):** `{RST_SENT} {MYZONE}`

### ARRL DX

- **id:** `arrl_dx`
- **Exchange:** RST + state/province (W/VE) or RST + power (DX)
- **Needs:** `my_xchg` — your state or power (e.g. `"CT"` or `"100"`)

### ARRL Sweepstakes

- **id:** `ss` (alias: `sweeps`)
- **Exchange:** serial + precedence + call + check + section
- **Needs:** nothing special; serial is tracked automatically

### NAQP

- **id:** `naqp`
- **Exchange:** name + state/province
- **Needs:** `my_name`, `my_xchg` (your state/province)

### CWT (CWops Mini-Test)

- **id:** `cwt`
- **Exchange:** name + CWops number (or state/prov for non-members)
- **Needs:** `my_name`, `my_xchg` (your CWops number or state)
- **Macros (default F2):** `{MYNAME} {MYXCHG}`

### MST (Medium Speed Test)

- **id:** `mst`
- **Exchange:** name + serial
- **Needs:** `my_name`

### NS Sprint

- **id:** `ns_sprint`
- **Exchange:** serial + name + state/province
- **Needs:** `my_name`, `my_xchg`
- **Quirks:** auto-toggles Run/S&P mode (Sprint QSY rule)

---

## State / Province QSO parties

Thirteen are wired up. Each has an **in-state** mode (you send your
county) and an **out-of-state** mode (you send your state/province).

| id | Contest |
|---|---|
| `flqp` | Florida QSO Party |
| `gaqp` | Georgia QSO Party |
| `inqp` | Indiana QSO Party |
| `miqp` | Michigan QSO Party |
| `moqp` | Missouri QSO Party |
| `ndqp` | North Dakota QSO Party |
| `nhqp` | New Hampshire QSO Party |
| `nmqp` | New Mexico QSO Party |
| `neqp` | New England QSO Party (uses `my_is_ne`) |
| `neqsop` | Nebraska QSO Party (also uses `my_is_ne`) |
| `onqp` | Ontario QSO Party |
| `qcqp` | Quebec QSO Party |
| `deqp` | Delaware QSO Party |

### `[station]` config — required fields

For every state QP, in `contest.toml`:

```toml
[station]
my_is_<xx> = true|false      # true = operating from inside the state
```

The `<xx>` suffix matches the contest id except for the two NE ones,
which both use `my_is_ne` (contest-specific interpretation):

| Contest | Key |
|---|---|
| `flqp` | `my_is_fl` |
| `gaqp` | `my_is_ga` |
| `inqp` | `my_is_in` |
| `miqp` | `my_is_mi` |
| `moqp` | `my_is_mo` |
| `ndqp` | `my_is_nd` |
| `nhqp` | `my_is_nh` |
| `nmqp` | `my_is_nm` |
| `neqp` | `my_is_ne` |
| `neqsop` | `my_is_ne` |
| `onqp` | `my_is_on` |
| `qcqp` | `my_is_qc` |
| `deqp` | `my_is_de` |

Then one of the location fields depending on in/out-of-state:

- **In-state (`my_is_<xx> = true`)**: need a county (or equivalent):
  - Most contests: `my_county = "ABC"` (3-letter county code per
    contest sponsor's list).
  - Ontario (`onqp`): `my_area = "..."` instead of county.
  - Quebec (`qcqp`): `my_region = "..."` instead of county.

- **Out-of-state (`my_is_<xx> = false`)**: `my_loc = "NC"` (your
  state/province two-letter code, or `"DX"`).

### Power-class contests

NMQP, DEQP, and NEQSOP apply a score multiplier based on your power
class:

```toml
[station]
my_power_class = "LOW"   # QRP (5x) | LOW (2x) | HIGH (1x)
```

Required for those three; other state QPs ignore it.

### Example: in-state Michigan QSO Party

```toml
contest = "miqp"
my_xchg = "WAYN"           # appears as {MYXCHG} in macros

[station]
my_is_mi = true
my_county = "WAYN"
```

### Example: out-of-state MIQP from North Carolina

```toml
contest = "miqp"
my_xchg = "NC"

[station]
my_is_mi = false
my_loc = "NC"
```

Note `my_xchg` and `my_loc` are set to the same value here. That's
because `my_xchg` feeds the `{MYXCHG}` macro token (what you send),
while `my_loc` is the scoring-engine-facing field. Both need the
value; see the note in
[Operating — CW macros](operating.md#cw-macros) about how `my_xchg`
and `my_loc` interact.

---

## Picking the right contest id

If you're unsure, `contest = "cqww"` is the smallest exchange (RST +
zone) and works as a sanity-check. For anything else, consult the
table above and match by name.

Unknown contest ids fail at startup with an explicit error — clogger
won't silently load a generic fallback.
