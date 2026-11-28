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
    let cmd_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
    let mut list = vec![
        (cmd_shift(Key::Z), Command::Redo),
        (cmd_shift(Key::F), Command::Immersive),
    ];
    if mac {
        // macOS's own full-screen shortcut.
        let ctrl_cmd = Modifiers::COMMAND | Modifiers::CTRL;
        list.push((KeyboardShortcut::new(ctrl_cmd, Key::F), Command::FullScreen));
    } else {
        list.push((cmd(Key::Y), Command::Redo));
        list.push((
            KeyboardShortcut::new(Modifiers::NONE, Key::F11),
            Command::FullScreen,
        ));
    }
    list.extend([
        (cmd(Key::Z), Command::Undo),
        (cmd(Key::N), Command::NewGame),
        (cmd(Key::R), Command::Restart),
        (cmd(Key::G), Command::Games),
        (cmd(Key::S), Command::Save),
        (cmd(Key::O), Command::Load),
        // Cmd/Ctrl and the key with + on it, shifted or not.
        (cmd(Key::Equals), Command::ZoomIn),
        (cmd(Key::Plus), Command::ZoomIn),
        (cmd(Key::Minus), Command::ZoomOut),
        (cmd(Key::Num0), Command::ZoomFit),
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
                // Either minus sign: macOS writes "−", others "-".
                let text = text.replace('−', "-");
                let key = shortcut.logical_key.symbol_or_name().replace('−', "-");
                assert!(
                    text.ends_with(&key) || matches!(command, Command::Redo | Command::ZoomIn),
                    "{command:?}: {text} vs {key}"
                );
            }
        }
        assert!(!shortcuts(true).iter().any(|(s, _)| s.logical_key == Key::Y));
    }

    /// The command a key press runs, matched as the app does (in order,
    /// through egui's `consume_shortcut`).
    fn command_for(mac: bool, modifiers: Modifiers, key: Key) -> Option<Command> {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        ctx.begin_pass(raw);
        let found = ctx.input_mut(|i| {
            shortcuts(mac)
                .into_iter()
                .find(|(shortcut, _)| i.consume_shortcut(shortcut))
                .map(|(_, command)| command)
        });
        ctx.end_pass().textures_delta.clear();
        found
    }

    #[test]
    fn full_screen_and_immersive_keys_are_told_apart() {
        let mac_cmd = Modifiers::MAC_CMD | Modifiers::COMMAND;
        let mac = |extra| command_for(true, mac_cmd | extra, Key::F);
        assert_eq!(mac(Modifiers::CTRL), Some(Command::FullScreen));
        assert_eq!(mac(Modifiers::SHIFT), Some(Command::Immersive));
        assert_eq!(mac(Modifiers::NONE), None);
        assert_eq!(command_for(true, Modifiers::NONE, Key::F11), None);

        let ctrl = Modifiers::CTRL | Modifiers::COMMAND;
        assert_eq!(
            command_for(false, Modifiers::NONE, Key::F11),
            Some(Command::FullScreen)
        );
        assert_eq!(
            command_for(false, ctrl | Modifiers::SHIFT, Key::F),
            Some(Command::Immersive)
        );
    }

    #[test]
    fn zoom_keys_work_with_and_without_shift() {
        for (mac, cmd) in [
            (true, Modifiers::MAC_CMD | Modifiers::COMMAND),
            (false, Modifiers::CTRL | Modifiers::COMMAND),
        ] {
            let run = |mods, key| command_for(mac, mods, key);
            assert_eq!(run(cmd, Key::Equals), Some(Command::ZoomIn));
            assert_eq!(
                run(cmd | Modifiers::SHIFT, Key::Plus),
                Some(Command::ZoomIn)
            );
            assert_eq!(run(cmd, Key::Minus), Some(Command::ZoomOut));
            assert_eq!(run(cmd, Key::Num0), Some(Command::ZoomFit));
            assert_eq!(run(Modifiers::NONE, Key::Minus), None, "a game's key");
        }
    }
}
