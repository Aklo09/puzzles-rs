//! Platform-independent engine of the puzzle collection.
//!
//! This crate will hold the shared algorithms ported from the C project
//! (random number generator, union-find, Latin squares, grids, ...), the
//! `Game` trait every puzzle implements, the drawing
//! interface, colour palette specs, and the mid-end (undo/redo, animation,
//! game IDs, save files).
//!
//! Compatibility with the C version is a hard requirement for generation,
//! solving, moves and save files; see `docs/porting-plan.md` (decision D1).

pub mod button;
pub mod config;
pub mod divvy;
pub mod drawing;
pub mod dsf;
pub mod findloop;
pub mod game;
pub mod latin;
pub mod laydomino;
pub mod matching;
pub mod misc;
pub mod palette;
pub mod preset;
pub mod random;
pub mod solver_log;
pub mod sort;
pub mod sorted_set;
pub mod todo_queue;
