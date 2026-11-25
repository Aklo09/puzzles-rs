//! Port of `third_party/puzzles-c/matching.c` (upstream 616da16).
//!
//! Maximum matching in a bipartite graph by the Hopcroft–Karp algorithm.
//! Latin-square generation (Keen, Towers, Solo's Latin squares, Unequal,
//! Group) uses it to fill each new row, and Tents to place trees next to
//! tents and to check that a grid still has a tent/tree pairing.
//!
//! When given a [`RandomState`], the search is randomised so that any
//! maximum matching can come out. Which one does decides puzzles, so this
//! is a faithful port (D5 exception): same iteration order and the same
//! random draws as C, checked against it by the oracle tests.

use crate::random::RandomState;

/// Maximum bipartite matcher (C's `struct scratch` plus
/// `matching_with_scratch`).
///
/// Reusable: [`run`](Self::run) matches a graph, then
/// [`left_to_right`](Self::left_to_right),
/// [`right_to_left`](Self::right_to_left) and [`witness`](Self::witness)
/// answer for that graph until the next run. Keeping one matcher across
/// runs saves reallocating its buffers, which is what C's separate scratch
/// space is for.
///
/// ```
/// use puzzles_core::matching::Matcher;
///
/// // Left 0 can only take right 1; left 1 can take right 0 or 1.
/// let adjacency: [&[usize]; 2] = [&[1], &[1, 0]];
/// let mut matcher = Matcher::new();
/// assert_eq!(matcher.run(2, 2, |l| adjacency[l].iter().copied(), None), 2);
/// assert_eq!(matcher.left_to_right(), [Some(1), Some(0)]);
/// assert_eq!(matcher.right_to_left(), [Some(1), Some(0)]);
/// ```
#[doc(alias = "scratch")]
#[derive(Clone, Debug, Default)]
pub struct Matcher {
    /// All adjacency lists, back to back (left vertex `l`'s list is
    /// `adjacency[starts[l]..starts[l + 1]]`). A randomised run permutes
    /// the lists in place as it goes, as C does to the caller's lists.
    adjacency: Vec<usize>,
    starts: Vec<usize>,
    /// The matching so far: partner of each left / right vertex.
    l_to_r: Vec<Option<usize>>,
    r_to_l: Vec<Option<usize>>,
    /// Layer of each vertex in the breadth-first search; `None` if not
    /// reached, or already used by the depth-first search.
    l_layer: Vec<Option<usize>>,
    r_layer: Vec<Option<usize>>,
    l_queue: Vec<usize>,
    r_queue: Vec<usize>,
    /// The augmenting path being built: left vertices at even positions,
    /// right vertices at odd ones.
    aug_path: Vec<usize>,
    /// Progress of the depth-first search at each left vertex of the path:
    /// the next index into `l_order` (depth 0) or into the previous left
    /// vertex's adjacency list.
    dfs_state: Vec<usize>,
    /// Order in which free left vertices start a search.
    l_order: Vec<usize>,
}

