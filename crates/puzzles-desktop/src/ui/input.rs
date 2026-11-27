//! Keyboard and mouse events to the games' [`Button`]s
//! (`docs/porting-plan.md` §4.5). Typed characters come from egui's text
//! events, so they follow the keyboard layout; other keys from key
//! events. App shortcuts are taken out before this sees the events.
//!
//! Mice without a middle or right button: Alt/Option-click is a middle
//! click everywhere, and Ctrl-click a right click on macOS (the system
//! convention there).

use eframe::egui;
use egui::{Key, PointerButton};
use puzzles_core::button::{Button, CursorKey, Modifiers, MouseButton};

/// The modifiers games see: Shift, and Ctrl (also Cmd on macOS).
fn modifiers(m: egui::Modifiers) -> Modifiers {
    let mut mods = Modifiers::empty();
    if m.shift {
        mods |= Modifiers::SHIFT;
    }
    if m.ctrl || m.mac_cmd {
        mods |= Modifiers::CTRL;
    }
    mods
}

/// The button for a key that types no character, if games use it.
#[must_use]
pub fn key_button(key: Key, m: egui::Modifiers) -> Option<Button> {
    let button = match key {
        Key::ArrowUp => Button::cursor(CursorKey::Up),
        Key::ArrowDown => Button::cursor(CursorKey::Down),
        Key::ArrowLeft => Button::cursor(CursorKey::Left),
        Key::ArrowRight => Button::cursor(CursorKey::Right),
        Key::Enter => Button::cursor(CursorKey::Select),
        Key::Backspace => Button::key('\u{8}'),
        Key::Delete => Button::key('\u{7f}'),
        Key::Tab => Button::key('\t'),
        Key::Escape => Button::key('\u{1b}'),
        _ => return None,
    };
    Some(button.with(modifiers(m)))
}

/// The buttons for typed text: printable ASCII only, as games know no
/// other keys. A space is `CURSOR_SELECT2` once the mid-end reads it.
pub fn text_buttons(text: &str) -> impl Iterator<Item = Button> + '_ {
    text.chars()
        .filter(|c| (' '..='~').contains(c))
        .map(Button::key)
}

/// The game's mouse button for a pointer button with `m` held.
#[must_use]
pub fn mouse_button(button: PointerButton, m: egui::Modifiers, mac: bool) -> Option<MouseButton> {
    match button {
        PointerButton::Primary if m.alt => Some(MouseButton::Middle),
        PointerButton::Primary if mac && m.ctrl => Some(MouseButton::Right),
        PointerButton::Primary => Some(MouseButton::Left),
        PointerButton::Secondary => Some(MouseButton::Right),
        PointerButton::Middle => Some(MouseButton::Middle),
        PointerButton::Extra1 | PointerButton::Extra2 => None,
    }
}

#[cfg(test)]
mod tests {
    use puzzles_core::button::ButtonKind;

    use super::*;

    const NONE: egui::Modifiers = egui::Modifiers::NONE;

    #[test]
    fn arrows_keep_shift_and_ctrl() {
        assert_eq!(
            key_button(Key::ArrowLeft, NONE),
            Some(Button::cursor(CursorKey::Left))
        );
        let shift_ctrl = egui::Modifiers {
            shift: true,
            ctrl: true,
            ..NONE
        };
        assert_eq!(
            key_button(Key::ArrowUp, shift_ctrl).unwrap().mods,
            Modifiers::SHIFT | Modifiers::CTRL
        );
        let cmd = egui::Modifiers {
            mac_cmd: true,
            ..NONE
        };
        assert_eq!(key_button(Key::ArrowUp, cmd).unwrap().mods, Modifiers::CTRL);
    }

    #[test]
    fn editing_keys_are_c_characters() {
        assert_eq!(
            key_button(Key::Enter, NONE),
            Some(Button::cursor(CursorKey::Select))
        );
        assert_eq!(key_button(Key::Backspace, NONE), Some(Button::key('\u{8}')));
        assert_eq!(key_button(Key::Delete, NONE), Some(Button::key('\u{7f}')));
        assert_eq!(key_button(Key::A, NONE), None, "letters come as text");
        assert_eq!(key_button(Key::F5, NONE), None);
    }

    #[test]
    fn text_becomes_keys() {
        let keys: Vec<_> = text_buttons("a5 Z").map(|b| b.kind).collect();
        assert_eq!(
            keys,
            ['a', '5', ' ', 'Z'].map(ButtonKind::Key),
            "a space is a key too"
        );
        assert_eq!(text_buttons("é×\n").count(), 0);
    }

    #[test]
    fn one_button_mice_reach_every_button() {
        let alt = egui::Modifiers { alt: true, ..NONE };
        let ctrl = egui::Modifiers { ctrl: true, ..NONE };
        let primary = PointerButton::Primary;
        assert_eq!(mouse_button(primary, NONE, true), Some(MouseButton::Left));
        assert_eq!(mouse_button(primary, alt, false), Some(MouseButton::Middle));
        assert_eq!(mouse_button(primary, ctrl, true), Some(MouseButton::Right));
        assert_eq!(mouse_button(primary, ctrl, false), Some(MouseButton::Left));
        assert_eq!(
            mouse_button(PointerButton::Secondary, NONE, false),
            Some(MouseButton::Right)
        );
        assert_eq!(mouse_button(PointerButton::Extra1, NONE, false), None);
    }
}
