//! The puzzles of the collection.
//!
//! Each puzzle is a module implementing the `Game` trait from
//! `puzzles-core`, behind a cargo feature of the same name (all enabled by
//! default). The crate also exports the registry of all available games.
//! See `docs/porting-plan.md` (decision D3).
//!
//! `nullgame`, C's template for new games, sits behind a feature of its
//! own that is off by default; it is never in the registry.

#[cfg(feature = "flip")]
pub mod flip;
#[cfg(feature = "nullgame")]
pub mod nullgame;
