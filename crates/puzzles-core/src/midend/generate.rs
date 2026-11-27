//! New Game in two halves, so a front end can generate puzzles on
//! another thread (threading decision in `docs/porting-plan.md`):
//! [`Midend::begin_new_game`](super::Midend::begin_new_game) returns a
//! [`NewGameJob`], which runs the game's generator anywhere, and
//! [`Midend::finish_new_game`](super::Midend::finish_new_game) installs
//! the result. `Midend::new_game` is the three in a row.
//!
//! Beginning draws the random seed, exactly as C's `midend_new_game`
//! does before generating, but changes nothing else, so a job that is
//! thrown away (the user cancelled) leaves the current game as it was.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::game::NewDesc;

/// A unique id for each mid-end, so a job can't finish in another one.
pub(super) fn next_owner_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The generator call, with everything it needs moved in.
type Generate = Box<dyn FnOnce() -> NewDesc + Send>;

/// The slow part of a New Game, ready to run on any thread.
pub struct NewGameJob {
    pub(super) owner: u64,
    pub(super) epoch: u64,
    /// The random seed drawn for this game, if it is a fresh one.
    pub(super) fresh_seed: Option<Vec<u8>>,
    /// `None` when there is nothing to generate (a descriptive game ID).
    pub(super) generate: Option<Generate>,
}

impl fmt::Debug for NewGameJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewGameJob")
            .field("owner", &self.owner)
            .field("epoch", &self.epoch)
            .field("generates", &self.generate.is_some())
            .finish_non_exhaustive()
    }
}

impl NewGameJob {
    /// True if running the job calls the generator. A descriptive game
    /// ID needs none, so a front end can finish it at once.
    #[must_use]
    pub fn generates(&self) -> bool {
        self.generate.is_some()
    }

    /// Runs the game's generator (C's `new_desc`), which may take long.
    #[must_use]
    pub fn run(self) -> GeneratedGame {
        GeneratedGame {
            owner: self.owner,
            epoch: self.epoch,
            fresh_seed: self.fresh_seed,
            desc: self.generate.map(|generate| generate()),
        }
    }
}

/// A finished [`NewGameJob`], to hand back to its mid-end.
#[derive(Clone, Debug)]
pub struct GeneratedGame {
    pub(super) owner: u64,
    pub(super) epoch: u64,
    pub(super) fresh_seed: Option<Vec<u8>>,
    pub(super) desc: Option<NewDesc>,
}
