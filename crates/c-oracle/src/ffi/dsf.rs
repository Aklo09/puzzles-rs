//! Bindings to `dsf.c`: disjoint-set forests in all three flavours.

use std::ffi::c_int;
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;

/// Opaque C `DSF` (defined privately in `dsf.c`).
#[repr(C)]
struct DsfC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    fn dsf_new(size: c_int) -> *mut DsfC;
    fn dsf_new_min(size: c_int) -> *mut DsfC;
    fn dsf_new_flip(size: c_int) -> *mut DsfC;
    fn dsf_free(dsf: *mut DsfC);
    fn dsf_reinit(dsf: *mut DsfC);
    fn dsf_copy(to: *mut DsfC, from: *mut DsfC);
    fn dsf_canonify(dsf: *mut DsfC, n: c_int) -> c_int;
    fn dsf_equivalent(dsf: *mut DsfC, n1: c_int, n2: c_int) -> bool;
    fn dsf_size(dsf: *mut DsfC, n: c_int) -> c_int;
    fn dsf_merge(dsf: *mut DsfC, n1: c_int, n2: c_int);
    fn dsf_minimal(dsf: *mut DsfC, n: c_int) -> c_int;
    fn dsf_canonify_flip(dsf: *mut DsfC, n: c_int, inverse: *mut bool) -> c_int;
    fn dsf_merge_flip(dsf: *mut DsfC, n1: c_int, n2: c_int, inverse: bool);
}

/// Which flavour of C `DSF` to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CDsfKind {
    /// `dsf_new`.
    Plain,
    /// `dsf_new_min`: also tracks each class's smallest element.
    Min,
    /// `dsf_new_flip`: also tracks flips between elements.
    Flip,
}

/// An owned C `DSF`, mirroring the C API one-to-one (one type, flavour
/// chosen at construction). Calls that C would reject with an assertion,
/// which would abort the whole test process, panic here first instead.
pub struct CDsf {
    dsf: NonNull<DsfC>,
    kind: CDsfKind,
    len: usize,
}

impl CDsf {
    /// Creates a C `DSF` of `len` elements.
    ///
    /// # Panics
    /// If `len` is 0 (C asserts a positive size) or exceeds `c_int::MAX`.
    #[must_use]
    pub fn new(kind: CDsfKind, len: usize) -> Self {
        assert!(len > 0, "C dsf needs at least one element");
        let size = to_c(len);
        // SAFETY: `size` is positive and in range; the constructors return
        // a fresh allocation (smalloc aborts instead of returning NULL).
        let dsf = unsafe {
            match kind {
                CDsfKind::Plain => dsf_new(size),
                CDsfKind::Min => dsf_new_min(size),
                CDsfKind::Flip => dsf_new_flip(size),
            }
        };
        Self {
            dsf: NonNull::new(dsf).expect("dsf_new returned NULL"),
            kind,
            len,
        }
    }

    /// C's `dsf_canonify`.
    ///
    /// # Panics
    /// If `n` is out of range.
    pub fn canonify(&mut self, n: usize) -> usize {
        let n = self.index(n);
        // SAFETY: `self.dsf` is live and owned by `self`; `n` is in range.
        from_c(unsafe { dsf_canonify(self.dsf.as_ptr(), n) })
    }

    /// C's `dsf_equivalent`.
    ///
    /// # Panics
    /// If `a` or `b` is out of range.
    pub fn equivalent(&mut self, a: usize, b: usize) -> bool {
        let (a, b) = (self.index(a), self.index(b));
        // SAFETY: `self.dsf` is live and owned by `self`; both in range.
        unsafe { dsf_equivalent(self.dsf.as_ptr(), a, b) }
    }

    /// C's `dsf_size`.
    ///
    /// # Panics
    /// If `n` is out of range.
    pub fn size(&mut self, n: usize) -> usize {
        let n = self.index(n);
        // SAFETY: `self.dsf` is live and owned by `self`; `n` is in range.
        from_c(unsafe { dsf_size(self.dsf.as_ptr(), n) })
    }

