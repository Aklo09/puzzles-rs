//! Bindings to `tdq.c`: the de-duplicating to-do queue.

use std::ffi::c_int;
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;

/// Opaque C `tdq` (defined privately in `tdq.c`).
#[repr(C)]
struct TdqC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    fn tdq_new(n: c_int) -> *mut TdqC;
    fn tdq_free(tdq: *mut TdqC);
    fn tdq_add(tdq: *mut TdqC, k: c_int);
    fn tdq_remove(tdq: *mut TdqC) -> c_int;
    fn tdq_fill(tdq: *mut TdqC);
}

/// An owned C `tdq`, mirroring the C API.
pub struct CTdq {
    tdq: NonNull<TdqC>,
    len: usize,
}

impl CTdq {
    /// C's `tdq_new(n)`.
    ///
    /// # Panics
    /// If `n` is 0 or exceeds `c_int::MAX`.
    #[must_use]
    pub fn new(n: usize) -> Self {
        // C allocates zero-length arrays as NULL and then indexes them in
        // tdq_remove, so a 0-capacity queue isn't safe to use.
        assert!(n > 0, "C tdq needs at least one element");
        let size = c_int::try_from(n).expect("size exceeds c_int::MAX");
        // SAFETY: `size` is positive; `tdq_new` returns a fresh allocation
        // (smalloc aborts instead of returning NULL).
        let tdq = unsafe { tdq_new(size) };
        Self {
            tdq: NonNull::new(tdq).expect("tdq_new returned NULL"),
            len: n,
        }
    }

    /// C's `tdq_add`.
    ///
    /// # Panics
    /// If `k` is out of range (C asserts, which would abort the tests).
    pub fn add(&mut self, k: usize) {
        assert!(k < self.len, "tdq_add: {k} out of range for {}", self.len);
        let k = c_int::try_from(k).expect("in range, so fits c_int");
        // SAFETY: `self.tdq` is live and owned by `self`; `k` is in range.
        unsafe { tdq_add(self.tdq.as_ptr(), k) }
    }

    /// C's `tdq_remove`, with its `-1` "empty" result mapped to `None`.
    pub fn remove(&mut self) -> Option<usize> {
        // SAFETY: `self.tdq` is live and owned by `self`.
        let k = unsafe { tdq_remove(self.tdq.as_ptr()) };
        usize::try_from(k).ok()
    }

    /// C's `tdq_fill`.
    pub fn fill(&mut self) {
        // SAFETY: `self.tdq` is live and owned by `self`.
        unsafe { tdq_fill(self.tdq.as_ptr()) }
    }
}

impl Drop for CTdq {
    fn drop(&mut self) {
        // SAFETY: `self.tdq` came from `tdq_new`, is owned by `self` and is
        // freed exactly once, here.
        unsafe { tdq_free(self.tdq.as_ptr()) }
    }
}
