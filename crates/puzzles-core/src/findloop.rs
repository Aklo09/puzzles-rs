//! Port of `third_party/puzzles-c/findloop.c` (upstream 616da16).
//!
//! Finds which edges of an undirected graph lie on a loop, and which are
//! bridges (edges whose removal would split a connected component). Used
//! by Bridges, Net, Slant and Tracks to highlight loops, and by Dominosa's
//! solver, which also needs the size of each side of a bridge.
//!
//! The graph is given by a closure listing each vertex's neighbours, so
//! games don't have to build a graph structure. The answers are graph
//! properties, but this is a faithful port of C's iterative DFS so that the
//! edge cases behave identically (see [`LoopFinder::is_loop_edge`]).

use std::cmp::min;

/// Per-vertex DFS bookkeeping (C's `struct findloopstate`).
#[derive(Clone, Debug, Default)]
struct Vertex {
    /// Depth in the DFS tree once visited; `None` before the first visit.
    depth: Option<usize>,
    /// Shallowest depth reachable from this vertex's subtree by a back edge.
    shallowest_reachable: usize,
    /// Number of vertices in this vertex's DFS subtree.
    subtree_size: usize,
    parent: Option<usize>,
    /// Root of this vertex's DFS tree (its connected component).
    component_root: usize,
    /// Visiting order: a doubly-linked list over all vertices.
    prev: Option<usize>,
    next: Option<usize>,
}

/// Loop and bridge finder (C's `findloopstate` plus `findloop_run`).
///
/// Reusable: [`run`](Self::run) analyses a graph, then the query methods
/// answer for that graph until the next run.
///
/// ```
/// use puzzles_core::findloop::LoopFinder;
///
/// // A triangle 0-1-2 with a tail 2-3.
/// let adjacency: [&[usize]; 4] = [&[1, 2], &[0, 2], &[0, 1, 3], &[2]];
/// let mut finder = LoopFinder::new();
/// assert!(finder.run(4, |v| adjacency[v].iter().copied()));
/// assert!(finder.is_loop_edge(0, 1));
/// assert!(!finder.is_loop_edge(2, 3));
/// assert_eq!(finder.is_bridge(2, 3), Some((3, 1)));
/// ```
#[doc(alias = "findloopstate")]
#[derive(Clone, Debug, Default)]
pub struct LoopFinder {
    vertices: Vec<Vertex>,
    has_loop: bool,
}

impl LoopFinder {
    /// Creates a finder; buffers grow to fit the graphs it is run on (C's
    /// `findloop_new_state`).
    #[doc(alias = "findloop_new_state")]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyses the undirected graph on vertices `0..n` whose edges are
    /// given by `neighbours(v)`, and returns whether it has any loop (C's
    /// `findloop_run`).
    ///
    /// `neighbours` must be symmetric (`w` is a neighbour of `v` iff `v` is
    /// a neighbour of `w`). A vertex listing itself counts as a loop.
    ///
    /// # Panics
    /// If `neighbours` returns a vertex `>= n`.
    #[doc(alias = "findloop_run")]
    pub fn run<I>(&mut self, n: usize, mut neighbours: impl FnMut(usize) -> I) -> bool
    where
        I: IntoIterator<Item = usize>,
    {
        let vertices = &mut self.vertices;
        vertices.clear();
        vertices.extend((0..n).map(|u| Vertex {
            depth: None,
            shallowest_reachable: n,
            subtree_size: 1,
            parent: None,
            component_root: u,
            prev: u.checked_sub(1),
            next: (u + 1 < n).then_some(u + 1),
        }));
        let mut has_loop = false;

        // Each vertex is visited twice: on the way down (depth still None),
        // when its unvisited neighbours become its children and are
        // scheduled before it in the list; and on the way back up, when its
        // subtree's statistics are folded into its parent.
        let mut cursor = (n > 0).then_some(0);
        while let Some(u) = cursor {
            if vertices[u].depth.is_none() {
                let (depth, root) = match vertices[u].parent {
                    None => (0, u),
                    Some(p) => (
                        vertices[p].depth.map_or(0, |d| d + 1),
                        vertices[p].component_root,
                    ),
                };
                vertices[u].depth = Some(depth);
                vertices[u].component_root = root;

                // `next_visit` is the first scheduled vertex: the most
                // recently found child, or `u` itself if it has none.
                let mut next_visit = u;
                for w in neighbours(u) {
                    if Some(w) == vertices[u].parent {
                        continue;
                    }
                    match vertices[w].depth {
                        Some(w_depth) => {
                            // Back edge to an ancestor (or a self-loop).
                            vertices[u].shallowest_reachable =
                                min(vertices[u].shallowest_reachable, w_depth);
                            has_loop = true;
                        }
                        // A repeat of the child just scheduled: nothing to
                        // do. (C would corrupt its list into a cycle and
                        // hang here; no caller lists a neighbour twice.)
                        None if w == next_visit => {}
                        None => {
                            vertices[w].parent = Some(u);
                            Self::unlink(vertices, w);
                            Self::insert_before(vertices, w, next_visit);
                            next_visit = w;
                        }
                    }
                }
                cursor = Some(next_visit);
            } else {
                if let Some(p) = vertices[u].parent {
                    vertices[p].subtree_size += vertices[u].subtree_size;
                    vertices[p].shallowest_reachable = min(
                        vertices[p].shallowest_reachable,
                        vertices[u].shallowest_reachable,
                    );
                }
                cursor = vertices[u].next;
            }
        }

        self.has_loop = has_loop;
        has_loop
    }

