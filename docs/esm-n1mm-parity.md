# ESM — N1MM+ parity comparison

clogger's ESM (Enter Sends Message) is modeled on N1MM+'s so operators
with muscle memory from N1MM+ don't get surprised. This document
captures — for each user-visible ESM behavior — whether clogger matches
N1MM+, differs deliberately, or has a gap worth filling later.

For the internal ESM state machine, see [esm.md](esm.md). For the
serial-number lifecycle that sits underneath most ESM transitions, see
[serial-numbers.md](serial-numbers.md).

## Core ESM semantics — matches

| Behavior | clogger | N1MM+ |
|---|---|---|
| Enter drives a state machine that walks a QSO from start to commit | ✅ | ✅ |
| Run mode is two-step: first Enter sends call + exchange, second Enter logs and sends TU | ✅ | ✅ |
| S&P mode commits atomically at the Enter past the CALL field | ✅ | ✅ |
| F-keys bypass ESM state and can be pressed at any time | ✅ | ✅ |
| `{SERIAL}` / `#` macro: current serial when CALL non-empty, previous serial when empty | ✅ | ✅ |
| Serial reservation timing (Run keystroke, S&P cursor-leave, F-key send) | ✅ | ✅ |
| Wipe rolls back the serial counter (SO2R-safe) | ✅ (F12) | ✅ (Alt+W) |
| Space advances focus and skips fields that don't normally need to change (RST) | ✅ | ✅ |

## Material differences

| # | Aspect | N1MM+ | clogger |
|---|---|---|---|
| 1 | **Runtime ESM toggle** | `Ctrl+M` turns ESM on/off during a contest | No runtime toggle; set `esm_enabled` in `config.toml` and restart |
| 2 | **Per-mode ESM state** | Independent on/off per CW / Phone / Digital | Single global flag |
| 3 | **Visual "next message" indicator** | Highlights (configurable color) the F-key button that the next Enter will fire; "Log It" button also lights at commit step | **No visual indicator.** Clogger operators depend on mode awareness and muscle memory |
| 4 | **Per-mode macro sets** | Separate 12-key sets for Run vs S&P, and separate for CW / SSB / RTTY (up to 24 per mode) | One shared set (`f1`..`f9`, `ctrl_alt_f1..12`). Only `sp_f2` exists as an S&P-specific override |
| 5 | **S&P "Big Gun / Little Pistol" switch** | Config option controls whether Enter in CALL field advances cursor to exchange (Big Gun) or keeps it on CALL for repeat-MYCALL (Little Pistol) | Hardcoded to Little Pistol behavior — Enter on CALL repeats MYCALL and leaves cursor on CALL; cursor only advances via Tab or Space |
| 6 | **CALL edit after first Enter in Run** | Operator guidance is to manually press F5 + F2 to resend the corrected call + exchange, then Enter to log. Implies ESM state is not auto-reset on CALL edit | CALL edit unconditionally resets `esm_step` to `Idle` (`reducer.rs` `touched_call` branch). The next Enter re-fires Enter #1, automatically re-sending call+exchange with the preserved serial |
| 7 | **Wipe with undo** | `Alt+W` is reversible — restores the last wiped contact if nothing new has been entered since. `Ctrl+W` is irreversible | F12 is the only wipe; **no "unwipe."** See [serial-numbers.md — F12 rollback](serial-numbers.md#f12-rollback-semantics) for the counter-rollback story |
| 8 | **Dupe handling under ESM** | "Work dupes when running" config option determines what Enter sends when a dupe calls you (can still work them, or refuse) | `block_dupes` config: when true, Enter beeps and refuses; F-keys are always unaffected so the operator can still confirm the dupe on air |
| 9 | **QuickLog / log without CW** | No direct documented equivalent found | `Alt+Enter` logs the QSO without emitting any CW (`AppEvent::QuickLog` → `quick_log` in `esm.rs`) |
| 10 | **Auto-toggle mode (Sprint)** | "Sprint mode" auto-flips Run↔S&P after logging, per NA Sprint rules | `auto_toggle_mode` per-contest flag; currently only NS Sprint enables it |
| 11 | **Macro `{ENTER}` / macro chaining** | Macros can embed `{ENTER}` to script full sequences | Clogger macros do not support embedded Enter |

## Gaps worth flagging

The differences above that most affect an N1MM+-trained operator landing in clogger:

- **No visual indicator of next-Enter action (#3).** N1MM+ operators rely on the highlighted F-key button to know whether the next Enter will CQ, send call+exchange, or log+TU. Without it, clogger users have to hold the state in their head. Candidate future feature — would live in the TUI/GUI entry panel.
- **No unwipe (#7).** `Alt+W` in N1MM+ catches "oh I didn't mean to F12." Clogger's F12 is terminal for the entry's field values, though the serial counter is rolled back.
- **No runtime ESM toggle (#1).** Minor. Most operators pick a mode and stick with it.

## Notable places where clogger is more automatic than N1MM+

- **CALL-edit auto-resend (#6).** Editing CALL in clogger resets `esm_step`, so the next Enter re-sends call+exchange with the preserved serial. N1MM+ requires manually pressing F5 + F2. Same on-air result; fewer keystrokes in clogger. Watch-out: received exchange fields (NR / NAME / LOC / etc.) are retained across the edit, so this is only safe if those fields are empty or actually correct for the new call. If in doubt, F12 and start over.
- **F-key serial parity.** Clogger claims a serial both at Run-mode first-keystroke (matching N1MM+) AND at F-key macro expansion with `{SERIAL}` + non-empty CALL (as a safety net). N1MM+ achieves this via the early-keystroke reservation alone.
- **QuickLog (#9).** `Alt+Enter` as an escape hatch for "I sent the exchange by paddle, just log it."

## Sources

- [N1MM+ — Function Keys, Messages and Macros](https://n1mmwp.hamdocs.com/setup/function-keys/) — ESM section, macro reference, F-key highlight
- [N1MM+ — Operating a Contest](https://n1mmwp.hamdocs.com/getting-started/operating-a-contest/) — ESM overview
- [N1MM+ — The Entry Window](https://n1mmwp.hamdocs.com/manual-windows/entry-window/) — `Ctrl+M` toggle, per-mode ESM
- [N1MM+ — Key Assignments / Keyboard Shortcuts](https://n1mmwp.hamdocs.com/setup/keyboard-shortcuts/) — `Alt+W` / `Ctrl+W` wipe semantics
- [N1MM+ — FAQ](https://n1mmwp.hamdocs.com/faq/) — Big Gun / Little Pistol switch; "Work dupes when running"
- [Rob Locher — N1MM Morse Contest Notes](https://www.roblocher.com/technotes/n1mm-morse-notes.html) — hands-on ESM Run / S&P walkthrough, CALL-edit guidance
