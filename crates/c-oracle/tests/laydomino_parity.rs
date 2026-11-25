//! Differential tests: the Rust `domino_layout` against C's `laydomino.c`.
//!
//! For each grid we compare the layout and the generator state afterwards
//! (so the shuffles drew the same values, in the same order).

#![cfg(feature = "oracle")]

use c_oracle::{CRandom, c_domino_layout};
use proptest::prelude::*;
use puzzles_core::laydomino::domino_layout;
use puzzles_core::random::RandomState;

/// Lays out each `(w, h)` in turn from one generator in both
/// implementations, comparing every layout and the final state.
fn compare(seed: &[u8], sizes: &[(usize, usize)]) -> Result<(), TestCaseError> {
    let mut rust_rng = RandomState::new(seed);
    let mut c_rng = CRandom::new(seed);
    for &(w, h) in sizes {
        let rust = domino_layout(w, h, &mut rust_rng);
        let c = c_domino_layout(w, h, &mut c_rng);
        prop_assert_eq!(rust, c, "{}x{}", w, h);
        prop_assert_eq!(rust_rng.encode(), c_rng.encode(), "{}x{}", w, h);
    }
    Ok(())
}

proptest! {
    /// Any grid up to 24 x 24, including strips and odd areas.
    #[test]
    fn random_grids_match_c(
        w in 1..=24_usize,
        h in 1..=24_usize,
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        compare(&seed, &[(w, h)])?;
    }

    /// Several layouts drawn from one generator, as Dominosa's generator
    /// does when it retries.
    #[test]
    fn layout_sequences_match_c(
        sizes in proptest::collection::vec((1..=12_usize, 1..=12_usize), 1..8),
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        compare(&seed, &sizes)?;
    }
}

/// Every grid size up to 16 x 16, a few seeds each.
#[test]
fn all_small_grids_match_c() {
    for seed in [&b"a"[..], b"bb", b"ccc"] {
        let sizes: Vec<_> = (1..=16)
            .flat_map(|w| (1..=16).map(move |h| (w, h)))
            .collect();
        compare(seed, &sizes).expect("parity");
    }
}

/// Large grids: long fix-up searches through many dominoes.
#[test]
fn large_grids_match_c() {
    compare(b"large", &[(60, 45), (99, 1), (1, 99), (75, 75), (101, 63)]).expect("parity");
}