    /// Whether the last graph analysed had any loop (the value
    /// [`run`](Self::run) returned).
    #[must_use]
    pub fn has_loop(&self) -> bool {
        self.has_loop
    }

    /// Whether the edge `u`–`v` lies on a loop (C's
    /// `findloop_is_loop_edge`).
    ///
    /// Quirk kept from C: for a pair that is *not* an edge this returns
    /// `true`, because internally only bridges are recorded. Callers rely
    /// on this when they test pairs that may not be connected.
    ///
    /// # Panics
    /// If `u` or `v` is not a vertex of the last graph analysed.
    #[doc(alias = "findloop_is_loop_edge")]
    #[must_use]
    pub fn is_loop_edge(&self, u: usize, v: usize) -> bool {
        !(self.is_bridge_child(u, v) || self.is_bridge_child(v, u))
    }

    /// If `u`–`v` is a bridge, returns the number of vertices that would be
    /// on `u`'s side and on `v`'s side after removing it; `None` if it isn't
    /// a bridge (or isn't an edge). C's `findloop_is_bridge`, with the two
    /// out-parameters returned instead.
    ///
    /// # Panics
    /// If `u` or `v` is not a vertex of the last graph analysed.
    #[doc(alias = "findloop_is_bridge")]
    #[must_use]
    pub fn is_bridge(&self, u: usize, v: usize) -> Option<(usize, usize)> {
        if self.is_bridge_child(u, v) {
            let (child_side, rest) = self.split_sizes(u);
            Some((child_side, rest))
        } else if self.is_bridge_child(v, u) {
            let (child_side, rest) = self.split_sizes(v);
            Some((rest, child_side))
        } else {
            None
        }
    }

    /// True if `child`–`parent` is a DFS tree edge that no back edge from
    /// `child`'s subtree bypasses, i.e. a bridge.
    fn is_bridge_child(&self, child: usize, parent: usize) -> bool {
        let c = &self.vertices[child];
        c.parent == Some(parent) && c.depth.is_some_and(|d| c.shallowest_reachable >= d)
    }

    /// Sizes of `child`'s subtree and of the rest of its component.
    fn split_sizes(&self, child: usize) -> (usize, usize) {
        let c = &self.vertices[child];
        let component = self.vertices[c.component_root].subtree_size;
        (c.subtree_size, component - c.subtree_size)
    }

    fn unlink(vertices: &mut [Vertex], w: usize) {
        let (prev, next) = (vertices[w].prev, vertices[w].next);
        if let Some(p) = prev {
            vertices[p].next = next;
        }
        if let Some(n) = next {
            vertices[n].prev = prev;
        }
    }

