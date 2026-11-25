//! Bindings to `latin.c`: the Latin square solver, generator and check.
//!
//! The oracle compiles `latin.c` without `STANDALONE_SOLVER`, so the C
//! `struct latin_solver` has no `names` field and the strategy functions
//! take no format arguments.

use std::ffi::{c_int, c_void};
use std::ptr::{self, NonNull};
use std::slice;

use super::random::RandomStateC;
use super::{CRandom, sfree};

/// C's `struct latin_solver` (non-`STANDALONE_SOLVER` layout).
#[repr(C)]
pub struct LatinSolverC {
    o: c_int,
    cube: *mut u8,
    grid: *mut u8,
    row: *mut u8,
    col: *mut u8,
}

/// Opaque C `struct latin_solver_scratch`.
#[repr(C)]
struct ScratchC {
    _data: [u8; 0],
    _marker: std::marker::PhantomData<(*mut u8, std::marker::PhantomPinned)>,
}

type UserSolver = unsafe extern "C" fn(*mut LatinSolverC, *mut c_void) -> c_int;
type Validator = unsafe extern "C" fn(*mut LatinSolverC, *mut c_void) -> bool;
type CtxNew = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type CtxFree = unsafe extern "C" fn(*mut c_void);

unsafe extern "C" {
    fn latin_solver_alloc(solver: *mut LatinSolverC, grid: *mut u8, o: c_int) -> bool;
    fn latin_solver_free(solver: *mut LatinSolverC);
    fn latin_solver_place(solver: *mut LatinSolverC, x: c_int, y: c_int, n: c_int);
    fn latin_solver_elim(solver: *mut LatinSolverC, start: c_int, step: c_int) -> c_int;
    fn latin_solver_new_scratch(solver: *mut LatinSolverC) -> *mut ScratchC;
    fn latin_solver_free_scratch(scratch: *mut ScratchC);
    fn latin_solver_set(
        solver: *mut LatinSolverC,
        scratch: *mut ScratchC,
        start: c_int,
        step1: c_int,
        step2: c_int,
    ) -> c_int;
    fn latin_solver_forcing(solver: *mut LatinSolverC, scratch: *mut ScratchC) -> c_int;
    fn latin_solver_diff_simple(solver: *mut LatinSolverC) -> c_int;
    fn latin_solver_diff_set(
        solver: *mut LatinSolverC,
        scratch: *mut ScratchC,
        extreme: bool,
    ) -> c_int;
    #[allow(clippy::too_many_arguments, reason = "mirrors the C signature")]
    fn latin_solver_main(
        solver: *mut LatinSolverC,
        maxdiff: c_int,
        diff_simple: c_int,
        diff_set_0: c_int,
        diff_set_1: c_int,
        diff_forcing: c_int,
        diff_recursive: c_int,
        usersolvers: *const Option<UserSolver>,
        valid: Option<Validator>,
        ctx: *mut c_void,
        ctxnew: Option<CtxNew>,
        ctxfree: Option<CtxFree>,
    ) -> c_int;
    fn latin_generate(o: c_int, rs: *mut RandomStateC) -> *mut u8;
    fn latin_generate_rect(w: c_int, h: c_int, rs: *mut RandomStateC) -> *mut u8;
    fn latin_check(sq: *mut u8, order: c_int) -> bool;
}

/// C's `diff_impossible`, `diff_ambiguous` and `diff_unfinished`.
pub const C_DIFF_IMPOSSIBLE: i32 = 10;
/// See [`C_DIFF_IMPOSSIBLE`].
pub const C_DIFF_AMBIGUOUS: i32 = 11;
/// See [`C_DIFF_IMPOSSIBLE`].
pub const C_DIFF_UNFINISHED: i32 = 12;

/// The most difficulty levels a test can use (one trampoline per level).
pub const C_MAX_LEVELS: usize = 8;

/// The difficulty levels passed to `latin_solver_main`.
#[derive(Clone, Copy, Debug)]
pub struct CLevels {
    /// `diff_simple`.
    pub simple: usize,
    /// `diff_set_0`.
    pub set: usize,
    /// `diff_set_1`.
    pub extreme_set: usize,
    /// `diff_forcing`.
    pub forcing: usize,
    /// `diff_recursive`.
    pub recursive: usize,
}

