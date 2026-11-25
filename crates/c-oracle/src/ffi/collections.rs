//! Bindings to the C code replaced by crates or small ports: `combi.c`
//! (combinations), `sort.c` (`arraysort`) and `tree234.c` (counted
//! 2-3-4 trees).

use std::cmp::Ordering;
use std::ffi::{c_int, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;

/// C's `combi_ctx` (public in `puzzles.h`).
#[repr(C)]
struct CombiC {
    r: c_int,
    n: c_int,
    nleft: c_int,
    total: c_int,
    a: *mut c_int,
}

/// Opaque C `tree234`.
#[repr(C)]
struct TreeC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

type Cmp234 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type ArraySortCmp = unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void) -> c_int;

unsafe extern "C" {
    fn new_combi(r: c_int, n: c_int) -> *mut CombiC;
    fn next_combi(combi: *mut CombiC) -> *mut CombiC;
    fn free_combi(combi: *mut CombiC);

    fn arraysort_fn(
        array: *mut c_void,
        nmemb: usize,
        size: usize,
        cmp: ArraySortCmp,
        ctx: *mut c_void,
    );

    fn newtree234(cmp: Cmp234) -> *mut TreeC;
    fn freetree234(t: *mut TreeC);
    fn add234(t: *mut TreeC, e: *mut c_void) -> *mut c_void;
    fn del234(t: *mut TreeC, e: *mut c_void) -> *mut c_void;
    fn delpos234(t: *mut TreeC, index: c_int) -> *mut c_void;
    fn index234(t: *mut TreeC, index: c_int) -> *mut c_void;
    fn count234(t: *mut TreeC) -> c_int;
    fn findrelpos234(
        t: *mut TreeC,
        e: *mut c_void,
        cmp: Option<Cmp234>,
        relation: c_int,
        index: *mut c_int,
    ) -> *mut c_void;
}

/// Every combination C's `combi.c` yields for `r` of `n`, in its order.
///
/// # Panics
/// If `r > n` or `n == 0` (C asserts), or `n > 12` (C's `long` arithmetic
/// for the count overflows on Windows beyond that).
#[must_use]
pub fn c_combinations(r: usize, n: usize) -> Vec<Vec<usize>> {
    assert!(
        r <= n && (1..=12).contains(&n),
        "r {r} of n {n} out of range"
    );
    let (cr, cn) = (to_c(r), to_c(n));
    // SAFETY: C's asserts hold (r <= n, n >= 1) and the count fits a
    // 32-bit long. Returns a fresh context (smalloc never returns NULL).
    let combi = unsafe { new_combi(cr, cn) };
    let combi = NonNull::new(combi).expect("new_combi returned NULL");
    let mut all = Vec::new();
    // SAFETY: `combi` is live until freed below.
    while !unsafe { next_combi(combi.as_ptr()) }.is_null() {
        if r == 0 {
            // `a` is a zero-length allocation, which may be NULL.
            all.push(Vec::new());
            continue;
        }
        // SAFETY: `a` holds `r` ints, valid while the context lives.
        let a = unsafe { std::slice::from_raw_parts((*combi.as_ptr()).a, r) };
        all.push(a.iter().map(|&i| from_c(i)).collect());
    }
    // SAFETY: allocated by new_combi, freed once.
    unsafe { free_combi(combi.as_ptr()) };
    all
}

/// C's `arraysort` on `u32`s, comparing with `key(element)`.
pub fn c_arraysort_by_key(items: &mut [u32], key: fn(u32) -> u32) {
    unsafe extern "C" fn cmp(a: *const c_void, b: *const c_void, ctx: *mut c_void) -> c_int {
        // SAFETY: C passes pointers to two elements of the `u32` array and
        // the ctx we gave it, a `*const fn(u32) -> u32`.
        let (a, b, key) = unsafe {
            (
                *a.cast::<u32>(),
                *b.cast::<u32>(),
                *ctx.cast::<fn(u32) -> u32>(),
            )
        };
        match key(a).cmp(&key(b)) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }
    let mut key = key;
    // SAFETY: `items` is valid for its length; C only swaps elements
    // within it; `ctx` points at `key` for the whole call.
    unsafe {
        arraysort_fn(
            items.as_mut_ptr().cast(),
            items.len(),
            size_of::<u32>(),
            cmp,
            (&raw mut key).cast(),
        );
    }
}

