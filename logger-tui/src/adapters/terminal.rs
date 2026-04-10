use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use logger_core::{AppEvent, Key};
use tokio::sync::mpsc;

pub enum TerminalEvent {
    App(AppEvent),
    Shutdown,
    OpenExportModal,
}

pub fn spawn_terminal_reader(tx: mpsc::Sender<TerminalEvent>) {
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
                (m, KeyCode::Up) if m.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                    TerminalEvent::App(AppEvent::BandmapUp { radio: 2 })
                }
                (m, KeyCode::Down) if m.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
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
                (_, KeyCode::Down) => {
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
                (_, KeyCode::Char(c)) => TerminalEvent::App(AppEvent::TextInput {
                    s: c.to_uppercase().to_string(),
                }),
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
