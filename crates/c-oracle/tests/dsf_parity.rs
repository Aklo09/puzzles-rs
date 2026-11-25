//! Differential tests: the Rust `Dsf`, `MinDsf` and `FlipDsf` against C's
//! `dsf.c`.
//!
//! Games use a set's canonical element as an index, so beyond "same
//! partition" we require the *same canonical element* for every query, plus
//! the same sizes, minima and flips, after any sequence of operations.

#![cfg(feature = "oracle")]

use c_oracle::{CDsf, CDsfKind};
use proptest::prelude::*;
use puzzles_core::dsf::{DisjointSets, Dsf, FlipDsf, MinDsf};

const MAX_LEN: usize = 40;

/// One operation; indices are reduced modulo the forest's length.
#[derive(Clone, Debug)]
enum Op {
    Union(usize, usize),
    Find(usize),
    Size(usize),
    Same(usize, usize),
    /// Only for flip forests: union with this flip relation.
    UnionFlip(usize, usize, bool),
    Reset,
}

fn op() -> impl Strategy<Value = Op> {
    let i = 0..MAX_LEN;
    prop_oneof![
        8 => (i.clone(), i.clone()).prop_map(|(a, b)| Op::Union(a, b)),
        4 => i.clone().prop_map(Op::Find),
        2 => i.clone().prop_map(Op::Size),
        2 => (i.clone(), i.clone()).prop_map(|(a, b)| Op::Same(a, b)),
        8 => (i.clone(), i, any::<bool>()).prop_map(|(a, b, f)| Op::UnionFlip(a, b, f)),
        1 => Just(Op::Reset),
    ]
}

/// Canonical element, size, and (min dsf only) minimum of every element.
fn snapshot_plain(
    rust: &mut impl DisjointSets,
    c: &mut CDsf,
) -> (Vec<[usize; 2]>, Vec<[usize; 2]>) {
    (0..rust.len())
        .map(|n| ([rust.find(n), rust.set_size(n)], [c.canonify(n), c.size(n)]))
        .unzip()
}

