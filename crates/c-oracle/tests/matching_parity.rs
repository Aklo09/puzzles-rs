//! Differential tests: the Rust `Matcher` against C's `matching.c`.
//!
//! For each graph we compare the matching count, both partner arrays, the
//! maximality witness and, for randomised runs, the generator state
//! afterwards (so the draws were the same, in the same order). C's one-off
//! `matching()` is checked against the same answers.

#![cfg(feature = "oracle")]

use c_oracle::{CMatching, CRandom, c_matching, c_matching_with_scratch};
use proptest::prelude::*;
use puzzles_core::matching::Matcher;
use puzzles_core::random::RandomState;

/// Adjacency lists for `nl` left vertices from `(l, r)` pairs, in pair
/// order, reduced modulo the side sizes. Repeats are kept: C allows them.
fn from_pairs(nl: usize, nr: usize, pairs: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); nl];
    if nr > 0 {
        for &(l, r) in pairs {
            if nl > 0 {
                adjacency[l % nl].push(r % nr);
            }
        }
    }
    adjacency
}

/// Runs both implementations on one graph, with a generator seeded by
/// `seed` if given, and compares everything observable.
fn compare_with(
    matcher: &mut Matcher,
    nr: usize,
    adjacency: &[Vec<usize>],
    seed: Option<&[u8]>,
) -> Result<CMatching, TestCaseError> {
    let nl = adjacency.len();
    let mut rust_rng = seed.map(RandomState::new);
    let mut c_rng = seed.map(CRandom::new);
    let count = matcher.run(nl, nr, |l| adjacency[l].iter().copied(), rust_rng.as_mut());
    let c = c_matching_with_scratch(nr, adjacency, c_rng.as_mut());

    prop_assert_eq!(count, c.count);
    prop_assert_eq!(matcher.left_to_right(), &c.left_to_right[..]);
    prop_assert_eq!(matcher.right_to_left(), &c.right_to_left[..]);
    prop_assert_eq!(matcher.witness(), c.witness.clone());
    prop_assert_eq!(
        rust_rng.as_ref().map(RandomState::encode),
        c_rng.as_ref().map(CRandom::encode)
    );

    // The one-off entry point gives the same answer from the same state.
    let mut c_rng = seed.map(CRandom::new);
    let (one_off_count, outl, outr) = c_matching(nr, adjacency, c_rng.as_mut());
    prop_assert_eq!(one_off_count, c.count);
    prop_assert_eq!(&outl, &c.left_to_right);
    prop_assert_eq!(&outr, &c.right_to_left);
    Ok(c)
}

fn compare(nr: usize, adjacency: &[Vec<usize>], seed: Option<&[u8]>) -> Result<(), TestCaseError> {
    compare_with(&mut Matcher::new(), nr, adjacency, seed).map(|_| ())
}

fn seed() -> impl Strategy<Value = Option<Vec<u8>>> {
    proptest::option::weighted(0.8, proptest::collection::vec(any::<u8>(), 0..8))
}

proptest! {
    /// Sparse graphs from arbitrary edge lists: any order, repeated edges,
    /// isolated vertices, empty sides.
    #[test]
    fn random_edge_lists_match_c(
        nl in 0..=12_usize,
        nr in 0..=12_usize,
        pairs in proptest::collection::vec((0..12_usize, 0..12_usize), 0..60),
        seed in seed(),
    ) {
        compare(nr, &from_pairs(nl, nr, &pairs), seed.as_deref())?;
    }

    /// Graphs of every density from empty to complete, lists in increasing
    /// order (as Latin and Tents build them).
    #[test]
    fn graphs_of_any_density_match_c(
        nl in 0..=16_usize,
        nr in 0..=16_usize,
        density in 0..=100_u8,
        cells in proptest::collection::vec(0..100_u8, 256),
        seed in seed(),
    ) {
        let adjacency: Vec<Vec<usize>> = (0..nl)
            .map(|l| (0..nr).filter(|&r| cells[l * 16 + r] < density).collect())
            .collect();
        compare(nr, &adjacency, seed.as_deref())?;
    }

    /// One matcher reused over a sequence of graphs of different shapes,
    /// each compared with a fresh C run.
    #[test]
    fn reused_matcher_matches_c(
        graphs in proptest::collection::vec(
            (0..=10_usize, 0..=10_usize,
             proptest::collection::vec((0..10_usize, 0..10_usize), 0..40)),
            1..6,
        ),
        seed in seed(),
    ) {
        let mut matcher = Matcher::new();
        for (nl, nr, pairs) in &graphs {
            compare_with(&mut matcher, *nr, &from_pairs(*nl, *nr, pairs), seed.as_deref())?;
        }
    }

    /// Latin-square generation as `latin.c` does it: each row is a perfect
    /// matching of columns to the digits not yet used in them, all drawn
    /// from one generator. Every row and the final generator state agree.
    #[test]
    fn latin_square_rows_match_c(
        order in 1..=10_usize,
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        let mut rust_rng = RandomState::new(&seed);
        let mut c_rng = CRandom::new(&seed);
        let mut matcher = Matcher::new();
        let mut square: Vec<Vec<usize>> = Vec::new();
        for _ in 0..order {
            let adjacency: Vec<Vec<usize>> = (0..order)
                .map(|col| (0..order).filter(|&d| square.iter().all(|row| row[col] != d)).collect())
                .collect();
            let count = matcher.run(order, order, |l| adjacency[l].iter().copied(), Some(&mut rust_rng));
            let c = c_matching_with_scratch(order, &adjacency, Some(&mut c_rng));
            prop_assert_eq!(count, order);
            prop_assert_eq!(c.count, order);
            prop_assert_eq!(matcher.left_to_right(), &c.left_to_right[..]);
            square.push(c.left_to_right.iter().map(|d| d.expect("perfect")).collect());
        }
        prop_assert_eq!(rust_rng.encode(), c_rng.encode());
    }
}

/// `auxiliary/matching.c`'s autotest graph at full size (256 + 256
/// vertices): every subset of an 8-element set joined to the subsets one
/// element smaller. Repeated randomised runs on one generator, so many
/// passes and many draws per run.
#[test]
fn upstream_subset_graph_matches_c() {
    let b = 8;
    let n = 1_usize << b;
    let adjacency: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..b)
                .filter(|j| i & (1 << j) != 0)
                .map(|j| i & !(1 << j))
                .collect()
        })
        .collect();
    let seed = b"fixed random seed for repeatability";
    let mut rust_rng = RandomState::new(seed);
    let mut c_rng = CRandom::new(seed);
    let mut matcher = Matcher::new();
    for run in 0..50 {
        let count = matcher.run(n, n, |l| adjacency[l].iter().copied(), Some(&mut rust_rng));
        let c = c_matching_with_scratch(n, &adjacency, Some(&mut c_rng));
        assert_eq!(count, c.count, "run {run}");
        assert_eq!(matcher.left_to_right(), c.left_to_right, "run {run}");
        assert_eq!(matcher.witness(), c.witness, "run {run}");
        assert_eq!(rust_rng.encode(), c_rng.encode(), "run {run}");
    }
}

/// A long augmenting chain: the deterministic search needs a path through
/// every vertex, the deepest the search can go (C's off-by-one write past
/// its `dfsstate` array happens here).
#[test]
fn long_chain_matches_c() {
    let n = 200;
    let adjacency: Vec<Vec<usize>> = (0..n)
        .map(|l| if l + 1 < n { vec![l + 1, l] } else { vec![l] })
        .collect();
    compare(n, &adjacency, None).expect("parity");
    compare(n, &adjacency, Some(b"chain")).expect("parity");
}