/// A C solver's state as seen from game rules: C's `struct latin_solver`
/// borrowed for the duration of a callback.
pub struct CLatinView<'a> {
    solver: &'a mut LatinSolverC,
}

impl CLatinView<'_> {
    /// The order `o`.
    #[must_use]
    pub fn order(&self) -> usize {
        from_c(self.solver.o)
    }

    /// C's `cube(x, y, n)`.
    #[must_use]
    pub fn is_possible(&self, x: usize, y: usize, n: usize) -> bool {
        let pos = self.cube_pos(x, y, n);
        // SAFETY: the C solver's cube holds o^3 bytes and `pos` is in range
        // (checked by `cube_pos`).
        unsafe { *self.solver.cube.add(pos) != 0 }
    }

    /// `cube(x, y, n) = false`.
    pub fn rule_out(&mut self, x: usize, y: usize, n: usize) {
        let pos = self.cube_pos(x, y, n);
        // SAFETY: as in `is_possible`; the view borrows the solver
        // exclusively.
        unsafe { *self.solver.cube.add(pos) = 0 }
    }

    /// `solver->grid[y * o + x]`.
    ///
    /// # Panics
    /// If `(x, y)` is out of range.
    #[must_use]
    pub fn digit(&self, x: usize, y: usize) -> u8 {
        let o = self.order();
        assert!(x < o && y < o, "cell out of range");
        // SAFETY: the grid holds o^2 bytes and the index is in range.
        unsafe { *self.solver.grid.add(y * o + x) }
    }

    /// C's `latin_solver_place`.
    ///
    /// # Panics
    /// If `n` isn't a candidate at `(x, y)` (C asserts).
    pub fn place(&mut self, x: usize, y: usize, n: usize) {
        assert!(self.is_possible(x, y, n), "not a candidate");
        // SAFETY: the solver is live and exclusively borrowed; the
        // arguments are in range and `n` is a candidate, so C's asserts hold.
        unsafe { latin_solver_place(self.solver, to_c(x), to_c(y), to_c(n)) }
    }

    fn cube_pos(&self, x: usize, y: usize, n: usize) -> usize {
        let o = self.order();
        assert!(
            x < o && y < o && (1..=o).contains(&n),
            "cube index out of range"
        );
        (x * o + y) * o + n - 1
    }
}