impl Matcher {
    /// Creates a matcher; buffers grow to fit the graphs it is run on.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Finds a maximum matching of the bipartite graph with left vertices
    /// `0..nl` and right vertices `0..nr`, where `neighbours(l)` lists the
    /// right vertices adjacent to `l`. Returns the number of matched pairs
    /// (C's `matching_with_scratch`, or `matching`).
    ///
    /// `neighbours` is called once for each left vertex, in order, before
    /// the search starts; a list may repeat a vertex. With `rng`, the
    /// search order is randomised so that any maximum matching is a
    /// possible result, drawing exactly as C does. Without it the result
    /// is deterministic.
    ///
    /// Unlike C, the caller's lists are never modified: the randomised
    /// search permutes the matcher's own copy.
    ///
    /// # Panics
    /// If `neighbours` returns a vertex `>= nr`, or a list has `2^28`
    /// entries or more (see [`RandomState::upto`]).
    #[doc(alias = "matching_with_scratch")]
    #[doc(alias = "matching")]
    pub fn run<I>(
        &mut self,
        nl: usize,
        nr: usize,
        mut neighbours: impl FnMut(usize) -> I,
        mut rng: Option<&mut RandomState>,
    ) -> usize
    where
        I: IntoIterator<Item = usize>,
    {
        let Self {
            adjacency,
            starts,
            l_to_r,
            r_to_l,
            l_layer,
            r_layer,
            l_queue,
            r_queue,
            aug_path,
            dfs_state,
            l_order,
        } = self;

        adjacency.clear();
        starts.clear();
        starts.push(0);
        for l in 0..nl {
            for r in neighbours(l) {
                assert!(r < nr, "left vertex {l} lists right vertex {r} of {nr}");
                adjacency.push(r);
            }
            starts.push(adjacency.len());
        }

        let nmin = nl.min(nr);
        reset(l_to_r, nl, None);
        reset(r_to_l, nr, None);
        reset(aug_path, 2 * nmin, 0);
        // C sizes this `nmin`, but the search can reach depth `nmin` and
        // then writes one element past the end (into its `Lorder[0]`, which
        // has been read by then, so C is unaffected). One more slot here.
        reset(dfs_state, nmin + 1, 0);

        // Each pass: a breadth-first search from the free left vertices
        // layers the graph along alternating paths (unmatched edges left to
        // right, matched edges right to left) until it reaches a free right
        // vertex; then a depth-first search through those layers finds as
        // many vertex-disjoint shortest augmenting paths as it can, and
        // flips each one, growing the matching by one per path. Stops when
        // no free right vertex can be reached.
        loop {
            reset(l_layer, nl, None);
            reset(r_layer, nr, None);
            l_queue.clear();
            for l in 0..nl {
                if l_to_r[l].is_none() {
                    l_layer[l] = Some(0);
                    l_queue.push(l);
                }
            }

            let mut layer = 0;
            let target_layer = loop {
                let mut found_free_r = false;
                r_queue.clear();
                for &l in l_queue.iter() {
                    assert_eq!(l_layer[l], Some(layer));
                    for &r in &adjacency[starts[l]..starts[l + 1]] {
                        // C's check for the matched edge is redundant (`l`
                        // was reached through its partner, which already
                        // has a layer) but kept, as in C.
                        if Some(r) != l_to_r[l] && r_layer[r].is_none() {
                            r_layer[r] = Some(layer + 1);
                            r_queue.push(r);
                            if r_to_l[r].is_none() {
                                found_free_r = true;
                            }
                        }
                    }
                }
                layer += 1;

                if found_free_r {
                    break layer;
                }
                if r_queue.is_empty() {
                    return finish(l_to_r);
                }

                l_queue.clear();
                for &r in r_queue.iter() {
                    assert_eq!(r_layer[r], Some(layer));
                    if let Some(l) = r_to_l[r]
                        && l_layer[l].is_none()
                    {
                        l_layer[l] = Some(layer + 1);
                        l_queue.push(l);
                    }
                }
                layer += 1;

                if l_queue.is_empty() {
                    return finish(l_to_r);
                }
            };

            // Only free vertices in the target layer end a path; dropping
            // the others saves a special case in the search below.
            for r in 0..nr {
                if r_layer[r] == Some(target_layer) && r_to_l[r].is_some() {
                    r_layer[r] = None;
                }
            }

            l_order.clear();
            l_order.extend(0..nl);
            if let Some(rng) = rng.as_deref_mut() {
                rng.shuffle(l_order);
            }

            // `i` is the number of left vertices on the path so far.
            // Visited vertices get their layer cleared, so each vertex is
            // used by at most one path per pass. (For left vertices this is
            // belt and braces, as in C: each is reachable only through its
            // partner, or once from `l_order` if free.)
            dfs_state[0] = 0;
            let mut i = 0;
            loop {
                let l = if i == 0 {
                    // Look for the next free left vertex to start from.
                    if dfs_state[0] == nl {
                        break;
                    }
                    let l = l_order[dfs_state[0]];
                    dfs_state[0] += 1;
                    if l_layer[l] != Some(0) {
                        continue;
                    }
                    l
                } else {
                    // Try the next neighbour of the path's last left vertex.
                    let prev = aug_path[2 * i - 2];
                    let list = &mut adjacency[starts[prev]..starts[prev + 1]];
                    let j = dfs_state[i];
                    dfs_state[i] += 1;
                    if j == list.len() {
                        i -= 1;
                        continue;
                    }
                    if let Some(rng) = rng.as_deref_mut()
                        && list.len() - j > 1
                    {
                        let limit = u32::try_from(list.len() - j).unwrap_or(u32::MAX);
                        let which = j + rng.upto(limit) as usize;
                        list.swap(which, j);
                    }
                    let r = list[j];

                    if r_layer[r] != Some(2 * i - 1) {
                        continue;
                    }
                    aug_path[2 * i - 1] = r;
                    r_layer[r] = None;

                    if 2 * i - 1 == target_layer {
                        // An augmenting path from a free left vertex to a
                        // free right one: match each left vertex to the
                        // right vertex after it, then start a new path.
                        for &[l, r] in aug_path[..2 * i].as_chunks::<2>().0 {
                            l_to_r[l] = Some(r);
                            r_to_l[r] = Some(l);
                        }
                        i = 0;
                        continue;
                    }

                    // `r` is short of the target layer, so it is matched
                    // (a free one would have ended the breadth-first search).
                    let l = r_to_l[r].expect("right vertex before the target layer is matched");
                    if l_layer[l] != Some(2 * i) {
                        continue;
                    }
                    l
                };

                aug_path[2 * i] = l;
                l_layer[l] = None;
                i += 1;
                dfs_state[i] = 0;
            }
        }
    }

