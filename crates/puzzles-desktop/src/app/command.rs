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
    /// Zoom the puzzle in a step.
    ZoomIn,
    /// Zoom the puzzle out a step.
    ZoomOut,
    /// Fit the puzzle to the window.
    ZoomFit,
    /// Enter or leave full screen.
    FullScreen,
    /// Hide or show the toolbar and status bar.
    Immersive,
    /// Close the app.
    Quit,
}

impl Command {
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
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::ZoomFit => "Fit to window",
            Self::FullScreen => "Full screen",
            Self::Immersive => "Immersive mode",
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
            (Self::ZoomIn, true) => "⌘+",
            (Self::ZoomIn, false) => "Ctrl++",
            (Self::ZoomOut, true) => "⌘−",
            (Self::ZoomOut, false) => "Ctrl+-",
            (Self::ZoomFit, true) => "⌘0",
            (Self::ZoomFit, false) => "Ctrl+0",
            (Self::FullScreen, true) => "⌃⌘F",
            (Self::FullScreen, false) => "F11",
            (Self::Immersive, true) => "⇧⌘F",
            (Self::Immersive, false) => "Ctrl+Shift+F",
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
    fn full_screen_follows_each_os() {
        assert_eq!(Command::FullScreen.tooltip(true), "Full screen (⌃⌘F)");
        assert_eq!(Command::FullScreen.tooltip(false), "Full screen (F11)");
    }
}
