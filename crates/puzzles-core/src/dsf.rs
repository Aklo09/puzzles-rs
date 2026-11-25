//! Port of `third_party/puzzles-c/dsf.c` (upstream 616da16).
//!
//! Disjoint-set forests (union-find), used by most solvers and generators
//! to track connected regions and to avoid closing loops.
//!
//! C has one `DSF` type in three flavours chosen at construction, and
//! asserts at runtime if an operation doesn't match the flavour. Here each
//! flavour is its own type, so a mismatch is a compile error:
//!
//! - [`Dsf`]: plain union-find.
//! - [`MinDsf`]: also tracks the smallest element of each set.
//! - [`FlipDsf`]: also tracks, for each element, whether it is "flipped"
//!   relative to its set's root, for puzzles where two elements must be
//!   equal or opposite (C's `dsf_*_flip`).
//!
//! The operations all three share are in the [`DisjointSets`] trait.
//!
//! **Compatibility:** games use the representative ("canonical") element of
//! a set as an index, so which element becomes the root must match C:
//! unions are by size, and on a tie the root of the *second* argument wins.
//! Path compression doesn't change roots, only speed.

/// Operations shared by every kind of disjoint-set forest.
///
/// Elements are `0..len()`; each starts in a set of its own.
pub trait DisjointSets {
    /// Number of elements.
    fn len(&self) -> usize;

    /// True if there are no elements.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the representative element of `n`'s set (C's
    /// `dsf_canonify`). Takes `&mut self` because it compresses paths.
    ///
    /// # Panics
    /// If `n >= self.len()`.
    #[doc(alias = "dsf_canonify")]
    fn find(&mut self, n: usize) -> usize;

    /// Number of elements in `n`'s set (C's `dsf_size`).
    ///
    /// # Panics
    /// If `n >= self.len()`.
    #[doc(alias = "dsf_size")]
    fn set_size(&mut self, n: usize) -> usize;

    /// True if `a` and `b` are in the same set (C's `dsf_equivalent`).
    ///
    /// # Panics
    /// If `a` or `b` is out of range.
    #[doc(alias = "dsf_equivalent")]
    fn same_set(&mut self, a: usize, b: usize) -> bool {
        self.find(a) == self.find(b)
    }

    /// Puts every element back into a set of its own (C's `dsf_reinit`).
    #[doc(alias = "dsf_reinit")]
    fn reset(&mut self);
}

/// The forest shared by all three kinds: parent links plus set sizes.
#[derive(Clone, Debug)]
struct Forest {
    /// Parent of each element; an element is a root iff it is its own parent.
    parent: Vec<usize>,
    /// Number of elements in the set, meaningful only at roots.
    size: Vec<usize>,
}

impl Forest {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            size: vec![1; len],
        }
    }

    fn reset(&mut self) {
        for (n, parent) in self.parent.iter_mut().enumerate() {
            *parent = n;
        }
        self.size.fill(1);
    }

    fn is_root(&self, n: usize) -> bool {
        self.parent[n] == n
    }

    fn find_root(&self, mut n: usize) -> usize {
        while !self.is_root(n) {
            n = self.parent[n];
        }
        n
    }

    /// Points every element on the path from `n` directly at `root`.
    fn compress(&mut self, mut n: usize, root: usize) {
        while !self.is_root(n) {
            let next = self.parent[n];
            self.parent[n] = root;
            n = next;
        }
        debug_assert_eq!(n, root);
    }

    fn find(&mut self, n: usize) -> usize {
        let root = self.find_root(n);
        self.compress(n, root);
        root
    }

    /// Links two distinct roots and returns the new root. The bigger set's
    /// root wins; on a tie `r2` wins, exactly as in C.
    fn link(&mut self, r1: usize, r2: usize) -> usize {
        debug_assert_ne!(r1, r2);
        let (root, child) = if self.size[r1] > self.size[r2] {
            (r1, r2)
        } else {
            (r2, r1)
        };
        self.parent[child] = root;
        self.size[root] = self.size[r1] + self.size[r2];
        root
    }

    /// Merges the sets of `a` and `b`; returns `(root, old roots)`, the old
    /// roots being `None` if they were already one set.
    fn union(&mut self, a: usize, b: usize) -> (usize, Option<(usize, usize)>) {
        let (r1, r2) = (self.find_root(a), self.find_root(b));
        let merged = (r1 != r2).then_some((r1, r2));
        let root = if r1 == r2 { r1 } else { self.link(r1, r2) };
        self.compress(a, root);
        self.compress(b, root);
        (root, merged)
    }
}

