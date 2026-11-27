//! App shortcuts per OS: Cmd on macOS, Ctrl elsewhere (egui's
//! `Modifiers::COMMAND`). They are taken out of the input before the
//! game sees it.

use eframe::egui;
use egui::{Key, KeyboardShortcut, Modifiers};

use crate::app::command::Command;

/// The shortcuts, in the order to check them. egui ignores an extra
/// Shift on a shortcut that doesn't name it, so the ones with Shift come
/// first (Cmd+Shift+Z is Redo, not Undo).
#[must_use]
pub fn shortcuts(mac: bool) -> Vec<(KeyboardShortcut, Command)> {
    let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
    let mut list = vec![(
        KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
        Command::Redo,
    )];
    if !mac {
        list.push((cmd(Key::Y), Command::Redo));
    }
    list.extend([
        (cmd(Key::Z), Command::Undo),
        (cmd(Key::N), Command::NewGame),
        (cmd(Key::R), Command::Restart),
        (cmd(Key::G), Command::Games),
        (cmd(Key::Q), Command::Quit),
    ]);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_shortcuts_come_before_their_plain_forms() {
        for mac in [true, false] {
            let list = shortcuts(mac);
            for (i, (a, _)) in list.iter().enumerate() {
                if a.modifiers.shift {
                    assert!(
                        !list[..i]
                            .iter()
                            .any(|(b, _)| b.logical_key == a.logical_key),
                        "{a:?} must come first"
                    );
                }
            }
        }
    }

    #[test]
    fn every_shortcut_matches_its_tooltip() {
        for mac in [true, false] {
            for (shortcut, command) in shortcuts(mac) {
                let text = command.shortcut_text(mac).expect("documented");
                let key = shortcut.logical_key.name();
                assert!(
                    text.ends_with(key) || command == Command::Redo,
                    "{command:?}: {text} vs {key}"
                );
            }
        }
        assert!(!shortcuts(true).iter().any(|(s, _)| s.logical_key == Key::Y));
    }
}
