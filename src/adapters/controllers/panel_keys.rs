//! The terminal's key and mouse events, as the panel understands them.

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};

use crate::adapters::presenters::panel::Key;
use crate::entities::Action;

/// `None` for events the panel ignores (releases, moves, resizes).
pub fn key_for(event: &Event) -> Option<Key> {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            Some(match (key.code, control) {
                (KeyCode::Char('k' | 'c'), true)
                | (KeyCode::Esc, _)
                | (KeyCode::Char('q'), false) => Key::Close,
                (KeyCode::Char('w'), false) => Key::Section(Action::Why),
                (KeyCode::Char('f'), false) => Key::Section(Action::Fix),
                (KeyCode::Char('a'), false) => Key::Section(Action::Agent),
                (KeyCode::Char('i'), false) => Key::Section(Action::Ignore),
                (KeyCode::Char('p'), false) => Key::Section(Action::Privacy),
                (KeyCode::Tab | KeyCode::Right, _) => Key::Next,
                (KeyCode::BackTab | KeyCode::Left, _) => Key::Prev,
                (KeyCode::Up, _) => Key::Up,
                (KeyCode::Down, _) => Key::Down,
                (KeyCode::PageUp, _) => Key::PageUp,
                (KeyCode::PageDown, _) => Key::PageDown,
                (KeyCode::Enter, _) => Key::Enter,
                (KeyCode::Char('c'), false) => Key::Copy,
                (KeyCode::Char('?'), _) => Key::Help,
                _ => return None,
            })
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(Key::Up),
            MouseEventKind::ScrollDown => Some(Key::Down),
            MouseEventKind::Down(MouseButton::Left) => Some(Key::Click {
                column: mouse.column,
                row: mouse.row,
            }),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn letters_jump_control_k_and_escape_close_and_tab_moves_on() {
        assert_eq!(
            key_for(&key(KeyCode::Char('w'), KeyModifiers::NONE)),
            Some(Key::Section(Action::Why))
        );
        assert_eq!(
            key_for(&key(KeyCode::Char('p'), KeyModifiers::NONE)),
            Some(Key::Section(Action::Privacy))
        );
        assert_eq!(
            key_for(&key(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            Some(Key::Close)
        );
        assert_eq!(
            key_for(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Key::Close)
        );
        assert_eq!(
            key_for(&key(KeyCode::Char('c'), KeyModifiers::NONE)),
            Some(Key::Copy)
        );
        assert_eq!(
            key_for(&key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Key::Close)
        );
        assert_eq!(
            key_for(&key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Key::Close)
        );
        assert_eq!(
            key_for(&key(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Key::Next)
        );
        assert_eq!(
            key_for(&key(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(Key::Prev)
        );
        assert_eq!(
            key_for(&key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Key::Enter)
        );
        assert_eq!(
            key_for(&key(KeyCode::Char('?'), KeyModifiers::SHIFT)),
            Some(Key::Help)
        );
        assert_eq!(key_for(&key(KeyCode::Char('z'), KeyModifiers::NONE)), None);
        assert_eq!(key_for(&Event::Resize(80, 24)), None);
    }

    #[test]
    fn the_wheel_scrolls_and_a_left_click_lands() {
        let wheel = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 3,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(key_for(&wheel), Some(Key::Down));
        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(key_for(&click), Some(Key::Click { column: 3, row: 4 }));
        let moved = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 3,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(key_for(&moved), None);
    }
}