/// Game rules on the C side: what the `usersolvers`, `valid` and
/// `ctxnew`/`ctxfree` callbacks do, written in Rust. Implementations must
/// not panic (a panic can't unwind through C, so it aborts).
pub trait CLatinRules {
    /// `usersolvers[level]`: a C-style return code (<0, 0, >0).
    fn deduce(&mut self, solver: &mut CLatinView<'_>, level: usize) -> i32;
    /// `valid`.
    fn is_valid(&mut self, solver: &mut CLatinView<'_>) -> bool;
    /// `ctxnew`: a context for one guess (called only when branching).
    fn branch(&self) -> Box<dyn CLatinRules>;
}

/// The `void *ctx` handed to C: a thin pointer to the boxed rules.
type Ctx = Box<dyn CLatinRules>;

/// Recovers the rules and a view from the C callback arguments.
///
/// # Safety
/// `solver` must be a live C solver and `ctx` a live `*mut Ctx` from
/// [`CLatinSolver::solve`] or [`ctx_new`], neither aliased for the call.
unsafe fn callback_args<'a>(
    solver: *mut LatinSolverC,
    ctx: *mut c_void,
) -> (&'a mut Ctx, CLatinView<'a>) {
    // SAFETY: guaranteed by the caller.
    unsafe {
        (
            &mut *ctx.cast::<Ctx>(),
            CLatinView {
                solver: &mut *solver,
            },
        )
    }
}

/// `usersolvers[LEVEL]`.
unsafe extern "C" fn deduce_at<const LEVEL: usize>(
    solver: *mut LatinSolverC,
    ctx: *mut c_void,
) -> c_int {
    // SAFETY: C passes the solver it is running and the ctx we gave it.
    let (rules, mut view) = unsafe { callback_args(solver, ctx) };
    rules.deduce(&mut view, LEVEL)
}

unsafe extern "C" fn validate(solver: *mut LatinSolverC, ctx: *mut c_void) -> bool {
    // SAFETY: as in `deduce_at`.
    let (rules, mut view) = unsafe { callback_args(solver, ctx) };
    rules.is_valid(&mut view)
}

unsafe extern "C" fn ctx_new(ctx: *mut c_void) -> *mut c_void {
    // SAFETY: C passes a ctx we gave it (the original or a ctx_new result).
    let rules = unsafe { &*ctx.cast::<Ctx>() };
    Box::into_raw(Box::new(rules.branch())).cast()
}

unsafe extern "C" fn ctx_free(ctx: *mut c_void) {
    // SAFETY: C frees each ctx_new result exactly once.
    drop(unsafe { Box::from_raw(ctx.cast::<Ctx>()) });
}

const USER_SOLVERS: [Option<UserSolver>; C_MAX_LEVELS] = [
    Some(deduce_at::<0>),
    Some(deduce_at::<1>),
    Some(deduce_at::<2>),
    Some(deduce_at::<3>),
    Some(deduce_at::<4>),
    Some(deduce_at::<5>),
    Some(deduce_at::<6>),
    Some(deduce_at::<7>),
];

/// An owned C `struct latin_solver` with its grid. Calls that C would
/// reject with an assertion panic here first.
pub struct CLatinSolver {
    solver: LatinSolverC,
    /// The grid C writes straight back to; boxed so the pointer is stable.
    grid: Box<[u8]>,
    order: usize,
    consistent: bool,
}

impl CLatinSolver {
    /// C's `latin_solver_alloc(solver, grid, o)`.
    ///
    /// # Panics
    /// If `grid` isn't `order * order` long, `order` is 0 (C would allocate
    /// nothing) or a digit exceeds `order` (C's behaviour is undefined).
    #[must_use]
    pub fn new(grid: &[u8], order: usize) -> Self {
        assert!(order > 0, "C latin solver needs a positive order");
        assert_eq!(grid.len(), order * order, "grid size");
        assert!(
            grid.iter().all(|&d| usize::from(d) <= order),
            "digit out of range"
        );
        assert!(
            c_int::try_from(order * order * order).is_ok(),
            "order too large"
        );
        let mut this = Self {
            solver: LatinSolverC {
                o: 0,
                cube: ptr::null_mut(),
                grid: ptr::null_mut(),
                row: ptr::null_mut(),
                col: ptr::null_mut(),
            },
            grid: grid.into(),
            order,
            consistent: false,
        };
        // SAFETY: `solver` is a valid place for C to fill in; `grid` holds
        // o^2 digits, each at most `o`, and stays at a stable address for
        // the life of `this` (C keeps the pointer).
        this.consistent = unsafe {
            latin_solver_alloc(&raw mut this.solver, this.grid.as_mut_ptr(), to_c(order))
        };
        this
    }

    /// `latin_solver_alloc`'s return value.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.consistent
    }

    /// The grid as C left it.
    #[must_use]
    pub fn grid(&self) -> &[u8] {
        &self.grid
    }

    /// The candidate cube, as bools.
    #[must_use]
    pub fn cube(&self) -> Vec<bool> {
        let len = self.order.pow(3);
        // SAFETY: C allocated o^3 bytes for the cube; nothing writes to it
        // while `self` is borrowed.
        unsafe { slice::from_raw_parts(self.solver.cube, len) }
            .iter()
            .map(|&b| b != 0)
            .collect()
    }

    /// `row` and `col` (placed-digit flags), as bools.
    #[must_use]
    pub fn rows_cols(&self) -> (Vec<bool>, Vec<bool>) {
        let len = self.order.pow(2);
        // SAFETY: C allocated o^2 bytes for each.
        let (row, col) = unsafe {
            (
                slice::from_raw_parts(self.solver.row, len),
                slice::from_raw_parts(self.solver.col, len),
            )
        };
        (
            row.iter().map(|&b| b != 0).collect(),
            col.iter().map(|&b| b != 0).collect(),
        )
    }

