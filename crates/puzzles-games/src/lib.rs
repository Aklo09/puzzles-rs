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
#[cfg(feature = "mosaic")]
pub mod mosaic;
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
    /// A few words saying what kind of puzzle it is (upstream's
    /// `DESCRIPTION` in `CMakeLists.txt`), such as "Sliding block puzzle".
    pub description: &'static str,
    /// What the player has to do, in one sentence (upstream's
    /// `OBJECTIVE`).
    pub objective: &'static str,
    new: fn(&[u8], bool, MidendOptions) -> Box<dyn Puzzle>,
}

impl GameInfo {
    const fn of<G: Game>(description: &'static str, objective: &'static str) -> Self {
        Self {
            name: G::NAME,
            description,
            objective,
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
    GameInfo::of::<fifteen::Fifteen>(
        "Sliding block puzzle",
        "Slide the tiles around to arrange them into order.",
    ),
    #[cfg(feature = "flip")]
    GameInfo::of::<flip::Flip>(
        "Tile inversion puzzle",
        "Flip groups of squares to light them all up at once.",
    ),
    #[cfg(feature = "guess")]
    GameInfo::of::<guess::Guess>(
        "Combination-guessing puzzle",
        "Guess the hidden combination of colours.",
    ),
    #[cfg(feature = "mosaic")]
    GameInfo::of::<mosaic::Mosaic>(
        "Grid-filling puzzle",
        "Fill in the grid given clues about number of nearby black squares.",
    ),
];

/// The game with this name (as in save files), if it is built in.
#[must_use]
pub fn find(name: &str) -> Option<&'static GameInfo> {
    GAMES.iter().find(|g| g.name == name)
}