    fn insert_before(vertices: &mut [Vertex], w: usize, before: usize) {
        let prev = vertices[before].prev;
        vertices[w].prev = prev;
        vertices[w].next = Some(before);
        if let Some(p) = prev {
            vertices[p].next = Some(w);
        }
        vertices[before].prev = Some(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyse(n: usize, edges: &[(usize, usize)]) -> LoopFinder {
        let mut adjacency = vec![Vec::new(); n];
        for &(a, b) in edges {
            adjacency[a].push(b);
            if a != b {
                adjacency[b].push(a);
            }
        }
        let mut finder = LoopFinder::new();
        finder.run(n, |v| adjacency[v].clone());
        finder
    }

    #[test]
    fn tree_has_no_loop() {
        let finder = analyse(5, &[(0, 1), (1, 2), (1, 3), (3, 4)]);
        assert!(!finder.has_loop());
    }

    #[test]
    fn every_tree_edge_is_a_bridge() {
        let finder = analyse(4, &[(0, 1), (1, 2), (2, 3)]);
        assert!(
            [(0, 1), (1, 2), (2, 3)]
                .iter()
                .all(|&(a, b)| !finder.is_loop_edge(a, b))
        );
    }

    #[test]
    fn cycle_edges_are_loop_edges() {
        let finder = analyse(4, &[(0, 1), (1, 2), (2, 3), (3, 0)]);
        assert!(finder.has_loop());
        assert!(
            [(0, 1), (1, 2), (2, 3), (3, 0)]
                .iter()
                .all(|&(a, b)| finder.is_loop_edge(a, b))
        );
    }

    #[test]
    fn bridge_between_two_cycles_is_found_with_side_sizes() {
        // Triangle 0-1-2, bridge 2-3, square 3-4-5-6.
        let edges = [
            (0, 1),
            (1, 2),
            (2, 0),
            (2, 3),
            (3, 4),
            (4, 5),
            (5, 6),
            (6, 3),
        ];
        let finder = analyse(7, &edges);
        assert_eq!(finder.is_bridge(2, 3), Some((3, 4)));
        assert_eq!(finder.is_bridge(3, 2), Some((4, 3)));
        assert_eq!(finder.is_bridge(0, 1), None);
    }

    #[test]
    fn side_sizes_are_per_component() {
        // Component {0, 1} and component {2, 3, 4} (a path).
        let finder = analyse(5, &[(0, 1), (2, 3), (3, 4)]);
        assert_eq!(finder.is_bridge(0, 1), Some((1, 1)));
        assert_eq!(finder.is_bridge(3, 4), Some((2, 1)));
    }

    #[test]
    fn non_edge_counts_as_loop_edge_like_c() {
        let finder = analyse(3, &[(0, 1)]);
        assert!(finder.is_loop_edge(0, 2));
        assert_eq!(finder.is_bridge(0, 2), None);
    }

    #[test]
    fn self_loop_is_a_loop_but_not_a_loop_edge_elsewhere() {
        let finder = analyse(2, &[(0, 1), (1, 1)]);
        assert!(finder.has_loop());
        assert!(!finder.is_loop_edge(0, 1));
    }

    #[test]
    fn repeated_neighbour_is_ignored() {
        let adjacency: [&[usize]; 2] = [&[1, 1], &[0]];
        let mut finder = LoopFinder::new();
        assert!(!finder.run(2, |v| adjacency[v].iter().copied()));
        assert_eq!(finder.is_bridge(0, 1), Some((1, 1)));
    }

    #[test]
    fn empty_graph_has_no_loop() {
        assert!(!LoopFinder::new().run(0, |_| std::iter::empty()));
    }

    #[test]
    fn finder_can_be_reused_for_another_graph() {
        let mut finder = analyse(3, &[(0, 1), (1, 2), (2, 0)]);
        let path: [&[usize]; 2] = [&[1], &[0]];
        assert!(!finder.run(2, |v| path[v].iter().copied()));
        assert!(!finder.is_loop_edge(0, 1));
    }
}