/// An element of a [`CTree`]: ordered by `key` only, so a `tag` shows
/// which of two equal elements a tree kept. Stored in the C tree as the
/// pointer value `(key + 1) << 8 | tag` (never NULL, never dereferenced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeItem {
    /// What the tree's comparator orders by.
    pub key: u32,
    /// Payload the comparator ignores.
    pub tag: u8,
}

impl TreeItem {
    fn to_ptr(self) -> *mut c_void {
        let bits = (usize::try_from(self.key).expect("key fits") + 1) << 8 | usize::from(self.tag);
        std::ptr::without_provenance_mut(bits)
    }

    fn from_ptr(p: *mut c_void) -> Option<Self> {
        let bits = p.addr();
        (bits != 0).then(|| Self {
            key: u32::try_from((bits >> 8) - 1).expect("key fits"),
            tag: (bits & 0xff) as u8,
        })
    }
}

unsafe extern "C" fn tree_cmp(a: *mut c_void, b: *mut c_void) -> c_int {
    let key = |p: *mut c_void| TreeItem::from_ptr(p).expect("non-NULL element").key;
    match key(a).cmp(&key(b)) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// An owned C `tree234` of [`TreeItem`]s.
pub struct CTree {
    tree: NonNull<TreeC>,
}

impl Default for CTree {
    fn default() -> Self {
        Self::new()
    }
}

impl CTree {
    /// C's `newtree234` with a comparator on `key`.
    ///
    /// # Panics
    /// If C returns NULL (smalloc aborts first).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: the comparator is a valid function for the tree's life.
        let tree = unsafe { newtree234(tree_cmp) };
        Self {
            tree: NonNull::new(tree).expect("newtree234 returned NULL"),
        }
    }

    /// C's `add234`: the element now in the tree with that key (the new
    /// one, or the one already there).
    ///
    /// # Panics
    /// If C returns NULL (it never does for a non-NULL element).
    pub fn add(&mut self, item: TreeItem) -> TreeItem {
        // SAFETY: the tree is live; the element is a non-NULL value.
        let p = unsafe { add234(self.tree.as_ptr(), item.to_ptr()) };
        TreeItem::from_ptr(p).expect("add234 returned NULL")
    }

    /// C's `del234`: the removed element, if one had that key.
    pub fn del(&mut self, key: u32) -> Option<TreeItem> {
        let probe = TreeItem { key, tag: 0 };
        // SAFETY: the tree is live; the probe is only compared.
        TreeItem::from_ptr(unsafe { del234(self.tree.as_ptr(), probe.to_ptr()) })
    }

    /// C's `delpos234` (NULL out of range).
    pub fn del_pos(&mut self, index: usize) -> Option<TreeItem> {
        // SAFETY: the tree is live; C checks the index.
        TreeItem::from_ptr(unsafe { delpos234(self.tree.as_ptr(), to_c(index)) })
    }

    /// C's `index234` (NULL out of range).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<TreeItem> {
        // SAFETY: the tree is live; C checks the index.
        TreeItem::from_ptr(unsafe { index234(self.tree.as_ptr(), to_c(index)) })
    }

    /// C's `count234`.
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: the tree is live.
        from_c(unsafe { count234(self.tree.as_ptr()) })
    }

    /// True if the tree is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// C's `findrelpos234(t, key, NULL, relation, &index)` with
    /// `REL234_EQ` = 0, `LT` = 1, `LE` = 2, `GT` = 3, `GE` = 4: the element
    /// and its position.
    ///
    /// # Panics
    /// If `relation` exceeds 4.
    #[must_use]
    pub fn find_rel(&self, key: u32, relation: u8) -> Option<(TreeItem, usize)> {
        assert!(relation <= 4, "unknown relation");
        let probe = TreeItem { key, tag: 0 };
        let mut index: c_int = -1;
        // SAFETY: the tree is live; the probe is only compared; `index` is
        // a valid place for C to write.
        let p = unsafe {
            findrelpos234(
                self.tree.as_ptr(),
                probe.to_ptr(),
                None,
                c_int::from(relation),
                &raw mut index,
            )
        };
        TreeItem::from_ptr(p).map(|item| (item, from_c(index)))
    }
}

impl Drop for CTree {
    fn drop(&mut self) {
        // SAFETY: the tree is owned by `self` and freed once; its elements
        // are plain values, not allocations.
        unsafe { freetree234(self.tree.as_ptr()) }
    }
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("exceeds c_int::MAX")
}

fn from_c(n: c_int) -> usize {
    usize::try_from(n).expect("negative")
}
