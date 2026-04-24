use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use logger_core::{AppEvent, Key};
use tokio::sync::mpsc;

pub enum TerminalEvent {
    App(AppEvent),
    Shutdown,
    OpenExportModal,
    /// Toggle the OTRSP RX audio routing between mono and stereo. Handled
    /// directly by the event loop; does not flow through the reducer because
    /// RX audio routing is a runtime/UI concern, not part of the contest state
    /// machine.
    ToggleRxMode,
}

/// Spawn the terminal input reader.
///
/// `has_second_rig` controls whether R2-specific key bindings are honored.
/// When false (single-rig or headless setup), the down-arrow focus switch and
/// the Ctrl+Alt+arrow R2 bandmap scrolls are dropped — the R2 entry box isn't
/// rendered in that configuration, so binding those keys would just shift
/// invisible state. Captured at spawn time because the rig roster is fixed
/// for the life of the process.
pub fn spawn_terminal_reader(tx: mpsc::Sender<TerminalEvent>, has_second_rig: bool) {
    std::thread::spawn(move || {
        loop {
            let Ok(ev) = event::read() else {
                break;
            };
            let Event::Key(key_ev) = ev else {
                continue;
            };
            if key_ev.kind != KeyEventKind::Press {
                continue;
            }
            let Some(terminal_event) = map_key_event(key_ev, has_second_rig) else {
                continue;
            };
            if tx.blocking_send(terminal_event).is_err() {
                break;
            }
        }
    });
}

