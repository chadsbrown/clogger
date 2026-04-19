//! iced keyboard event → `AppEvent` translator. Mirrors the TUI's
//! `logger-tui::adapters::terminal::map_key_event` so contesting muscle
//! memory survives the GUI port.

use iced::keyboard::{key::Named, Key, Modifiers};
use logger_core::{AppEvent, Key as LKey};

pub fn translate(key: &Key, modifiers: Modifiers, text: Option<&str>) -> Option<AppEvent> {
    let ctrl = modifiers.control() || modifiers.command();
    let alt = modifiers.alt();

    if let Key::Named(named) = key {
        match named {
            // Quit handled by the iced application; no AppEvent for Ctrl-C.
            Named::Enter if alt => return Some(AppEvent::QuickLog),
            Named::Enter => return Some(AppEvent::KeyPress { key: LKey::Enter }),
            Named::Backspace => return Some(AppEvent::KeyPress { key: LKey::Backspace }),
            Named::Escape => return Some(AppEvent::KeyPress { key: LKey::Esc }),
            Named::Tab => return Some(AppEvent::KeyPress { key: LKey::Tab }),
            Named::Space => return Some(AppEvent::KeyPress { key: LKey::Space }),

            Named::ArrowUp if ctrl && alt => return Some(AppEvent::BandmapUp { radio: 2 }),
            Named::ArrowDown if ctrl && alt => return Some(AppEvent::BandmapDown { radio: 2 }),
            Named::ArrowUp if ctrl => return Some(AppEvent::BandmapUp { radio: 1 }),
            Named::ArrowDown if ctrl => return Some(AppEvent::BandmapDown { radio: 1 }),
            Named::ArrowUp => return Some(AppEvent::FocusRadio { radio: 1 }),
            Named::ArrowDown => return Some(AppEvent::FocusRadio { radio: 2 }),
            Named::ArrowLeft => return Some(AppEvent::KeyPress { key: LKey::Left }),
            Named::ArrowRight => return Some(AppEvent::KeyPress { key: LKey::Right }),

            Named::Insert => return Some(AppEvent::ToggleOpMode),
            Named::PageUp => return Some(AppEvent::CwSpeedAdjust { delta: 2 }),
            Named::PageDown => return Some(AppEvent::CwSpeedAdjust { delta: -2 }),

            Named::F1 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF1 }),
            Named::F2 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF2 }),
            Named::F3 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF3 }),
            Named::F4 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF4 }),
            Named::F5 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF5 }),
            Named::F6 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF6 }),
            Named::F7 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF7 }),
            Named::F8 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF8 }),
            Named::F9 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF9 }),
            Named::F10 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF10 }),
            Named::F11 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF11 }),
            Named::F12 if ctrl && alt => return Some(AppEvent::KeyPress { key: LKey::CtrlAltF12 }),

            Named::F1 => return Some(AppEvent::KeyPress { key: LKey::F1 }),
            Named::F2 => return Some(AppEvent::KeyPress { key: LKey::F2 }),
            Named::F3 => return Some(AppEvent::KeyPress { key: LKey::F3 }),
            Named::F4 => return Some(AppEvent::KeyPress { key: LKey::F4 }),
            Named::F5 => return Some(AppEvent::KeyPress { key: LKey::F5 }),
            Named::F7 => return Some(AppEvent::KeyPress { key: LKey::F7 }),
            Named::F8 => return Some(AppEvent::KeyPress { key: LKey::F8 }),
            Named::F9 => return Some(AppEvent::KeyPress { key: LKey::F9 }),
            Named::F12 => return Some(AppEvent::KeyPress { key: LKey::F12 }),

            _ => {}
        }
    }

    // SCP cycle on bare "=" — TUI parity. The reducer treats Key::Equal as
    // "advance through scp_matches, replacing the CALL field." Intercept
    // it BEFORE the generic text path so the reducer doesn't get a
    // TextInput { s: "=" } that would just append the literal character.
    // Modifier-held variants (Ctrl-= for font scale) are handled above.
    if !ctrl && !alt && text == Some("=") {
        return Some(AppEvent::KeyPress { key: LKey::Equal });
    }

    // Fall through to character text input for printable keys. Skip when
    // a modifier other than Shift is held (e.g. Ctrl-A) so the reducer
    // doesn't see "A" pasted into the entry box for shortcut chords.
    if ctrl || alt {
        return None;
    }
    let text = text?;
    let s: String = text
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if s.is_empty() {
        return None;
    }
    Some(AppEvent::TextInput { s })
}
