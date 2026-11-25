//! Bindings to `laydomino.c`: random domino tilings.

use std::ffi::{c_int, c_void};
use std::slice;

use super::random::RandomStateC;
use super::{CRandom, sfree};

unsafe extern "C" {
    fn domino_layout(w: c_int, h: c_int, rs: *mut RandomStateC) -> *mut c_int;
}

/// C's `domino_layout(w, h, rng)`: for each square, the other end of its
/// domino (itself for the one square an odd area leaves over).
///
/// # Panics
/// If `w` or `h` is 0 (C asserts), or `w * h` exceeds `c_int`.
#[must_use]
pub fn c_domino_layout(w: usize, h: usize, rng: &mut CRandom) -> Vec<usize> {
    assert!(w > 0 && h > 0, "C domino_layout needs a non-empty grid");
    let wh = w.checked_mul(h).expect("grid size overflows");
    let (cw, ch) = (to_c(w), to_c(h));
    assert!(
        c_int::try_from(2 * wh).is_ok(),
        "C's placement list size overflows c_int"
    );
    // SAFETY: both sizes are positive and `2 * w * h` fits in a `c_int`, so
    // C's size arithmetic doesn't overflow; the rng state is live and
    // exclusively borrowed. Returns a fresh `w * h` array (smalloc aborts
    // instead of returning NULL).
    let grid = unsafe { domino_layout(cw, ch, rng.as_mut_ptr()) };
    assert!(!grid.is_null(), "domino_layout returned NULL");
    // SAFETY: `grid` points at `wh` initialised `c_int`s, freed only below.
    let layout = unsafe { slice::from_raw_parts(grid, wh) }
        .iter()
        .map(|&j| usize::try_from(j).expect("C returned a negative square"))
        .collect();
    // SAFETY: `grid` came from C's allocator, is no longer borrowed, and is
    // freed exactly once.
    unsafe { sfree(grid.cast::<c_void>()) };
    layout
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("exceeds c_int::MAX")
}
