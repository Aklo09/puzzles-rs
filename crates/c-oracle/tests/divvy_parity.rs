//! Differential tests: the Rust `divvy` against C's `divvy.c`.
//!
//! For each attempt we compare success or failure, every square's
//! representative (so the sets *and* their canonical elements match, which
//! Solo's block numbering depends on), and the generator state afterwards.

#![cfg(feature = "oracle")]

use c_oracle::{CRandom, c_divvy_rectangle_attempt};
use proptest::prelude::*;
use puzzles_core::divvy::{divvy_rectangle, try_divvy_rectangle};
use puzzles_core::dsf::DisjointSets;
use puzzles_core::random::RandomState;

/// Runs `attempts` attempts at `w x h` into `k`-ominoes from one generator
/// in both implementations, comparing each. Returns how many failed.
fn compare_attempts(
    seed: &[u8],
    (w, h, k): (usize, usize, usize),
    attempts: usize,
) -> Result<usize, TestCaseError> {
    let mut rust_rng = RandomState::new(seed);
    let mut c_rng = CRandom::new(seed);
    let mut failures = 0;
    for attempt in 0..attempts {
        let rust = try_divvy_rectangle(w, h, k, &mut rust_rng);
        let c = c_divvy_rectangle_attempt(w, h, k, &mut c_rng);
        match (rust, c) {
            (None, None) => failures += 1,
            (Some(mut rust), Some(mut c)) => {
                for i in 0..w * h {
                    prop_assert_eq!(
                        rust.find(i),
                        c.canonify(i),
                        "{}x{}/{} attempt {} square {}",
                        w,
                        h,
                        k,
                        attempt,
                        i
                    );
                }
            }
            (rust, c) => prop_assert!(
                false,
                "{}x{}/{} attempt {}: Rust {}, C {}",
                w,
                h,
                k,
                attempt,
                if rust.is_some() {
                    "succeeded"
                } else {
                    "failed"
                },
                if c.is_some() { "succeeded" } else { "failed" }
            ),
        }
        prop_assert_eq!(
            rust_rng.encode(),
            c_rng.encode(),
            "{}x{}/{} attempt {}",
            w,
            h,
            k,
            attempt
        );
    }
    Ok(failures)
}

/// Every `(w, h, k)` with `k` dividing `w * h`, for `w, h` in `1..=max`.
fn shapes(max: usize) -> Vec<(usize, usize, usize)> {
    let mut shapes = Vec::new();
    for w in 1..=max {
        for h in 1..=max {
            shapes.extend(
                (1..=w * h)
                    .filter(|&k| (w * h).is_multiple_of(k))
                    .map(|k| (w, h, k)),
            );
        }
    }
    shapes
}

/// A random rectangle up to 12 x 12 with a random divisor of its area.
fn shape() -> impl Strategy<Value = (usize, usize, usize)> {
    (1..=12_usize, 1..=12_usize).prop_flat_map(|(w, h)| {
        let divisors: Vec<_> = (1..=w * h).filter(|&k| (w * h).is_multiple_of(k)).collect();
        proptest::sample::select(divisors).prop_map(move |k| (w, h, k))
    })
}

proptest! {
    #[test]
    fn random_shapes_match_c(
        shape in shape(),
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        compare_attempts(&seed, shape, 3)?;
    }

    /// The retrying `divvy_rectangle` against C's loop over attempts.
    #[test]
    fn retrying_matches_c(
        shape in shape(),
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        let (w, h, k) = shape;
        let mut rust_rng = RandomState::new(&seed);
        let mut c_rng = CRandom::new(&seed);
        let mut rust = divvy_rectangle(w, h, k, &mut rust_rng);
        let mut c = loop {
            if let Some(c) = c_divvy_rectangle_attempt(w, h, k, &mut c_rng) {
                break c;
            }
        };
        for i in 0..w * h {
            prop_assert_eq!(rust.find(i), c.canonify(i), "square {}", i);
        }
        prop_assert_eq!(rust_rng.encode(), c_rng.encode());
    }
}

/// Every rectangle up to 8 x 8 with every divisor of its area.
#[test]
fn all_small_shapes_match_c() {
    for seed in [&b"a"[..], b"bb", b"ccc"] {
        for shape in shapes(8) {
            compare_attempts(seed, shape, 2).expect("parity");
        }
    }
}

/// The sizes the games use: Solo's jigsaw blocks (cr x cr into
/// cr-ominoes) and Palisade's presets and larger custom grids.
#[test]
fn game_shapes_match_c() {
    let shapes = [
        (4, 4, 4),
        (6, 6, 6),
        (9, 9, 9),
        (12, 12, 12),
        (16, 16, 16),
        (5, 5, 5),
        (8, 6, 6),
        (10, 8, 8),
        (15, 12, 10),
        (20, 20, 25),
    ];
    for shape in shapes {
        compare_attempts(b"games", shape, 10).expect("parity");
    }
}

/// Many attempts at shapes where attempts do fail, so the failure path
/// (and the generator state it leaves) is compared too. Upstream's
/// `divvy-test` uses 9 x 4 into 6-ominoes for this.
#[test]
fn failing_attempts_match_c() {
    let mut failures = 0;
    for shape in [(9, 4, 6), (12, 12, 12), (8, 8, 16), (10, 10, 25)] {
        failures += compare_attempts(b"123456", shape, 200).expect("parity");
    }
    assert!(
        failures > 0,
        "no attempt failed, so the failure path went untested"
    );
}
