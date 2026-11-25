//! Types games and the mid-end exchange (from `puzzles.h`, upstream
//! 616da16). The `Game` trait itself joins them with the first game port.

use std::borrow::Cow;
use std::fmt;

use crate::button::Button;

/// What `interpret_move` made of an input event (C's move string, or one
/// of `MOVE_UNUSED`, `MOVE_NO_EFFECT`, `MOVE_UI_UPDATE`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveResult<M> {
    /// The game has no use for this input (C's `MOVE_UNUSED`); the front
    /// end may handle it.
    #[doc(alias = "MOVE_UNUSED")]
    Unused,
    /// Understood, but it changed nothing, not even the UI (C's
    /// `MOVE_NO_EFFECT`).
    #[doc(alias = "MOVE_NO_EFFECT")]
    NoEffect,
    /// Only the UI changed (cursor, highlight); nothing goes on the undo
    /// chain (C's `MOVE_UI_UPDATE`).
    #[doc(alias = "MOVE_UI_UPDATE")]
    UiUpdate,
    /// A move to execute and record.
    Move(M),
}

/// A user-facing error from a game: invalid parameters, description,
/// move or solve request. The messages are C's, word for word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameError(pub Cow<'static, str>);

impl GameError {
    /// An error with a fixed message, as most C messages are.
    #[must_use]
    pub const fn new(message: &'static str) -> Self {
        Self(Cow::Borrowed(message))
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GameError {}

impl From<String> for GameError {
    fn from(message: String) -> Self {
        Self(Cow::Owned(message))
    }
}

/// Whether a game is over (C's `status`: 0, +1, -1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameStatus {
    /// Still being played (0).
    Ongoing,
    /// Solved (+1).
    Won,
    /// Lost, e.g. a mine was hit (-1).
    Lost,
}

/// A key a game asks a keyboardless front end to offer as a button (C's
/// `key_label`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyLabel {
    /// What to show; `None` asks for the generic label
    /// ([`Button::label`]).
    pub label: Option<Cow<'static, str>>,
    /// What pressing it sends.
    pub button: Button,
}
