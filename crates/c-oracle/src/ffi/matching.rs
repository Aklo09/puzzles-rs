//! Bindings to `matching.c`: maximum bipartite matching (Hopcroft–Karp).

use std::ffi::{c_int, c_void};
use std::ptr;

use super::CRandom;
use super::random::RandomStateC;

unsafe extern "C" {
    fn matching_scratch_size(nl: c_int, nr: c_int) -> usize;
    fn matching_with_scratch(
        scratch: *mut c_void,
        nl: c_int,
        nr: c_int,
        adjlists: *mut *mut c_int,
        adjsizes: *mut c_int,
        rs: *mut RandomStateC,
        outl: *mut c_int,
        outr: *mut c_int,
    ) -> c_int;
    fn matching(
        nl: c_int,
        nr: c_int,
        adjlists: *mut *mut c_int,
        adjsizes: *mut c_int,
        rs: *mut RandomStateC,
        outl: *mut c_int,
        outr: *mut c_int,
    ) -> c_int;
    fn matching_witness(scratch: *mut c_void, nl: c_int, nr: c_int, witness: *mut c_int);
}

/// Everything C's matching reports for one graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CMatching {
    /// The return value: the number of matched pairs.
    pub count: usize,
    /// `outl`: the partner of each left vertex.
    pub left_to_right: Vec<Option<usize>>,
    /// `outr`: the partner of each right vertex.
    pub right_to_left: Vec<Option<usize>>,
    /// `matching_witness`, split into the left and right labels.
    pub witness: (Vec<bool>, Vec<bool>),
}

/// C's adjacency lists, kept as `c_int`s with the pointer array C wants.
struct CGraph {
    nl: c_int,
    nr: c_int,
    /// Owns the buffers `pointers` points into. Moving a `Vec` doesn't move
    /// its heap buffer, so the pointers stay valid while `self` lives.
    #[expect(dead_code, reason = "only read by C, through `pointers`")]
    lists: Vec<Vec<c_int>>,
    pointers: Vec<*mut c_int>,
    sizes: Vec<c_int>,
}

impl CGraph {
    /// # Panics
    /// If a neighbour is `>= nr` (C would index out of bounds) or a size
    /// exceeds `c_int`.
    fn new(nr: usize, adjacency: &[Vec<usize>]) -> Self {
        let mut lists: Vec<Vec<c_int>> = adjacency
            .iter()
            .map(|list| {
                list.iter()
                    .map(|&r| {
                        assert!(r < nr, "neighbour {r} out of range for {nr}");
                        to_c(r)
                    })
                    .collect()
            })
            .collect();
        let pointers = lists.iter_mut().map(Vec::as_mut_ptr).collect();
        let sizes = lists.iter().map(|list| to_c(list.len())).collect();
        Self {
            nl: to_c(adjacency.len()),
            nr: to_c(nr),
            lists,
            pointers,
            sizes,
        }
    }
}

/// Runs C's `matching_with_scratch` on the graph whose left vertex `l` is
/// adjacent to the right vertices `adjacency[l]` (so `nl` is
/// `adjacency.len()`), then reads `matching_witness` from the scratch space.
///
/// # Panics
/// If a neighbour is `>= nr`, or a size exceeds `c_int`.
#[must_use]
pub fn c_matching_with_scratch(
    nr: usize,
    adjacency: &[Vec<usize>],
    rng: Option<&mut CRandom>,
) -> CMatching {
    let mut graph = CGraph::new(nr, adjacency);
    let (nl, nr) = (graph.nl, graph.nr);
    // SAFETY: pure size computation on non-negative sizes.
    let bytes = unsafe { matching_scratch_size(nl, nr) };
    // `usize` storage: the scratch starts with a struct of pointers, so it
    // needs pointer alignment (C gets that from malloc).
    let mut scratch = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
    let mut outl = vec![0; adjacency.len()];
    let mut outr = vec![0; from_c(nr)];
    let rs = rng.map_or(ptr::null_mut(), CRandom::as_mut_ptr);
    // SAFETY: `scratch` is pointer-aligned and at least
    // `matching_scratch_size(nl, nr)` bytes, as C requires; `pointers[l]`
    // points at `sizes[l]` valid `c_int`s in `lists[l]` (whose buffers don't
    // move while `graph` is borrowed here), every entry is in `0..nr`, and C
    // only permutes within each list; `outl`/`outr` hold `nl`/`nr` ints; the
    // rng, if any, is live and exclusively borrowed.
    let count = unsafe {
        matching_with_scratch(
            scratch.as_mut_ptr().cast(),
            nl,
            nr,
            graph.pointers.as_mut_ptr(),
            graph.sizes.as_mut_ptr(),
            rs,
            outl.as_mut_ptr(),
            outr.as_mut_ptr(),
        )
    };
    let mut witness = vec![0; outl.len() + outr.len()];
    // SAFETY: the scratch was just filled by `matching_with_scratch` for the
    // same `nl` and `nr`; `witness` holds `nl + nr` ints.
    unsafe { matching_witness(scratch.as_mut_ptr().cast(), nl, nr, witness.as_mut_ptr()) };
    let (wl, wr) = witness.split_at(outl.len());
    CMatching {
        count: from_c(count),
        left_to_right: partners(&outl),
        right_to_left: partners(&outr),
        witness: (labels(wl), labels(wr)),
    }
}

/// Runs C's one-off `matching()` (which allocates its own scratch space),
/// returning the count and the two partner arrays.
///
/// # Panics
/// As [`c_matching_with_scratch`], or if C reports an allocation failure.
#[must_use]
pub fn c_matching(
    nr: usize,
    adjacency: &[Vec<usize>],
    rng: Option<&mut CRandom>,
) -> (usize, Vec<Option<usize>>, Vec<Option<usize>>) {
    let mut graph = CGraph::new(nr, adjacency);
    let mut outl = vec![0; adjacency.len()];
    let mut outr = vec![0; nr];
    let rs = rng.map_or(ptr::null_mut(), CRandom::as_mut_ptr);
    // SAFETY: as in `c_matching_with_scratch`, minus the scratch space,
    // which `matching` allocates and frees itself.
    let count = unsafe {
        matching(
            graph.nl,
            graph.nr,
            graph.pointers.as_mut_ptr(),
            graph.sizes.as_mut_ptr(),
            rs,
            outl.as_mut_ptr(),
            outr.as_mut_ptr(),
        )
    };
    assert!(count >= 0, "C matching() failed to allocate");
    (from_c(count), partners(&outl), partners(&outr))
}

/// C's `-1`-for-none partner array as `Option`s.
fn partners(out: &[c_int]) -> Vec<Option<usize>> {
    out.iter().map(|&v| usize::try_from(v).ok()).collect()
}

/// C's 0/1 witness labels as `bool`s.
fn labels(witness: &[c_int]) -> Vec<bool> {
    witness
        .iter()
        .map(|&w| {
            assert!(w == 0 || w == 1, "witness label {w}");
            w == 1
        })
        .collect()
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("exceeds c_int::MAX")
}

fn from_c(n: c_int) -> usize {
    usize::try_from(n).expect("C returned a negative value")
}
