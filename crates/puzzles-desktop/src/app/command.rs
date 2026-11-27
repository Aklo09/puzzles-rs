//! What the toolbar and shortcuts do, and how each shortcut is written
//! on each OS (Cmd on macOS, Ctrl elsewhere).

/// An app command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Back to the game list.
    Games,
    /// A new game of the same kind.
    NewGame,
    /// Back to the start of this game.
    Restart,
    /// Undo a move.
    Undo,
    /// Redo a move.
    Redo,
    /// Show the solution.
    Solve,
    /// Save the game to a file.
    Save,
    /// Open a saved game.
    Load,
    /// Close the app.
    Quit,
}

impl Command {
    /// The commands on the toolbar, in order; `None` is a separator.
    pub const TOOLBAR: [Option<Self>; 8] = [
        Some(Self::Games),
        None,
        Some(Self::NewGame),
        Some(Self::Restart),
        None,
        Some(Self::Undo),
        Some(Self::Redo),
        Some(Self::Solve),
    ];

    /// What the command does, for tooltips.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Games => "Games",
            Self::NewGame => "New game",
            Self::Restart => "Restart",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Solve => "Solve",
            Self::Save => "Save…",
            Self::Load => "Load…",
            Self::Quit => "Quit",
        }
    }

    /// The shortcut as the OS writes it, if the command has one.
    #[must_use]
    pub fn shortcut_text(self, mac: bool) -> Option<&'static str> {
        Some(match (self, mac) {
            (Self::Games, true) => "⌘G",
            (Self::Games, false) => "Ctrl+G",
            (Self::NewGame, true) => "⌘N",
            (Self::NewGame, false) => "Ctrl+N",
            (Self::Restart, true) => "⌘R",
            (Self::Restart, false) => "Ctrl+R",
            (Self::Undo, true) => "⌘Z",
            (Self::Undo, false) => "Ctrl+Z",
            (Self::Redo, true) => "⇧⌘Z",
            (Self::Redo, false) => "Ctrl+Y",
            (Self::Save, true) => "⌘S",
            (Self::Save, false) => "Ctrl+S",
            (Self::Load, true) => "⌘O",
            (Self::Load, false) => "Ctrl+O",
            (Self::Quit, true) => "⌘Q",
            (Self::Quit, false) => "Ctrl+Q",
            (Self::Solve, _) => return None,
        })
    }

    /// The tooltip: the label, and the shortcut if any.
    #[must_use]
    pub fn tooltip(self, mac: bool) -> String {
        match self.shortcut_text(mac) {
            Some(keys) => format!("{} ({keys})", self.label()),
            None => self.label().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltips_name_the_os_shortcut() {
        assert_eq!(Command::Undo.tooltip(true), "Undo (⌘Z)");
        assert_eq!(Command::Undo.tooltip(false), "Undo (Ctrl+Z)");
        assert_eq!(Command::Redo.tooltip(true), "Redo (⇧⌘Z)");
        assert_eq!(Command::Solve.tooltip(false), "Solve");
    }

    #[test]
    fn the_toolbar_starts_with_the_game_list() {
        assert_eq!(Command::TOOLBAR[0], Some(Command::Games));
        assert!(!Command::TOOLBAR.contains(&Some(Command::Quit)));
    }
}
