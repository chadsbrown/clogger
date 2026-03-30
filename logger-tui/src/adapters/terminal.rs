use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use logger_core::{AppEvent, Key, OpMode};
use tokio::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};

pub enum TerminalEvent {
    App(AppEvent),
    Shutdown,
}

pub fn spawn_terminal_reader(tx: mpsc::Sender<TerminalEvent>) {
    std::thread::spawn(move || {
        static IS_RUN: AtomicBool = AtomicBool::new(true);
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
                (_, KeyCode::Insert) => {
                    let was_run = IS_RUN.fetch_xor(true, Ordering::Relaxed);
                    let mode = if was_run { OpMode::Sp } else { OpMode::Run };
                    TerminalEvent::App(AppEvent::SetOpMode { mode })
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
                (_, KeyCode::Tab) => TerminalEvent::App(AppEvent::KeyPress { key: Key::Tab }),
                (_, KeyCode::F(1)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F1 }),
                (_, KeyCode::F(2)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F2 }),
                (_, KeyCode::F(3)) => TerminalEvent::App(AppEvent::KeyPress { key: Key::F3 }),
                _ => continue,
            };
            if tx.blocking_send(terminal_event).is_err() {
                break;
            }
        }
    });
}