/// Plain disjoint-set forest (C's `dsf_new`).
///
/// ```
/// use puzzles_core::dsf::{DisjointSets, Dsf};
///
/// let mut dsf = Dsf::new(5);
/// dsf.union(0, 3);
/// dsf.union(3, 4);
/// assert!(dsf.same_set(0, 4));
/// assert_eq!(dsf.set_size(0), 3);
/// assert!(!dsf.same_set(1, 2));
/// ```
#[derive(Clone, Debug)]
pub struct Dsf {
    forest: Forest,
}

impl Dsf {
    /// Creates `len` elements, each in a set of its own (C's `dsf_new`).
    #[doc(alias = "dsf_new")]
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            forest: Forest::new(len),
        }
    }

    /// Merges the sets containing `a` and `b` (C's `dsf_merge`).
    ///
    /// # Panics
    /// If `a` or `b` is out of range.
    #[doc(alias = "dsf_merge")]
    pub fn union(&mut self, a: usize, b: usize) {
        self.forest.union(a, b);
    }
}

impl DisjointSets for Dsf {
    fn len(&self) -> usize {
        self.forest.parent.len()
    }

    fn find(&mut self, n: usize) -> usize {
        self.forest.find(n)
    }

    fn set_size(&mut self, n: usize) -> usize {
        let root = self.forest.find(n);
        self.forest.size[root]
    }

    fn reset(&mut self) {
        self.forest.reset();
    }
}

/// Disjoint-set forest that also tracks the smallest element of each set
/// (C's `dsf_new_min`).
///
/// ```
/// use puzzles_core::dsf::{DisjointSets, MinDsf};
///
/// let mut dsf = MinDsf::new(6);
/// dsf.union(5, 2);
/// dsf.union(2, 4);
/// assert_eq!(dsf.min_element(4), 2);
/// assert_eq!(dsf.min_element(1), 1);
/// ```
#[derive(Clone, Debug)]
pub struct MinDsf {
    forest: Forest,
    /// Smallest element of each set, meaningful only at roots.
    min: Vec<usize>,
}

impl MinDsf {
    /// Creates `len` elements, each in a set of its own (C's
    /// `dsf_new_min`).
    #[doc(alias = "dsf_new_min")]
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            forest: Forest::new(len),
            min: (0..len).collect(),
        }
    }

    /// Merges the sets containing `a` and `b` (C's `dsf_merge`).
    ///
    /// # Panics
    /// If `a` or `b` is out of range.
    #[doc(alias = "dsf_merge")]
    pub fn union(&mut self, a: usize, b: usize) {
        if let (root, Some((r1, r2))) = self.forest.union(a, b) {
            self.min[root] = self.min[r1].min(self.min[r2]);
        }
    }

    /// Smallest element in `n`'s set (C's `dsf_minimal`).
    ///
    /// # Panics
    /// If `n >= self.len()`.
    #[doc(alias = "dsf_minimal")]
    pub fn min_element(&mut self, n: usize) -> usize {
        let root = self.forest.find(n);
        self.min[root]
    }
}

impl DisjointSets for MinDsf {
    fn len(&self) -> usize {
        self.forest.parent.len()
    }

    fn find(&mut self, n: usize) -> usize {
        self.forest.find(n)
    }

    fn set_size(&mut self, n: usize) -> usize {
        let root = self.forest.find(n);
        self.forest.size[root]
    }

    fn reset(&mut self) {
        self.forest.reset();
        for (n, min) in self.min.iter_mut().enumerate() {
            *min = n;
        }
    }
}

/// Disjoint-set forest that also records whether each element is
/// "flipped" (opposite) relative to the others in its set (C's
/// `dsf_new_flip`). Used for constraints like "these two edges are both on
/// the loop or both off" versus "exactly one of them is on".
///
/// ```
/// use puzzles_core::dsf::{DisjointSets, FlipDsf};
///
/// let mut dsf = FlipDsf::new(3);
/// dsf.union(0, 1, true); // 0 and 1 are opposite
/// dsf.union(1, 2, true); // 1 and 2 are opposite, so 0 and 2 are equal
/// let (root0, flipped0) = dsf.find_with_flip(0);
/// let (root2, flipped2) = dsf.find_with_flip(2);
/// assert_eq!(root0, root2);
/// assert_eq!(flipped0, flipped2);
/// ```
#[derive(Clone, Debug)]
pub struct FlipDsf {
    forest: Forest,
    /// Whether each non-root element is flipped relative to its parent.
    flip: Vec<bool>,
}