    /// A view for direct changes (C's `cube(x, y, n) = false` etc.).
    pub fn view(&mut self) -> CLatinView<'_> {
        CLatinView {
            solver: &mut self.solver,
        }
    }

    /// C's `latin_solver_elim(solver, start, step)`.
    ///
    /// # Panics
    /// If the line leaves the cube.
    pub fn elim(&mut self, start: usize, step: usize) -> i32 {
        self.check_slice(start, step, 0);
        // SAFETY: the solver is live and the line stays inside the cube.
        unsafe { latin_solver_elim(&raw mut self.solver, to_c(start), to_c(step)) }
    }

    /// C's `latin_solver_set(solver, scratch, start, step1, step2)`.
    ///
    /// # Panics
    /// If the slice leaves the cube, or two rows with one candidate share
    /// its column (C asserts).
    pub fn set(&mut self, start: usize, step1: usize, step2: usize) -> i32 {
        self.check_slice(start, step1, step2);
        assert!(
            self.set_would_succeed(start, step1, step2),
            "latin_solver_set would assert"
        );
        self.with_scratch(|solver, scratch| {
            // SAFETY: the solver and scratch are live; the slice is inside
            // the cube and C's row/column assert holds.
            unsafe { latin_solver_set(solver, scratch, to_c(start), to_c(step1), to_c(step2)) }
        })
    }

    /// Whether `latin_solver_set` passes its assert on this slice: rows
    /// with a single candidate have distinct columns (or a row has none,
    /// which returns early).
    #[must_use]
    pub fn set_would_succeed(&self, start: usize, step1: usize, step2: usize) -> bool {
        let o = self.order;
        let cube = self.cube();
        let mut used = vec![false; o];
        for i in 0..o {
            let cols: Vec<usize> = (0..o)
                .filter(|&j| cube[start + i * step1 + j * step2])
                .collect();
            match cols[..] {
                [] => return true,
                [j] if used[j] => return false,
                [j] => used[j] = true,
                _ => {}
            }
        }
        true
    }

    /// C's `latin_solver_forcing`.
    pub fn forcing(&mut self) -> i32 {
        // SAFETY: the solver and scratch are live.
        self.with_scratch(|solver, scratch| unsafe { latin_solver_forcing(solver, scratch) })
    }

    /// C's `latin_solver_diff_simple`.
    pub fn diff_simple(&mut self) -> i32 {
        // SAFETY: the solver is live.
        unsafe { latin_solver_diff_simple(&raw mut self.solver) }
    }

    /// C's `latin_solver_diff_set`.
    ///
    /// # Panics
    /// If one of its slices would make `latin_solver_set` assert.
    pub fn diff_set(&mut self, extreme: bool) -> i32 {
        let o = self.order;
        let slices: Vec<(usize, usize, usize)> = if extreme {
            (0..o).map(|n| (n, o * o, o)).collect()
        } else {
            (0..o)
                .map(|y| (y * o, o * o, 1))
                .chain((0..o).map(|x| (x * o * o, o, 1)))
                .collect()
        };
        assert!(
            slices
                .iter()
                .all(|&(s, a, b)| self.set_would_succeed(s, a, b)),
            "latin_solver_set would assert"
        );
        // SAFETY: the solver and scratch are live; no slice asserts. (The
        // check above covers the whole pass only up to the first change,
        // which is where C stops.)
        self.with_scratch(|solver, scratch| unsafe {
            latin_solver_diff_set(solver, scratch, extreme)
        })
    }

    /// C's `latin_solver_main` with `rules` behind the callbacks: the
    /// difficulty or a `C_DIFF_*` code. With `branching`, `ctxnew` and
    /// `ctxfree` are passed, so each guess gets `rules.branch()`.
    ///
    /// # Panics
    /// If the solver is inconsistent (C callers never run it then),
    /// `max_diff > levels.recursive` or `levels.recursive` is
    /// `C_MAX_LEVELS` or more.
    pub fn solve(
        &mut self,
        max_diff: usize,
        levels: CLevels,
        rules: Box<dyn CLatinRules>,
        branching: bool,
    ) -> i32 {
        assert!(self.consistent, "solving an inconsistent solver");
        assert!(max_diff <= levels.recursive, "max_diff above recursive");
        assert!(levels.recursive < C_MAX_LEVELS, "too many levels");
        let mut ctx: Ctx = rules;
        let (ctxnew, ctxfree): (Option<CtxNew>, Option<CtxFree>) = if branching {
            (Some(ctx_new), Some(ctx_free))
        } else {
            (None, None)
        };
        // SAFETY: the solver is live and consistent; `USER_SOLVERS` has an
        // entry for every level up to `max_diff`; `ctx` points at a live
        // `Ctx` for the whole call, which the callbacks expect; C's
        // `maxdiff <= diff_recursive` assert holds.
        unsafe {
            latin_solver_main(
                &raw mut self.solver,
                to_c(max_diff),
                to_c(levels.simple),
                to_c(levels.set),
                to_c(levels.extreme_set),
                to_c(levels.forcing),
                to_c(levels.recursive),
                USER_SOLVERS.as_ptr(),
                Some(validate),
                (&raw mut ctx).cast(),
                ctxnew,
                ctxfree,
            )
        }
    }

    fn with_scratch(&mut self, f: impl FnOnce(*mut LatinSolverC, *mut ScratchC) -> c_int) -> i32 {
        // SAFETY: the solver is live; the scratch is freed below.
        let scratch = unsafe { latin_solver_new_scratch(&raw mut self.solver) };
        let scratch = NonNull::new(scratch).expect("scratch allocation failed");
        let ret = f(&raw mut self.solver, scratch.as_ptr());
        // SAFETY: allocated above, freed once.
        unsafe { latin_solver_free_scratch(scratch.as_ptr()) };
        ret
    }

    fn check_slice(&self, start: usize, step1: usize, step2: usize) {
        let last = self.order - 1;
        assert!(
            start + last * step1 + last * step2 < self.order.pow(3),
            "slice leaves the cube"
        );
    }
}

