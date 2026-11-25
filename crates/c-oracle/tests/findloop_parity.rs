//! Differential tests: the Rust `LoopFinder` against C's `findloop.c`.
//!
//! For each graph we compare the loop flag, `is_loop_edge` for every pair
//! of vertices (edges and non-edges, which C answers `true` for), and
//! `is_bridge` with both side sizes for every pair.

#![cfg(feature = "oracle")]

use std::collections::BTreeSet;

use c_oracle::CFindLoop;
use proptest::prelude::*;
use puzzles_core::findloop::LoopFinder;

/// Symmetric adjacency lists from an edge list: duplicates dropped,
/// self-loops listed once, neighbours in edge order.
fn adjacency(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut seen = BTreeSet::new();
    let mut adjacency = vec![Vec::new(); n];
    for &(a, b) in edges {
        let (a, b) = (a % n, b % n);
        if seen.insert((a.min(b), a.max(b))) {
            adjacency[a].push(b);
            if a != b {
                adjacency[b].push(a);
            }
        }
    }
    adjacency
}

fn compare(adjacency: &[Vec<usize>]) -> Result<(), TestCaseError> {
    let n = adjacency.len();
    let mut rust = LoopFinder::new();
    let rust_loop = rust.run(n, |v| adjacency[v].iter().copied());
    let c = CFindLoop::run(adjacency);
    prop_assert_eq!(rust_loop, c.has_loop());
    for u in 0..n {
        for v in 0..n {
            prop_assert_eq!(
                rust.is_loop_edge(u, v),
                c.is_loop_edge(u, v),
                "loop edge {}-{}",
                u,
                v
            );
            prop_assert_eq!(
                rust.is_bridge(u, v),
                c.is_bridge(u, v),
                "bridge {}-{}",
                u,
                v
            );
        }
    }
    Ok(())
}

proptest! {
    /// Arbitrary small graphs, including self-loops and several components.
    #[test]
    fn random_graphs_match_c(
        n in 1..=24_usize,
        edges in proptest::collection::vec((0..24_usize, 0..24_usize), 0..60),
    ) {
        compare(&adjacency(n, &edges))?;
    }

    /// Game-shaped graphs: a random subset of the edges of a w x h grid, as
    /// in Net, Slant, Tracks and Bridges.
    #[test]
    fn grid_subgraphs_match_c(
        w in 1..=8_usize,
        h in 1..=8_usize,
        keep in proptest::collection::vec(any::<bool>(), 2 * 64),
    ) {
        let mut edges = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = y * w + x;
                if x + 1 < w && keep[2 * v] {
                    edges.push((v, v + 1));
                }
                if y + 1 < h && keep[2 * v + 1] {
                    edges.push((v, v + w));
                }
            }
        }
        compare(&adjacency(w * h, &edges))?;
    }
}

/// A long path closed into a cycle at one end: deep DFS trees and long
/// chains of bridges with large side sizes.
#[test]
fn long_lollipop_matches_c() {
    let n = 500;
    let mut edges: Vec<(usize, usize)> = (1..n).map(|v| (v - 1, v)).collect();
    edges.extend([(0, 5), (n - 1, n - 1)]);
    compare(&adjacency(n, &edges)).expect("parity");
}
