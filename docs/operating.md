# Operating clogger

This doc covers the runtime workflow: keybindings, ESM, CW macros,
SO2R mechanics, and what each panel shows. Keybindings and ESM apply
to both the GUI and the TUI — they share the same input semantics.
The panel diagram below is the TUI's fixed layout; the GUI renders
the same panels as floating, draggable, resizable MDI panes.

See also: [Configuration](configuration.md),
[Contests](contests.md), [DX feed tuning](dxfeed-tuning.md).

## Panel layout (TUI)

```
┌──────────────┬──────────────────┬────────────────┐
│              │  Log tail (max   │                │
│   CondX      │   10 rows)       │   R1 Bandmap   │
│   (if on)    ├──────────────────┤                │
│              │  R1 Entry        │                │
├──────────────┤                  ├────────────────┤
│   Score      │  R2 Entry        │   R2 Bandmap   │
├──────────────┤  (when 2 rigs)   │                │
│   Avail      ├──────────────────┤                │
├──────────────┤  SCP / N+1       │                │
│   Rate       │                  │                │
├──────────────┤                  │                │
│  SO2R (if on)│                  │                │
└──────────────┴──────────────────┴────────────────┘
│ status bar (call, MULT/DUPE/QRM, clock, RIG/KEY/DXF/SO2R/SCRBD, …)  │
│ footer (keybinding hint strip)                                     │
```

Left column changes composition based on what's configured:
**CondX** appears at top only when `[condx] enabled = true`.
**SO2R** appears at bottom only when `[so2r]` is configured.

## Keybindings

### Running / S&P flow

| Key | Action |
|---|---|
| `Enter` | ESM — advances the send-and-log state machine |
| `Alt+Enter` | Log-only: insert the QSO into the log without CW |
| `Insert` | Toggle between Run and S&P mode |
| `Esc` | Stop CW / abort an in-flight transmission |
| `F12` | Wipe the current entry (clear all fields; RST auto-refills) |

### CW macros (focused radio)

| Key | Action |
|---|---|
| `F1` | CQ |
| `F2` | Exchange |
| `F3` | TU |
| `F5` | Call |
| `F7` / `F8` / `F9` | Free slots — empty by default, set via `[macros]` in `contest.toml` |
| `Ctrl+Alt+F1`..`F12` | 12 extra user-defined macro slots |

