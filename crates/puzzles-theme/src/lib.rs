//! Themes for the puzzle collection.
//!
//! Puzzles only name their colours (roles, series, derived and
//! game-specific specs from `puzzles-core`); this crate turns those names
//! into RGB values using data-only TOML themes. The built-in "Classic"
//! theme reproduces the original C colours exactly: it is generated from
//! the C code by the oracle tests (`c-oracle/tests/theme_parity.rs`). See
//! `docs/porting-plan.md` (decision D7).

mod colour;
mod model;
mod resolve;

pub use colour::{Bevel, Rgb, bevel, mix};
pub use model::{GameColours, ROLES, SERIES, Theme, ThemeError, game_key, role_key, series_key};
pub use resolve::{ResolveError, ThemeSet};