    /// C's `dsf_merge`.
    ///
    /// # Panics
    /// If `a` or `b` is out of range, or on a flip dsf (C asserts).
    pub fn merge(&mut self, a: usize, b: usize) {
        assert_ne!(self.kind, CDsfKind::Flip, "dsf_merge on a flip dsf");
        let (a, b) = (self.index(a), self.index(b));
        // SAFETY: `self.dsf` is live, owned by `self`, not a flip dsf; both
        // indices are in range.
        unsafe { dsf_merge(self.dsf.as_ptr(), a, b) }
    }

    /// C's `dsf_minimal`.
    ///
    /// # Panics
    /// If `n` is out of range, or the dsf isn't a min dsf (C asserts).
    pub fn minimal(&mut self, n: usize) -> usize {
        assert_eq!(self.kind, CDsfKind::Min, "dsf_minimal on a non-min dsf");
        let n = self.index(n);
        // SAFETY: `self.dsf` is a live min dsf owned by `self`; `n` is in
        // range.
        from_c(unsafe { dsf_minimal(self.dsf.as_ptr(), n) })
    }

    /// C's `dsf_canonify_flip`: the representative and whether `n` is
    /// flipped relative to it.
    ///
    /// # Panics
    /// If `n` is out of range, or the dsf isn't a flip dsf (C asserts).
    pub fn canonify_flip(&mut self, n: usize) -> (usize, bool) {
        assert_eq!(
            self.kind,
            CDsfKind::Flip,
            "dsf_canonify_flip on a non-flip dsf"
        );
        let n = self.index(n);
        let mut inverse = false;
        // SAFETY: `self.dsf` is a live flip dsf owned by `self`; `n` is in
        // range; `inverse` is a valid place for the C code to write a bool.
        let root = unsafe { dsf_canonify_flip(self.dsf.as_ptr(), n, &raw mut inverse) };
        (from_c(root), inverse)
    }

    /// C's `dsf_merge_flip`.
    ///
    /// # Panics
    /// If an index is out of range or the dsf isn't a flip dsf. Callers must
    /// also avoid contradictory merges: C asserts on those, which would
    /// abort the test process rather than panic.
    pub fn merge_flip(&mut self, a: usize, b: usize, inverse: bool) {
        assert_eq!(
            self.kind,
            CDsfKind::Flip,
            "dsf_merge_flip on a non-flip dsf"
        );
        let (a, b) = (self.index(a), self.index(b));
        // SAFETY: `self.dsf` is a live flip dsf owned by `self`; both
        // indices are in range; the caller guarantees consistency.
        unsafe { dsf_merge_flip(self.dsf.as_ptr(), a, b, inverse) }
    }

    /// C's `dsf_reinit`.
    pub fn reinit(&mut self) {
        // SAFETY: `self.dsf` is live and owned by `self`.
        unsafe { dsf_reinit(self.dsf.as_ptr()) }
    }

    /// C's `dsf_copy(self, from)`.
    ///
    /// # Panics
    /// If the two differ in size or flavour (C asserts).
    pub fn copy_from(&mut self, from: &Self) {
        assert_eq!(
            (self.kind, self.len),
            (from.kind, from.len),
            "dsf_copy mismatch"
        );
        // SAFETY: both are live dsfs of the same size and flavour; `from` is
        // only read, and they are distinct allocations (`&mut self` and
        // `&from` can't alias).
        unsafe { dsf_copy(self.dsf.as_ptr(), from.dsf.as_ptr()) }
    }

    fn index(&self, n: usize) -> c_int {
        assert!(
            n < self.len,
            "index {n} out of range for dsf of {}",
            self.len
        );
        to_c(n)
    }
}

impl Drop for CDsf {
    fn drop(&mut self) {
        // SAFETY: `self.dsf` came from a dsf constructor, is owned by `self`
        // and is freed exactly once, here.
        unsafe { dsf_free(self.dsf.as_ptr()) }
    }
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("index exceeds c_int::MAX")
}

fn from_c(n: c_int) -> usize {
    usize::try_from(n).expect("C returned a negative index")
}