    /// The partner of each left vertex in the last matching found, `None`
    /// if unmatched (C's `outl`).
    #[doc(alias = "outl")]
    #[must_use]
    pub fn left_to_right(&self) -> &[Option<usize>] {
        &self.l_to_r
    }

    /// The partner of each right vertex in the last matching found, `None`
    /// if unmatched (C's `outr`).
    #[doc(alias = "outr")]
    #[must_use]
    pub fn right_to_left(&self) -> &[Option<usize>] {
        &self.r_to_l
    }

    /// A labelling that proves the last matching is maximum (C's
    /// `matching_witness`): one flag per left vertex, then one per right
    /// vertex.
    ///
    /// With `false` as label 0 and `true` as label 1: every unmatched
    /// vertex is a left 0 or a right 1, every edge from a left 0 to a
    /// right 1 is in the matching, and no edge from a left 1 to a right 0
    /// is. Any larger matching would need an augmenting path from a left 0
    /// to a right 1 that never crosses from a 0 to a 1, which can't exist.
    #[doc(alias = "matching_witness")]
    #[must_use]
    pub fn witness(&self) -> (Vec<bool>, Vec<bool>) {
        (
            self.l_layer.iter().map(Option::is_none).collect(),
            self.r_layer.iter().map(Option::is_none).collect(),
        )
    }
}

/// Clears `v` and refills it with `len` copies of `value`, keeping its
/// allocation.
fn reset<T: Clone>(v: &mut Vec<T>, len: usize, value: T) {
    v.clear();
    v.resize(len, value);
}

