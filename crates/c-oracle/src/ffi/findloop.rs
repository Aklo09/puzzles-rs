//! Bindings to `findloop.c`: loop and bridge detection.

use std::ffi::{c_int, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;

/// Opaque C `struct findloopstate` array.
#[repr(C)]
struct FindLoopStateC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

/// C's `neighbour_fn_t`.
type NeighbourFn = unsafe extern "C" fn(vertex: c_int, ctx: *mut c_void) -> c_int;

unsafe extern "C" {
    fn findloop_new_state(nvertices: c_int) -> *mut FindLoopStateC;
    fn findloop_free_state(state: *mut FindLoopStateC);
    fn findloop_run(
        state: *mut FindLoopStateC,
        nvertices: c_int,
        neighbour: NeighbourFn,
        ctx: *mut c_void,
    ) -> bool;
    fn findloop_is_loop_edge(state: *mut FindLoopStateC, u: c_int, v: c_int) -> bool;
    fn findloop_is_bridge(
        state: *mut FindLoopStateC,
        u: c_int,
        v: c_int,
        u_vertices: *mut c_int,
        v_vertices: *mut c_int,
    ) -> bool;
}

/// Iteration state for the neighbour callback: C calls it with a vertex to
/// get that vertex's first neighbour, then with -1 for each next one, and
/// expects -1 when there are no more.
struct NeighbourCursor<'a> {
    adjacency: &'a [Vec<usize>],
    vertex: usize,
    next: usize,
}

/// The `neighbour_fn_t` handed to C. Must not panic: unwinding out of an
/// `extern "C"` function aborts the process.
unsafe extern "C" fn next_neighbour(vertex: c_int, ctx: *mut c_void) -> c_int {
    // SAFETY: `ctx` is the `&mut NeighbourCursor` that `CFindLoop::run`
    // passes to `findloop_run`, valid and unaliased for the whole call.
    let cursor = unsafe { &mut *ctx.cast::<NeighbourCursor<'_>>() };
    if let Ok(vertex) = usize::try_from(vertex) {
        cursor.vertex = vertex;
        cursor.next = 0;
    }
    match cursor
        .adjacency
        .get(cursor.vertex)
        .and_then(|list| list.get(cursor.next))
    {
        Some(&w) => {
            cursor.next += 1;
            c_int::try_from(w).unwrap_or(-1)
        }
        None => -1,
    }
}

/// C's findloop state after `findloop_run` on an adjacency list.
pub struct CFindLoop {
    state: NonNull<FindLoopStateC>,
    len: usize,
    has_loop: bool,
}

impl CFindLoop {
    /// Runs C's `findloop_run` on the graph whose neighbours of `v` are
    /// `adjacency[v]`.
    ///
    /// # Panics
    /// If the graph is empty, a neighbour is out of range, or a list repeats
    /// a vertex back to back: C hangs on that (it corrupts its visit list).
    #[must_use]
    pub fn run(adjacency: &[Vec<usize>]) -> Self {
        let len = adjacency.len();
        assert!(len > 0, "C findloop needs at least one vertex");
        for list in adjacency {
            assert!(list.iter().all(|&w| w < len), "neighbour out of range");
            assert!(
                list.windows(2).all(|pair| pair[0] != pair[1]),
                "repeated neighbour (C would hang)"
            );
        }
        let n = to_c(len);
        // SAFETY: `n` is positive; returns a fresh allocation of `n` states
        // (smalloc aborts instead of returning NULL).
        let state = unsafe { findloop_new_state(n) };
        let state = NonNull::new(state).expect("findloop_new_state returned NULL");
        let mut cursor = NeighbourCursor {
            adjacency,
            vertex: 0,
            next: 0,
        };
        // SAFETY: `state` holds `n` entries; the callback only reads
        // `adjacency` through `cursor`, which outlives the call and isn't
        // otherwise accessed during it; every neighbour is in range.
        let has_loop =
            unsafe { findloop_run(state.as_ptr(), n, next_neighbour, (&raw mut cursor).cast()) };
        Self {
            state,
            len,
            has_loop,
        }
    }

    /// The value `findloop_run` returned.
    #[must_use]
    pub fn has_loop(&self) -> bool {
        self.has_loop
    }

    /// C's `findloop_is_loop_edge`.
    ///
    /// # Panics
    /// If `u` or `v` is out of range.
    #[must_use]
    pub fn is_loop_edge(&self, u: usize, v: usize) -> bool {
        let (u, v) = (self.index(u), self.index(v));
        // SAFETY: the state is live and was filled by `findloop_run`; both
        // indices are in range.
        unsafe { findloop_is_loop_edge(self.state.as_ptr(), u, v) }
    }

    /// C's `findloop_is_bridge`, with the out-parameters returned.
    ///
    /// # Panics
    /// If `u` or `v` is out of range.
    #[must_use]
    pub fn is_bridge(&self, u: usize, v: usize) -> Option<(usize, usize)> {
        let (u, v) = (self.index(u), self.index(v));
        let (mut u_vertices, mut v_vertices) = (0, 0);
        // SAFETY: the state is live and was filled by `findloop_run`; both
        // indices are in range; the out-pointers are valid `c_int`s.
        let bridge = unsafe {
            findloop_is_bridge(
                self.state.as_ptr(),
                u,
                v,
                &raw mut u_vertices,
                &raw mut v_vertices,
            )
        };
        bridge.then(|| (from_c(u_vertices), from_c(v_vertices)))
    }

    fn index(&self, n: usize) -> c_int {
        assert!(n < self.len, "vertex {n} out of range for {}", self.len);
        to_c(n)
    }
}

impl Drop for CFindLoop {
    fn drop(&mut self) {
        // SAFETY: the state came from `findloop_new_state`, is owned by
        // `self` and is freed exactly once, here.
        unsafe { findloop_free_state(self.state.as_ptr()) }
    }
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("exceeds c_int::MAX")
}

fn from_c(n: c_int) -> usize {
    usize::try_from(n).expect("C returned a negative count")
}
