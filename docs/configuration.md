# Configuration Reference

clogger reads **two** TOML files at startup:

- **`config.toml`** — the *stable* config: station identity, hardware,
  scoreboard endpoints, UI preferences. Rarely changes between
  contests.
- **`contest.toml`** — the *per-contest* config: contest id, sent
  exchange, database path, CW macros, Cabrillo category. Switch files
  when you switch contests.

Both files are passed on the command line. The GUI and TUI front-ends
accept the same flags:

```
logger-gui --config /path/to/config.toml --contest /path/to/contest.toml
logger-tui --config /path/to/config.toml --contest /path/to/contest.toml
```

Copy `config.example.toml` and `contest.example.toml` as starting
points — they include commented-out examples of every section.

---

## `config.toml` reference

### Station identity

| Field | Type | Default | Notes |
|---|---|---|---|
| `my_call` | string | required | Your callsign |
| `my_zone` | int | 0 | CQ zone |
| `rst_sent` | string | `"599"` | Default RST to seed the entry field |
| `my_name` | string | — | Used by CWT, MST, NAQP, NS Sprint |

### Database lookups

| Field | Type | Default | Notes |
|---|---|---|---|
| `scp_file` | path | — | SCP (Super Check Partial) database for callsign suggestions |
| `cty_file` | path | — | cty.dat (Big CTY format) — DXCC entity resolver. Required if any `geo.*` rule is used in your dxfeed filter; also improves mult scoring accuracy |

### UI