F1–F3 and F5 have reasonable defaults per contest (e.g. CWT F2 sends
`{MYNAME} {MYXCHG}`). F7–F9 are empty until you configure them. See
[Configuration — `[macros]`](configuration.md#macros--cw-macros).

### Entry-field navigation

| Key | Action |
|---|---|
| `Tab` | Next field |
| `Space` | Next field (on RST contests: skips RST once) |
| `Backspace` | Delete char |
| `←` / `→` | Move cursor within field |
| `=` | Cycle through SCP matches for the current call |

### Radio / bandmap

| Key | Action |
|---|---|
| `↑` | Focus Radio 1 |
| `↓` | Focus Radio 2 (only when a second rig is configured) |
| `Ctrl+↑` / `Ctrl+↓` | Bandmap step for R1 (tune rig to next/prev spot) |
| `Ctrl+Alt+↑` / `Ctrl+Alt+↓` | Bandmap step for R2 |
| `` ` `` (backtick) | Toggle SO2R RX mono/stereo |
| `PageUp` / `PageDown` | CW speed ±2 WPM on focused radio |

### App-level

| Key | Action |
|---|---|
| `Ctrl+E` | Open export modal |
| `Ctrl+C` | Quit |

### Text input

Any other character — inserted into the focused entry field,
uppercased. The CALL field stays ASCII-only.

## Command-line flags

| Flag | Required | Description |
|---|---|---|
| `--config PATH` | yes | Stable `config.toml` |
| `--contest PATH` | yes | Per-contest `contest.toml` |
| `--db PATH` | no | Overrides `db_path` from contest.toml |
| `--call-history PATH` | no | Overrides `call_history_file` |
| `--scp PATH` | no | Overrides `scp_file` |
| `--cty PATH` | no | Overrides `cty_file` |
| `--debug` | no | Enable debug-level logging to `clogger.log` |

## ESM (Enter Sends Message)

ESM is a state machine on the Enter key. What it does depends on your
operating mode and which fields you've filled.

### Run mode (you're calling CQ)

1. **Enter #1** — with a call and exchange in the entry:
   sends `{CALL} {exchange}` (F2 content expanded). The QSO is not
   yet logged.
2. **Enter #2** — after the return exchange arrives:
   sends TU (F3), inserts the QSO into the log, clears the entry,
   RST repopulates.

### S&P mode (you're hunting)

1. Cursor in CALL field, Enter: sends your call (F5 by default =
   `{CALL}`) — actually `{MYCALL}` because you're responding to a
   CQer by transmitting your own call. Exact F5 content depends on
   config.
2. Cursor past CALL (exchange typed), Enter: sends your exchange
   (F2), logs the QSO, clears the entry.

Tune ESM by:
- Setting `esm_enabled = false` in `config.toml` to disable (Enter
  becomes log-only, you send CW exclusively via F-keys).
- Setting `block_dupes = true` to make Enter beep+refuse when the
  call is a known dupe on the current band/mode. F-keys still work,
  so you can confirm the dupe over the air.

## CW macros

### Placeholders

| Token | Expands to |
|---|---|
| `{MYCALL}` | Your call (`my_call` from config.toml) |
| `{MYZONE}` | Your CQ zone |
| `{RST_SENT}` | Your sent RST (`rst_sent` from config.toml) |
| `{CALL}` | The call in the entry's CALL field |
| `{SERIAL}` | Current serial number (for contests that use one) |
| `{MY<KEY>}` | e.g. `{MYNAME}`, `{MYXCHG}`, `{MYLOC}` — looked up from the `my_exchange` map |
| `{LABEL}` | Any received-exchange field value, by label (e.g. `{RST}`, `{NR}`, `{LOC}`) |

### Speed markers

- `<` — drop 2 WPM (stackable: `<<` = drop 4)
- `>` — raise 2 WPM
- `{0}` — explicit restore to default
- `<AR>`, `<SK>`, `<BT>`, `<KN>`, `<AS>` — prosigns (preserved)

Examples:

```
F1 = "CQ TEST {MYCALL}"
F2 = ">{RST_SENT} {MYZONE}<"    # faster for the exchange, then back
F3 = "TU {MYCALL}"
F5 = "{CALL}"
```

## SO2R

When `[so2r]` is configured, the left column shows a compact SO2R
panel:

```
┌ SO2R ─────┐
│ FOCUS: R1 │
│ RX: R1/R2 │
│ TX:    R1 │
└───────────┘
```

- **FOCUS** — which radio the entry box is bound to. Flip with
  `↑` / `↓`.
- **RX** — `Rn/Rm` where `n` is the left ear and `m` is the right
  ear. Mono mode duplicates the focused radio to both ears.
  Reverse-stereo flips R1/R2.
- **TX** — which radio the keyer is currently routed to. Normally
  follows FOCUS; briefly differs when cross-radio CW is in flight.

The `` ` `` (backtick) key toggles mono ↔ stereo. Reverse-stereo is
config-only (`default_rx_mode = "reverse_stereo"`); pressing ``` ` ```
from there collapses to mono on the next toggle.

## Bandmap

Each bandmap panel shows dedupe'd spots for its radio's current band +
mode, colored by classification:

- **white** — unworked, not a mult
- **bright** — would be a new multiplier
- **dim** — already worked
- **highlight** — cursor position (call you just navigated to or
  spot the rig is tuned inside of)
- **divider line** — rig is parked in clear air between spots; the
  line marks the insertion point

The cursor anchors on the callsign, not a list index, so new spots
arriving or old ones expiring don't drift the cursor off the station
you're trying to work.

`Ctrl+↑` / `Ctrl+↓` (R1) step to the next/prev spot and tune the rig.
`Ctrl+Alt+↑` / `Ctrl+Alt+↓` do the same for R2.

### Skip-worked mode

Set `bandmap_skip_worked = true` in `config.toml` to make the nav keys
automatically skip already-worked stations. If every spot in the
current band+mode is a dupe, the nav key becomes a no-op — nothing
moves, a useful "I've swept this list" signal.

### Reversed display

Set `bandmap_high_at_top = true` in `config.toml` to flip the bandmap
so the highest frequency appears at the top of the panel. Ctrl-↑/↓
keep their visual semantics — Ctrl-↓ always moves the highlight
visually downward in the panel, which now corresponds to lower
frequencies.

## Status bar

Left side shows your call + contest name, and either a `MULT` or
`DUPE` badge (they're mutually exclusive) based on the current entry.
Plus a `QRM` badge when `show_passband_qrm` is on and a spot overlaps
your passband.

Center: UTC clock.

Right: connection health indicators — green (configured + connected),
red (configured + not connected), omitted (not configured):
- `RIG` / `RIG1` / `RIG2` — rig control
- `KEY` — keyer
- `DXF` — dxfeed cluster
- `SO2R` — OTRSP switch
- `SCRBD` — scoreboard (yellow = idle, green = ok, red = failing)

## Export (Ctrl+E)

Opens a modal with format choices:

- **ADIF** — writes `.adi` with every non-voided QSO. Contest-spec
  mode tags are included.
- Cabrillo — not yet implemented.

## Rate panel

The left-column Rate box shows:

- **Last 10** — minutes to log the last 10 QSOs.
- **Last 100** — minutes to log the last 100.
- **Rate** — QSOs/hour extrapolated from the last 10.
- **Since** — seconds since the most recent logged QSO (`-` when
  empty, `45s` under a minute, `3m45s` under an hour, `1h23m` otherwise).

## Panel: CondX

When enabled, shows solar/propagation data pulled hourly from
hamqsl.com — SFI, sunspots, A/K indices, X-ray flux, geomagnetic
field label, signal/noise rating, and per-band-pair condition
ratings (day/night). Requires network access at startup and every
refresh interval. See the `[condx]` section in
[Configuration](configuration.md#propagation-panel-condx).