impl Drop for CLatinSolver {
    fn drop(&mut self) {
        // SAFETY: `latin_solver_alloc` allocated cube, row and col (it does
        // even when it returns false); freed once. The grid is ours.
        unsafe { latin_solver_free(&raw mut self.solver) }
    }
}

/// C's `latin_generate(order, rng)`.
///
/// # Panics
/// If `order` is 0 (C would allocate nothing) or above 255.
#[must_use]
pub fn c_latin_generate(order: usize, rng: &mut CRandom) -> Vec<u8> {
    assert!((1..=255).contains(&order), "order out of range");
    // SAFETY: the order is in range; the rng is live and exclusively
    // borrowed. Returns a fresh o^2 array.
    let sq = unsafe { latin_generate(to_c(order), rng.as_mut_ptr()) };
    take_digits(sq, order * order)
}

/// C's `latin_generate_rect(w, h, rng)`.
///
/// # Panics
/// If `w` or `h` is 0 or `max(w, h)` exceeds 255.
#[must_use]
pub fn c_latin_generate_rect(w: usize, h: usize, rng: &mut CRandom) -> Vec<u8> {
    assert!(w > 0 && h > 0 && w.max(h) <= 255, "size out of range");
    // SAFETY: the sizes are in range; the rng is live and exclusively
    // borrowed. Returns a fresh w * h array.
    let sq = unsafe { latin_generate_rect(to_c(w), to_c(h), rng.as_mut_ptr()) };
    take_digits(sq, w * h)
}

/// C's `latin_check(square, order)`: true if it is *not* a Latin square
/// (by symbol counts).
///
/// # Panics
/// If `square` is shorter than `order * order`.
#[must_use]
pub fn c_latin_check(square: &[u8], order: usize) -> bool {
    assert!(square.len() >= order * order, "square too short");
    let mut copy = square.to_vec();
    // SAFETY: `copy` holds at least order^2 digits; C only reads them.
    unsafe { latin_check(copy.as_mut_ptr(), to_c(order)) }
}

fn take_digits(p: *mut u8, len: usize) -> Vec<u8> {
    assert!(!p.is_null(), "C returned NULL");
    // SAFETY: `p` points at `len` digits from smalloc, freed below.
    let digits = unsafe { slice::from_raw_parts(p, len) }.to_vec();
    // SAFETY: allocated by C, freed once, not used afterwards.
    unsafe { sfree(p.cast()) };
    digits
}

fn to_c(n: usize) -> c_int {
    c_int::try_from(n).expect("exceeds c_int::MAX")
}

fn from_c(n: c_int) -> usize {
    usize::try_from(n).expect("negative")
}
