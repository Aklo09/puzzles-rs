//! Bindings to `divvy.c`: random division of a rectangle into polyominoes.

use std::ffi::c_int;
use std::ptr::NonNull;

use super::CRandom;
use super::dsf::{CDsf, DsfC};
use super::random::RandomStateC;

unsafe extern "C" {
    fn divvy_rectangle_attempt(w: c_int, h: c_int, k: c_int, rs: *mut RandomStateC) -> *mut DsfC;
}

/// C's `divvy_rectangle_attempt(w, h, k, rng)`: one set per polyomino, or
/// `None` if the attempt failed.
///
/// # Panics
/// If the rectangle is empty (C asserts a non-empty dsf), `k` is 0 or
/// doesn't divide `w * h` (C asserts or divides by zero), or `w * h`
/// exceeds `c_int`.
#[must_use]
pub fn c_divvy_rectangle_attempt(w: usize, h: usize, k: usize, rng: &mut CRandom) -> Option<CDsf> {
    assert!(w > 0 && h > 0, "C divvy needs a non-empty rectangle");
    let wh = w.checked_mul(h).expect("grid size overflows");
    assert!(k > 0 && wh.is_multiple_of(k), "k must divide w * h");
    // C allocates 4 * w * h ints for its addable table.
    assert!(
        c_int::try_from(4 * wh).is_ok(),
        "C's table size overflows c_int"
    );
    let (cw, ch, ck) = (to_c(w), to_c(h), to_c(k));
    // SAFETY: the sizes are positive, `k` divides `w * h` and `4 * w * h`
    // fits in a `c_int`, so C's asserts hold and its size arithmetic
    // doesn't overflow; the rng state is live and exclusively borrowed.
    // Returns a fresh dsf of `w * h` elements, or NULL on failure.
    let dsf = unsafe { divvy_rectangle_attempt(cw, ch, ck, rng.as_mut_ptr()) };
    // SAFETY: a non-NULL result is a plain dsf of `wh` elements that the
    // caller (now `CDsf`) owns.
    NonNull::new(dsf).map(|dsf| unsafe { CDsf::from_raw_plain(dsf, wh) })
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("exceeds c_int::MAX")
}