| Field | Type | Default | Notes |
|---|---|---|---|
| `theme` | string | `"default"` | Built-in theme name. See [Themes](#themes) |
| `theme_file` | path | — | TOML overlay on top of base theme |
| `cursor_style` | enum | `blinking_block` | Options: `blinking_block`, `steady_block`, `blinking_underline`, `steady_underline`, `blinking_bar`, `steady_bar` |
| `bandmap` | enum | `dual` | `dual` (R1 + R2 stacked), `r1`, `r2` |
| `show_passband_qrm` | bool | false | Show `QRM` badge in Run mode when a bandmap spot is within the focused radio's receive passband |
| `bandmap_skip_worked` | bool | false | Ctrl-↑/↓ (and Ctrl-Alt-↑/↓ for R2) skip already-worked stations |
| `bandmap_high_at_top` | bool | false | Reverse bandmap orientation (highest freq at top); Ctrl-↑/↓ follow visual direction |
| `esm_enabled` | bool | true | Enter-sends-message flow (see [Operating](operating.md)) |
| `block_dupes` | bool | false | Refuse to log/send via Enter when the entry call is a known dupe on current band/mode |

### Rigs

Use `[[rig]]` array-of-tables for one or two rigs (SO2R).

```toml
[[rig]]
radio_id = 1          # 1 or 2
model = "ic7300"      # riglib model id
port = "/dev/ttyUSB0"
baud_rate = 115200
cw_speed = 28         # optional, used by `<`/`>` macros
transceive = false    # enable Auto-Information / broadcast events
```

`transceive = true` makes clogger stop polling freq/mode and consume
the rig's broadcast events instead — avoids bus collisions that cause
front-panel lag on half-duplex radios. On Icom you must also enable
CI-V Transceive in the radio menu; on Yaesu/Kenwood/Elecraft riglib
sends the AI-enable command itself.

### Keyer

```toml
[keyer]
port = "/dev/ttyUSB1"
speed_wpm = 28
contest_spacing = true    # tight inter-character timing
cw_echo = true            # show characters as they send
sidetone = true           # let WinKeyer drive its speaker
```

### SO2R switch (OTRSP)

```toml
[so2r]
port = "/dev/ttyUSB3"
default_rx_mode = "stereo"   # mono | stereo | reverse_stereo
```

SO2R affects RX audio routing; TX routing follows whichever radio
the keyer is sending to. Toggle mono/stereo at runtime with the
backtick key (`` ` ``). See [Operating — SO2R](operating.md#so2r).

### DX feed

```toml
[[dxfeed.sources]]
host = "dxc.nc0d.com"
port = 7300
callsign = "N9UNX"

[dxfeed]
filter_file = "/path/to/filter.json"

[dxfeed.skimmer_quality]
# busted-call / corroboration tuning
```

The `filter_file` is a JSON document; the `[dxfeed.skimmer_quality]`
block is a set of TOML overrides. See **[DX feed tuning](dxfeed-tuning.md)**
for the full reference. Most of the nuance (how busted detection
works, unknown-policy, geo filters needing cty.dat) is documented
there.

### Propagation panel (CondX)

```toml
[condx]
enabled = true
refresh_interval_secs = 3600    # default; minimum 60
```

Pulls solar/band-conditions data from hamqsl.com hourly. Off by
default — an offline contest station shouldn't make unexpected HTTP
requests. Panel renders at the top of the left column when enabled.

### Scoreboard posting

```toml
[scoreboard]
interval_secs = 120

[[scoreboard.endpoints]]
url = "https://scoreboard.example.com/post"
password = "secret"
```

`[[scoreboard.endpoints]]` can repeat for multi-posting. Requires
`[category]` in `contest.toml` to generate a valid XML payload. Spec:
[contestonlinescore.com](https://blog.contestonlinescore.com/online-scoring-xml-specification/).

### Cabrillo metadata

```toml
[cabrillo]
club = "Yankee Clipper Contest Club"
operators = ["N9UNX"]
name = "Chad"
address = ["123 Main St", "Anytown ST 12345"]
email = "n9unx@example.com"
soapbox = ["Great conditions!", "First SO2R attempt"]
```

Used by Cabrillo export (export modal) and the online scoreboard.

---

## `contest.toml` reference

### Core

| Field | Type | Notes |
|---|---|---|
| `contest` | string | Contest id — see [Contests](contests.md) |
| `my_xchg` | string | Generic sent-exchange. Meaning depends on contest: CWT → membership number, NAQP → state, state QP → county or state |
| `db_path` | path | SQLite log file. **Per-contest per-run.** Without this, every restart is a fresh log |
| `call_history_file` | path | `.ch` file for exchange pre-population |

`my_xchg` appears in macros as `{MYXCHG}` (and also `{MYLOC}` for
compatibility with specs that use the `my_loc` field name).

### `[station]` typed passthrough

For contests whose scoring spec requires typed config beyond the
basic name / zone / xchg set — primarily state QSO parties. Every
key is fed verbatim to the scoring engine.

Example — in-state Florida QSO Party entry from Alachua county:

```toml
[station]
my_is_fl = true
my_county = "ALC"
```

Example — out-of-state New Mexico QSO Party entry, low power:

```toml
[station]
my_is_nm = false
my_loc = "NC"
my_power_class = "LOW"
```

Supported value types: bool, integer, string. Arrays, floats, and
datetimes are rejected at load. Per-contest required fields are
listed in [Contests](contests.md).

### `[macros]` — CW macros

F-key overrides. If a key isn't listed, the contest's default (or an
empty string) is used.

```toml
[macros]
f1 = "CQ TEST {MYCALL}"
f2 = ">{RST_SENT} {MYZONE}<"    # wrap in < > to slow down for exchange
f3 = "TU {MYCALL}"
f5 = "{CALL}"
f7 = "?"
f8 = "NR {MYZONE} {MYZONE}"
f9 = "QRL?"
ctrl_alt_f1 = "QRL?"
# ... ctrl_alt_f1 through ctrl_alt_f12 available
```

Placeholders and speed markers are documented in [Operating — CW macros](operating.md#cw-macros).

### `[category]` — Cabrillo class

Required when a scoreboard endpoint is configured; also used in
Cabrillo export.

```toml
[category]
power = "HIGH"           # LOW | HIGH | QRP
assisted = "ASSISTED"    # NON-ASSISTED | ASSISTED
transmitter = "ONE"      # UNLIMITED | TWO | ONE
operator = "SINGLE-OP"   # MULTI-OP | SINGLE-OP | MULTI-ONE | MULTI-TWO | MULTI-MULTI
bands = "ALL"            # ALL | 160M | 80M | 40M | 20M | 15M | 10M | 6M | 2M
mode = "CW"              # MIXED | CW | SSB | RTTY | DIGI
overlay = "N/A"          # N/A | TB-WIRES | ROOKIE | CLASSIC | WIRE-ONLY
```

---

## Themes

clogger ships 16 built-in themes — names you can drop into `theme`:
`default`, `dracula`, `one-dark-pro`, `nord`, `catppuccin-mocha`,
`catppuccin-latte`, `gruvbox-dark`, `gruvbox-light`, `tokyo-night`,
`solarized-dark`, `solarized-light`, `monokai-pro`, `rose-pine`,
`kanagawa`, `everforest`, `cyberpunk`.

A custom `theme_file` is a TOML with per-role hex colors. It overlays
on top of the named base theme, so you only set the roles you want to
change. See the theme loader source for the role list; common ones
are `background`, `entry_border_focused`, `field_label`, `field_valid`,
`field_invalid`, `bandmap_mult`, `status_callsign_badge`.

---

## CLI overrides

Every path-valued TOML field has a matching CLI flag that wins at load
time. Handy for one-off swaps (e.g. trying a different SCP file) without
editing the config.

| Flag | Overrides |
|---|---|
| `--db PATH` | `contest.toml` `db_path` |
| `--call-history PATH` | `contest.toml` `call_history_file` |
| `--scp PATH` | `config.toml` `scp_file` |
| `--cty PATH` | `config.toml` `cty_file` |
| `--debug` | — (enables debug-level logging to `clogger.log`) |
