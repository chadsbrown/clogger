use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
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
            let terminal_event = match (key_ev.modifiers, key_ev.code) {
                (m, KeyCode::Char('c')) if m.contains(KeyModifiers::CONTROL) => {
                    TerminalEvent::Shutdown
                }
                (m, KeyCode::Char('e')) if m.contains(KeyModifiers::CONTROL) => {
                TerminalEvent::OpenExportModal
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
                (_, KeyCode::F(1)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F1 }),
                (_, KeyCode::F(2)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F2 }),
                (_, KeyCode::F(3)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F3 }),
                (_, KeyCode::F(5)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F5 }),
                (_, KeyCode::F(7)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F7 }),
                (_, KeyCode::F(8)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F8 }),
                (_, KeyCode::F(9)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F9 }),
                (_, KeyCode::F(12)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F12 }),
                _ => continue,
            };
            if tx.blocking_send(terminal_event).is_err() {
                break;
            }
        }
    });
}
