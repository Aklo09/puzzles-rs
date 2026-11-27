//! The puzzles of the collection.
//!
//! Each puzzle is a module implementing the `Game` trait from
//! `puzzles-core`, behind a cargo feature of the same name (all enabled by
//! default). The crate also exports the registry of all available games.
//! See `docs/porting-plan.md` (decision D3).
//!
//! `nullgame`, C's template for new games, sits behind a feature of its
//! own that is off by default; it is never in the registry.

#[cfg(feature = "fifteen")]
pub mod fifteen;
#[cfg(feature = "flip")]
pub mod flip;
#[cfg(feature = "guess")]
pub mod guess;
#[cfg(feature = "nullgame")]
pub mod nullgame;

use puzzles_core::erased::Puzzle;
use puzzles_core::game::Game;
use puzzles_core::midend::{Midend, MidendOptions};

/// A game in the registry.
#[derive(Clone, Copy, Debug)]
pub struct GameInfo {
    /// The game's name (its `Game::NAME`), as in save files.
    pub name: &'static str,
    new: fn(&[u8], bool, MidendOptions) -> Box<dyn Puzzle>,
}

impl GameInfo {
    const fn of<G: Game>() -> Self {
        Self {
            name: G::NAME,
            new: |seed, interactive, options| {
                Box::new(Midend::<G>::new(seed, interactive, options))
            },
        }
    }

    /// A mid-end for this game; see [`Midend::new`].
    #[must_use]
    pub fn new_midend(
        &self,
        seed: &[u8],
        interactive: bool,
        options: MidendOptions,
    ) -> Box<dyn Puzzle> {
        (self.new)(seed, interactive, options)
    }
}

/// Every game built in, in C's order (alphabetical).
pub const GAMES: &[GameInfo] = &[
    #[cfg(feature = "fifteen")]
    GameInfo::of::<fifteen::Fifteen>(),
    #[cfg(feature = "flip")]
    GameInfo::of::<flip::Flip>(),
    #[cfg(feature = "guess")]
    GameInfo::of::<guess::Guess>(),
];

/// The game with this name (as in save files), if it is built in.
#[must_use]
pub fn find(name: &str) -> Option<&'static GameInfo> {
    GAMES.iter().find(|g| g.name == name)
}