/// Pure key → TerminalEvent mapping. Lifted out of the spawned reader
/// so it's unit-testable without crossterm stdin / a real terminal.
/// `None` means the key press isn't one clogger reacts to — the caller
/// should just drop it and keep reading.
pub(crate) fn map_key_event(key_ev: KeyEvent, has_second_rig: bool) -> Option<TerminalEvent> {
    let evt = match (key_ev.modifiers, key_ev.code) {
                (m, KeyCode::Char('c')) if m.contains(KeyModifiers::CONTROL) => {
                    TerminalEvent::Shutdown
                }
                (m, KeyCode::Char('e')) if m.contains(KeyModifiers::CONTROL) => {
                TerminalEvent::OpenExportModal
            }
                (m, KeyCode::Enter) if m.contains(KeyModifiers::ALT) => {
                    // QuickLog: log the current entry without emitting CW.
                    // Must precede the bare `KeyCode::Enter` arm below —
                    // match arms are evaluated top-to-bottom, and modifier-
                    // bearing matches have to come first or the bare arm
                    // would shadow them. Ctrl+Plus was the historical
                    // binding, but most terminals eat Ctrl+Plus for
                    // font-size zoom, so this binding never actually
                    // reached crossterm in practice.
                    TerminalEvent::App(AppEvent::QuickLog)
                }
                (m, KeyCode::Up)
                    if has_second_rig
                        && m.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    TerminalEvent::App(AppEvent::BandmapUp { radio: 2 })
                }
                (m, KeyCode::Down)
                    if has_second_rig
                        && m.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    TerminalEvent::App(AppEvent::BandmapDown { radio: 2 })
                }
                (m, KeyCode::Up) if m.contains(KeyModifiers::CONTROL) => {
                    TerminalEvent::App(AppEvent::BandmapUp { radio: 1 })
                }
                (m, KeyCode::Down) if m.contains(KeyModifiers::CONTROL) => {
                    TerminalEvent::App(AppEvent::BandmapDown { radio: 1 })
                }
                (_, KeyCode::Up) => {
                    TerminalEvent::App(AppEvent::FocusRadio { radio: 1 })
                }
                (_, KeyCode::Down) if has_second_rig => {
                    TerminalEvent::App(AppEvent::FocusRadio { radio: 2 })
                }
                (_, KeyCode::Insert) => {
                    TerminalEvent::App(AppEvent::ToggleOpMode)
                }
                // PageUp/PageDown: ±2 WPM on the focused radio. Matches
                // N1MM+'s bare-key convention. Modified variants (Shift for
                // secondary step, Alt for inactive radio, Ctrl for band
                // stepping) aren't wired up yet — easy to add here.
                (_, KeyCode::PageUp) => {
                    TerminalEvent::App(AppEvent::CwSpeedAdjust { delta: 2 })
                }
                (_, KeyCode::PageDown) => {
                    TerminalEvent::App(AppEvent::CwSpeedAdjust { delta: -2 })
                }
                (_, KeyCode::Char(' ')) => {
                    TerminalEvent::App(AppEvent::KeyPress { key: Key::Space })
                }
                (_, KeyCode::Char('=')) => {
                    TerminalEvent::App(AppEvent::KeyPress { key: Key::Equal })
                }
                // Backtick: toggle OTRSP RX audio routing (mono <-> stereo).
                // Intercepted here so it never reaches the entry field as text.
                (_, KeyCode::Char('`')) => TerminalEvent::ToggleRxMode,
                (_, KeyCode::Char(c)) => {
                    // `char::to_ascii_uppercase` is a single-branch bit op
                    // on the char — no iterator overhead from the generic
                    // `to_uppercase()`. Callsigns are ASCII-only, and the
                    // reducer's CALL-field invariant only requires ASCII
                    // uppercase. `char::to_string()` builds a minimally-sized
                    // String via a stack utf-8 encode.
                    let s = c.to_ascii_uppercase().to_string();
                    TerminalEvent::App(AppEvent::TextInput { s })
                }
                (_, KeyCode::Enter) => TerminalEvent::App(AppEvent::KeyPress { key: Key::Enter }),
                (_, KeyCode::Backspace) => TerminalEvent::App(AppEvent::KeyPress {
                    key: Key::Backspace,
                }),
                (_, KeyCode::Esc) => TerminalEvent::App(AppEvent::KeyPress { key: Key::Esc }),
                (_, KeyCode::Left) => TerminalEvent::App(AppEvent::KeyPress { key: Key::Left }),
                (_, KeyCode::Right) => TerminalEvent::App(AppEvent::KeyPress { key: Key::Right }),
                (_, KeyCode::Tab) => TerminalEvent::App(AppEvent::KeyPress { key: Key::Tab }),
                (m, KeyCode::F(n)) if m.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                    let key = match n {
                        1 => Key::CtrlAltF1,
                        2 => Key::CtrlAltF2,
                        3 => Key::CtrlAltF3,
                        4 => Key::CtrlAltF4,
                        5 => Key::CtrlAltF5,
                        6 => Key::CtrlAltF6,
                        7 => Key::CtrlAltF7,
                        8 => Key::CtrlAltF8,
                        9 => Key::CtrlAltF9,
                        10 => Key::CtrlAltF10,
                        11 => Key::CtrlAltF11,
                        12 => Key::CtrlAltF12,
                        _ => return None,
                    };
                    TerminalEvent::App(AppEvent::KeyPress { key })
                }
                (_, KeyCode::F(1)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F1 }),
                (_, KeyCode::F(2)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F2 }),
                (_, KeyCode::F(3)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F3 }),
                (_, KeyCode::F(4)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F4 }),
                (_, KeyCode::F(5)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F5 }),
                (_, KeyCode::F(6)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F6 }),
                (_, KeyCode::F(7)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F7 }),
                (_, KeyCode::F(8)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F8 }),
                (_, KeyCode::F(9)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F9 }),
                (_, KeyCode::F(10)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F10 }),
                (_, KeyCode::F(11)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F11 }),
                (_, KeyCode::F(12)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F12 }),
                _ => return None,
    };
    Some(evt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn alt_enter_maps_to_quicklog() {
        let ev = map_key_event(key(KeyCode::Enter, KeyModifiers::ALT), true)
            .expect("Alt+Enter should map");
        match ev {
            TerminalEvent::App(AppEvent::QuickLog) => {}
            _ => panic!("expected QuickLog"),
        }
    }

    #[test]
    fn plain_enter_is_not_quicklog() {
        // Bare Enter must fall through to the normal ESM/Enter path,
        // not be shadowed by the QuickLog arm.
        let ev = map_key_event(key(KeyCode::Enter, KeyModifiers::NONE), true)
            .expect("Enter should map");
        match ev {
            TerminalEvent::App(AppEvent::KeyPress { key: Key::Enter }) => {}
            TerminalEvent::App(AppEvent::QuickLog) => {
                panic!("plain Enter must not map to QuickLog")
            }
            _ => panic!("expected Enter keypress, not QuickLog"),
        }
    }

    #[test]
    fn ctrl_plus_no_longer_triggers_quicklog() {
        // The old binding (Ctrl+Plus) was never reachable through most
        // terminals because they intercept it for font zoom. Now that
        // we've moved off it, confirm nothing else caught it — it
        // should fall through to the generic Char path (which uppercases
        // to '+').
        let ev = map_key_event(key(KeyCode::Char('+'), KeyModifiers::CONTROL), true);
        if let Some(TerminalEvent::App(AppEvent::QuickLog)) = ev {
            panic!("Ctrl+Plus must no longer fire QuickLog")
        }
    }
}