impl FlipDsf {
    /// Creates `len` elements, each in a set of its own (C's
    /// `dsf_new_flip`).
    #[doc(alias = "dsf_new_flip")]
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            forest: Forest::new(len),
            flip: vec![false; len],
        }
    }

    /// Returns the representative of `n`'s set and whether `n` is flipped
    /// relative to it (C's `dsf_canonify_flip`).
    ///
    /// # Panics
    /// If `n >= self.len()`.
    #[doc(alias = "dsf_canonify_flip")]
    pub fn find_with_flip(&mut self, n: usize) -> (usize, bool) {
        let (root, flipped) = self.find_root_with_flip(n);
        self.compress(n, root, flipped);
        (root, flipped)
    }

    /// Merges the sets of `a` and `b`, recording that `b` is flipped
    /// relative to `a` if `flipped` is true, and equal to it otherwise
    /// (C's `dsf_merge_flip`).
    ///
    /// # Panics
    /// If `a` or `b` is out of range, or if they are already in the same
    /// set with the opposite relation (a solver bug: C asserts here too).
    #[doc(alias = "dsf_merge_flip")]
    pub fn union(&mut self, a: usize, b: usize, flipped: bool) {
        let (r1, mut f1) = self.find_root_with_flip(a);
        let (r2, mut f2) = self.find_root_with_flip(b);
        let root = if r1 == r2 {
            assert!(
                !(f1 ^ f2 ^ flipped),
                "inconsistent flip: {a} and {b} are already related the other way"
            );
            r1
        } else {
            let root = self.forest.link(r1, r2);
            // The linked root's flip makes `a` and `b` related as requested;
            // the path from that side now crosses the new link.
            let link_flip = f1 ^ f2 ^ flipped;
            if root == r1 {
                self.flip[r2] = link_flip;
                f2 ^= link_flip;
            } else {
                self.flip[r1] = link_flip;
                f1 ^= link_flip;
            }
            root
        };
        self.compress(a, root, f1);
        self.compress(b, root, f2);
    }

    /// Follows `n` up to its root, accumulating the flip along the way.
    fn find_root_with_flip(&self, mut n: usize) -> (usize, bool) {
        let mut flipped = false;
        while !self.forest.is_root(n) {
            flipped ^= self.flip[n];
            n = self.forest.parent[n];
        }
        (n, flipped)
    }

    /// Points every element on the path from `n` directly at `root`,
    /// rewriting each one's flip to be relative to the root. `flipped` is
    /// `n`'s flip relative to `root`.
    fn compress(&mut self, mut n: usize, root: usize, mut flipped: bool) {
        while !self.forest.is_root(n) {
            let next = self.forest.parent[n];
            let next_flipped = flipped ^ self.flip[n];
            self.flip[n] = flipped;
            self.forest.parent[n] = root;
            n = next;
            flipped = next_flipped;
        }
        debug_assert_eq!(n, root);
    }
}

impl DisjointSets for FlipDsf {
    fn len(&self) -> usize {
        self.forest.parent.len()
    }

    /// The representative of `n`'s set, ignoring the flip. Unlike C's
    /// `dsf_canonify` on a flip dsf, this keeps the flips consistent.
    fn find(&mut self, n: usize) -> usize {
        self.find_with_flip(n).0
    }

    fn set_size(&mut self, n: usize) -> usize {
        let root = self.find(n);
        self.forest.size[root]
    }

    fn reset(&mut self) {
        self.forest.reset();
        // Flips are only meaningful for non-roots, and there are none now.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_elements_are_singletons() {
        let mut dsf = Dsf::new(4);
        assert!((0..4).all(|n| dsf.find(n) == n && dsf.set_size(n) == 1));
    }

    #[test]
    fn union_on_tie_makes_second_root_canonical() {
        let mut dsf = Dsf::new(4);
        dsf.union(0, 1);
        assert_eq!(dsf.find(0), 1);
    }

    #[test]
    fn union_makes_bigger_sets_root_canonical() {
        let mut dsf = Dsf::new(4);
        dsf.union(0, 1); // root 1, size 2
        dsf.union(1, 2); // sizes 2 > 1: root stays 1
        assert_eq!(dsf.find(2), 1);
    }

    #[test]
    fn union_of_same_set_changes_nothing() {
        let mut dsf = Dsf::new(3);
        dsf.union(0, 1);
        dsf.union(1, 0);
        assert_eq!((dsf.find(0), dsf.set_size(0)), (1, 2));
    }