/// The number of matched pairs.
fn finish(l_to_r: &[Option<usize>]) -> usize {
    l_to_r.iter().filter(|r| r.is_some()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        nl: usize,
        nr: usize,
        adjacency: &[&[usize]],
        rng: Option<&mut RandomState>,
    ) -> (usize, Matcher) {
        let mut matcher = Matcher::new();
        let count = matcher.run(nl, nr, |l| adjacency[l].iter().copied(), rng);
        (count, matcher)
    }

    /// Checks everything `auxiliary/matching.c` checks: the two outputs
    /// agree, every pair is an edge, the count is right, and the witness
    /// proves maximality.
    fn check(nl: usize, nr: usize, adjacency: &[&[usize]], count: usize, matcher: &Matcher) {
        let (outl, outr) = (matcher.left_to_right(), matcher.right_to_left());
        assert_eq!(outl.len(), nl);
        assert_eq!(outr.len(), nr);
        for (l, &r) in outl.iter().enumerate() {
            if let Some(r) = r {
                assert_eq!(outr[r], Some(l));
                assert!(adjacency[l].contains(&r), "{l}-{r} is not an edge");
            }
        }
        for (r, &l) in outr.iter().enumerate() {
            if let Some(l) = l {
                assert_eq!(outl[l], Some(r));
            }
        }
        assert_eq!(outl.iter().flatten().count(), count);
        assert_eq!(outr.iter().flatten().count(), count);

        let (wl, wr) = matcher.witness();
        for l in 0..nl {
            if outl[l].is_none() {
                assert!(!wl[l], "unmatched left {l} must be labelled 0");
            }
            for &r in adjacency[l] {
                if outl[l] == Some(r) {
                    assert!(!wl[l] || wr[r], "matched L1-R0 edge {l}-{r}");
                } else {
                    assert!(wl[l] || !wr[r], "unmatched L0-R1 edge {l}-{r}");
                }
            }
        }
        for r in 0..nr {
            if outr[r].is_none() {
                assert!(wr[r], "unmatched right {r} must be labelled 1");
            }
        }
    }

    #[test]
    fn empty_sides_match_nothing() {
        for (nl, nr) in [(0, 0), (0, 3), (3, 0)] {
            let adjacency = vec![&[][..]; nl];
            let (count, matcher) = run(nl, nr, &adjacency, None);
            assert_eq!(count, 0);
            check(nl, nr, &adjacency, count, &matcher);
        }
    }

    #[test]
    fn single_edge_is_matched() {
        let (count, matcher) = run(1, 1, &[&[0]], None);
        assert_eq!(count, 1);
        assert_eq!(matcher.left_to_right(), [Some(0)]);
        // The final search starts from no free left vertex, so it reaches
        // nothing and every vertex is labelled 1.
        assert_eq!(matcher.witness(), (vec![true], vec![true]));
    }

    #[test]
    fn needs_an_augmenting_path() {
        // The greedy pairing 0-0 blocks left 1; the maximum is 0-1, 1-0.
        let adjacency: [&[usize]; 2] = [&[0, 1], &[0]];
        let (count, matcher) = run(2, 2, &adjacency, None);
        assert_eq!(count, 2);
        assert_eq!(matcher.left_to_right(), [Some(1), Some(0)]);
        check(2, 2, &adjacency, count, &matcher);
    }

    #[test]
    fn long_augmenting_chain() {
        // Left l prefers right l + 1 over right l; the last left vertex has
        // only right n - 1. The first pass takes every preferred edge,
        // leaving the last left vertex to an augmenting path through all
        // the others back to right 0.
        let n = 12;
        let lists: Vec<Vec<usize>> = (0..n)
            .map(|l| if l + 1 < n { vec![l + 1, l] } else { vec![l] })
            .collect();
        let adjacency: Vec<&[usize]> = lists.iter().map(Vec::as_slice).collect();
        let (count, matcher) = run(n, n, &adjacency, None);
        assert_eq!(count, n);
        check(n, n, &adjacency, count, &matcher);
    }

    #[test]
    fn deficient_graph_has_a_witness() {
        // Three left vertices share two right vertices.
        let adjacency: [&[usize]; 3] = [&[0, 1], &[0], &[1]];
        let (count, matcher) = run(3, 2, &adjacency, None);
        assert_eq!(count, 2);
        check(3, 2, &adjacency, count, &matcher);
    }

    #[test]
    fn repeated_neighbours_are_harmless() {
        let adjacency: [&[usize]; 2] = [&[0, 0, 1, 0], &[0, 0]];
        let mut rng = RandomState::new(b"repeat");
        let (count, matcher) = run(2, 2, &adjacency, Some(&mut rng));
        assert_eq!(count, 2);
        check(2, 2, &adjacency, count, &matcher);
    }

    #[test]
    fn subset_graph_from_upstream_test() {
        // auxiliary/matching.c's autotest: each subset of {0..b} joined to
        // the subsets with one element removed. Same-sized subsets form
        // separate graphs, each matching its smaller side, so the maximum
        // is n minus the largest layer.
        let b = 6;
        let n = 1 << b;
        let lists: Vec<Vec<usize>> = (0..n)
            .map(|i: usize| {
                (0..b)
                    .filter(|j| i & (1 << j) != 0)
                    .map(|j| i & !(1 << j))
                    .collect()
            })
            .collect();
        let adjacency: Vec<&[usize]> = lists.iter().map(Vec::as_slice).collect();
        let expected = lists.iter().filter(|list| list.len() != b / 2).count();
        let mut rng = RandomState::new(b"fixed random seed for repeatability");
        let mut matcher = Matcher::new();
        for _ in 0..20 {
            let count = matcher.run(n, n, |l| adjacency[l].iter().copied(), Some(&mut rng));
            assert_eq!(count, expected);
            check(n, n, &adjacency, count, &matcher);
        }
    }

    /// Six left vertices, five right: several maximum matchings.
    const SHARED: [&[usize]; 6] = [&[0, 1, 2], &[0], &[1, 3], &[2, 3, 4], &[0, 1], &[4, 2]];

    #[test]
    fn deterministic_matching_is_cs() {
        // Golden values recorded from the C oracle.
        let (count, matcher) = run(6, 5, &SHARED, None);
        assert_eq!(count, 5);
        assert_eq!(
            matcher.left_to_right(),
            [Some(0), None, Some(3), Some(2), Some(1), Some(4)]
        );
        assert_eq!(
            matcher.right_to_left(),
            [Some(0), Some(4), Some(3), Some(2), Some(5)]
        );
        assert_eq!(matcher.witness(), (vec![false; 6], vec![false; 5]));
        check(6, 5, &SHARED, count, &matcher);
    }

    #[test]
    fn randomised_matchings_are_cs() {
        // Golden values recorded from the C oracle: the matching, then the
        // generator's next 32 bits (so the draws used were the same).
        let golden = [
            (
                &b"golden"[..],
                [Some(2), Some(0), Some(1), Some(3), None, Some(4)],
                1_808_815_247,
            ),
            (
                b"other",
                [Some(2), None, Some(1), Some(3), Some(0), Some(4)],
                3_039_348_307,
            ),
            (
                b"third",
                [Some(1), None, Some(3), Some(2), Some(0), Some(4)],
                3_811_770_810,
            ),
        ];
        for (seed, outl, next) in golden {
            let mut rng = RandomState::new(seed);
            let (count, matcher) = run(6, 5, &SHARED, Some(&mut rng));
            assert_eq!(count, 5);
            assert_eq!(matcher.left_to_right(), outl);
            assert_eq!(rng.bits(32), next);
            check(6, 5, &SHARED, count, &matcher);
        }
    }

    #[test]
    fn latin_square_is_cs() {
        // latin.c's row-by-row generation (without its row shuffle); golden
        // square and next draw recorded from the C oracle.
        let order = 6;
        let mut rng = RandomState::new(b"golden");
        let mut matcher = Matcher::new();
        let mut square: Vec<Vec<usize>> = Vec::new();
        for _ in 0..order {
            let lists: Vec<Vec<usize>> = (0..order)
                .map(|col| {
                    (0..order)
                        .filter(|&d| square.iter().all(|row| row[col] != d))
                        .collect()
                })
                .collect();
            let count = matcher.run(order, order, |l| lists[l].iter().copied(), Some(&mut rng));
            assert_eq!(count, order);
            square.push(matcher.left_to_right().iter().flatten().copied().collect());
        }
        assert_eq!(
            square,
            [
                [2, 0, 4, 3, 1, 5],
                [1, 5, 3, 2, 0, 4],
                [5, 3, 1, 4, 2, 0],
                [4, 1, 0, 5, 3, 2],
                [3, 2, 5, 0, 4, 1],
                [0, 4, 2, 1, 5, 3],
            ]
        );
        assert_eq!(rng.bits(32), 1_095_805_807);
    }

    #[test]
    #[should_panic(expected = "lists right vertex 2 of 2")]
    fn out_of_range_neighbour_panics() {
        let _ = run(1, 2, &[&[2]], None);
    }

    #[test]
    fn matcher_can_be_reused_for_a_smaller_graph() {
        let mut matcher = Matcher::new();
        let big: [&[usize]; 3] = [&[0], &[1], &[2]];
        assert_eq!(matcher.run(3, 3, |l| big[l].iter().copied(), None), 3);
        let small: [&[usize]; 1] = [&[]];
        assert_eq!(matcher.run(1, 2, |l| small[l].iter().copied(), None), 0);
        assert_eq!(matcher.left_to_right(), [None]);
        assert_eq!(matcher.right_to_left(), [None, None]);
        assert_eq!(matcher.witness(), (vec![false], vec![true, true]));
    }
}