proptest! {
    #[test]
    fn plain_dsf_matches_c(len in 1..=MAX_LEN, ops in proptest::collection::vec(op(), 0..150)) {
        let mut rust = Dsf::new(len);
        let mut c = CDsf::new(CDsfKind::Plain, len);
        for op in &ops {
            match *op {
                Op::Union(a, b) | Op::UnionFlip(a, b, _) => {
                    rust.union(a % len, b % len);
                    c.merge(a % len, b % len);
                }
                Op::Find(n) => prop_assert_eq!(rust.find(n % len), c.canonify(n % len)),
                Op::Size(n) => prop_assert_eq!(rust.set_size(n % len), c.size(n % len)),
                Op::Same(a, b) => prop_assert_eq!(
                    rust.same_set(a % len, b % len),
                    c.equivalent(a % len, b % len)
                ),
                Op::Reset => {
                    rust.reset();
                    c.reinit();
                }
            }
        }
        let (rust_snap, c_snap) = snapshot_plain(&mut rust, &mut c);
        prop_assert_eq!(rust_snap, c_snap);
    }

    #[test]
    fn min_dsf_matches_c(len in 1..=MAX_LEN, ops in proptest::collection::vec(op(), 0..150)) {
        let mut rust = MinDsf::new(len);
        let mut c = CDsf::new(CDsfKind::Min, len);
        for op in &ops {
            match *op {
                Op::Union(a, b) | Op::UnionFlip(a, b, _) => {
                    rust.union(a % len, b % len);
                    c.merge(a % len, b % len);
                }
                Op::Find(n) | Op::Size(n) => {
                    prop_assert_eq!(rust.min_element(n % len), c.minimal(n % len));
                }
                Op::Same(a, b) => prop_assert_eq!(
                    rust.same_set(a % len, b % len),
                    c.equivalent(a % len, b % len)
                ),
                Op::Reset => {
                    rust.reset();
                    c.reinit();
                }
            }
        }
        let (rust_snap, c_snap) = snapshot_plain(&mut rust, &mut c);
        prop_assert_eq!(rust_snap, c_snap);
        let rust_min: Vec<usize> = (0..len).map(|n| rust.min_element(n)).collect();
        let c_min: Vec<usize> = (0..len).map(|n| c.minimal(n)).collect();
        prop_assert_eq!(rust_min, c_min);
    }

    #[test]
    fn flip_dsf_matches_c(len in 1..=MAX_LEN, ops in proptest::collection::vec(op(), 0..150)) {
        let mut rust = FlipDsf::new(len);
        let mut c = CDsf::new(CDsfKind::Flip, len);
        for op in &ops {
            match *op {
                Op::Union(a, b) | Op::UnionFlip(a, b, _) => {
                    let (a, b) = (a % len, b % len);
                    let requested = match *op { Op::UnionFlip(_, _, f) => f, _ => false };
                    // Query both sides first (this also compares them). If
                    // `a` and `b` are already related, ask for the existing
                    // relation: C aborts the process on a contradiction.
                    let (ra, fa) = rust.find_with_flip(a);
                    let (rb, fb) = rust.find_with_flip(b);
                    prop_assert_eq!((ra, fa), c.canonify_flip(a));
                    prop_assert_eq!((rb, fb), c.canonify_flip(b));
                    let flipped = if ra == rb { fa ^ fb } else { requested };
                    rust.union(a, b, flipped);
                    c.merge_flip(a, b, flipped);
                }
                Op::Find(n) | Op::Size(n) | Op::Same(n, _) => {
                    prop_assert_eq!(rust.find_with_flip(n % len), c.canonify_flip(n % len));
                }
                Op::Reset => {
                    rust.reset();
                    c.reinit();
                }
            }
        }
        let rust_all: Vec<(usize, bool)> = (0..len).map(|n| rust.find_with_flip(n)).collect();
        let c_all: Vec<(usize, bool)> = (0..len).map(|n| c.canonify_flip(n)).collect();
        prop_assert_eq!(rust_all, c_all);
    }

    /// `clone_from` (C's `dsf_copy`) yields a forest that keeps behaving
    /// like the C copy.
    #[test]
    fn copied_dsf_continues_like_c(
        len in 1..=MAX_LEN,
        before in proptest::collection::vec((0..MAX_LEN, 0..MAX_LEN), 0..40),
        after in proptest::collection::vec((0..MAX_LEN, 0..MAX_LEN), 0..40),
    ) {
        let (mut rust_src, mut c_src) = (Dsf::new(len), CDsf::new(CDsfKind::Plain, len));
        for &(a, b) in &before {
            rust_src.union(a % len, b % len);
            c_src.merge(a % len, b % len);
        }
        let (mut rust, mut c) = (Dsf::new(len), CDsf::new(CDsfKind::Plain, len));
        rust.clone_from(&rust_src);
        c.copy_from(&c_src);
        for &(a, b) in &after {
            rust.union(a % len, b % len);
            c.merge(a % len, b % len);
        }
        let (rust_snap, c_snap) = snapshot_plain(&mut rust, &mut c);
        prop_assert_eq!(rust_snap, c_snap);
    }
}

/// A long chain of unions builds deep trees before any compression, the
/// shape where root choice and flip bookkeeping are easiest to get wrong.
#[test]
fn long_flip_chain_matches_c() {
    let len = 2000;
    let mut rust = FlipDsf::new(len);
    let mut c = CDsf::new(CDsfKind::Flip, len);
    for n in 1..len {
        // Alternate union direction and flip to mix tie-breaks and sizes.
        let (a, b) = if n % 2 == 0 { (n - 1, n) } else { (n, n - 1) };
        rust.union(a, b, n % 3 == 0);
        c.merge_flip(a, b, n % 3 == 0);
    }
    for n in (0..len).rev() {
        assert_eq!(rust.find_with_flip(n), c.canonify_flip(n), "n = {n}");
    }
}