    #[test]
    fn set_size_counts_all_members() {
        let mut dsf = Dsf::new(6);
        for (a, b) in [(0, 1), (2, 3), (1, 3), (4, 5)] {
            dsf.union(a, b);
        }
        let sizes: Vec<usize> = (0..6).map(|n| dsf.set_size(n)).collect();
        assert_eq!(sizes, [4, 4, 4, 4, 2, 2]);
    }

    #[test]
    fn reset_restores_singletons() {
        let mut dsf = Dsf::new(3);
        dsf.union(0, 2);
        dsf.reset();
        assert!(!dsf.same_set(0, 2));
    }

    #[test]
    fn clone_from_copies_the_partition() {
        let mut a = Dsf::new(3);
        a.union(0, 1);
        let mut b = Dsf::new(3);
        b.clone_from(&a);
        assert!(b.same_set(0, 1));
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn find_panics_out_of_range() {
        Dsf::new(2).find(2);
    }

    #[test]
    fn min_element_tracks_smallest_member() {
        let mut dsf = MinDsf::new(8);
        dsf.union(7, 5);
        dsf.union(6, 5);
        assert_eq!(dsf.min_element(7), 5);
        dsf.union(7, 3);
        assert_eq!(dsf.min_element(6), 3);
    }

    #[test]
    fn min_dsf_reset_restores_own_minima() {
        let mut dsf = MinDsf::new(3);
        dsf.union(2, 0);
        dsf.reset();
        assert_eq!(dsf.min_element(2), 2);
    }

    #[test]
    fn flip_relations_compose() {
        let mut dsf = FlipDsf::new(4);
        dsf.union(0, 1, true);
        dsf.union(1, 2, false);
        dsf.union(2, 3, true);
        let flips: Vec<bool> = (0..4).map(|n| dsf.find_with_flip(n).1).collect();
        // 0 ~ 3, 1 ~ 2, and the two pairs are opposite.
        assert_eq!(flips[0], flips[3]);
        assert_eq!(flips[1], flips[2]);
        assert_ne!(flips[0], flips[1]);
    }

    /// Linking two sets must account for the flips both elements already
    /// carry relative to their old roots, on whichever side keeps its root.
    #[test]
    fn flip_union_accounts_for_existing_flips_on_both_sides() {
        let mut dsf = FlipDsf::new(4);
        dsf.union(0, 1, true); // root 1; 0 is flipped relative to it
        dsf.union(2, 0, false); // 2 must equal 0, so be flipped relative to 1
        assert_eq!(dsf.find_with_flip(2).1, dsf.find_with_flip(0).1);
        dsf.union(0, 3, true); // bigger set keeps root 1; 3 must be opposite 0
        assert_ne!(dsf.find_with_flip(3).1, dsf.find_with_flip(0).1);
    }

    #[test]
    fn flip_union_of_consistent_relation_is_accepted() {
        let mut dsf = FlipDsf::new(2);
        dsf.union(0, 1, true);
        dsf.union(1, 0, true);
        assert_eq!(dsf.set_size(0), 2);
    }

    #[test]
    #[should_panic(expected = "inconsistent flip")]
    fn flip_union_of_contradiction_panics() {
        let mut dsf = FlipDsf::new(2);
        dsf.union(0, 1, true);
        dsf.union(0, 1, false);
    }

    #[test]
    fn flip_relations_survive_path_compression() {
        let mut dsf = FlipDsf::new(16);
        for n in 1..16 {
            dsf.union(n - 1, n, n % 3 == 0);
        }
        // Query from the far end first to compress long paths.
        let expected: Vec<bool> = (0..16)
            .scan(false, |acc, n| {
                *acc ^= n != 0 && n % 3 == 0;
                Some(*acc)
            })
            .collect();
        let root_flip = dsf.find_with_flip(0).1;
        for n in (0..16).rev() {
            assert_eq!(dsf.find_with_flip(n).1 ^ root_flip, expected[n], "n = {n}");
        }
    }

    #[test]
    fn generic_code_works_with_every_kind() {
        fn count_sets(dsf: &mut impl DisjointSets) -> usize {
            (0..dsf.len()).filter(|&n| dsf.find(n) == n).count()
        }
        let mut plain = Dsf::new(4);
        plain.union(0, 1);
        let mut min = MinDsf::new(4);
        min.union(2, 3);
        let mut flip = FlipDsf::new(4);
        flip.union(1, 2, true);
        assert_eq!(
            [
                count_sets(&mut plain),
                count_sets(&mut min),
                count_sets(&mut flip)
            ],
            [3, 3, 3]
        );
    }
}
