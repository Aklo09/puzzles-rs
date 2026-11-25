//! Port of `third_party/puzzles-c/latin.c` (upstream 616da16).
//!
//! Latin squares: an `o` × `o` grid holding the digits `1..=o` so that
//! each appears once in every row and every column. Keen, Towers and
//! Unequal build their solvers on [`LatinSolver`], adding their own clue
//! deductions through [`LatinRules`]; they and Singles generate their
//! grids with [`generate`] / [`generate_rect`].
//!
//! Which square the generator draws, which deductions the solver makes and
//! in what order, decide the puzzles and their difficulty ratings, so this
//! is a faithful port (D5 exception), checked against C by the oracle
//! tests. The solver's reasoning text (C's `STANDALONE_SOLVER` output) is
//! kept, recorded through a [`SolverLog`].
//!
//! Coordinates are 0-based `(x, y)`, digits 1-based; 0 in a grid means
//! empty. Messages print coordinates 1-based, as C does.

use std::fmt;

use crate::matching::Matcher;
use crate::random::RandomState;
use crate::solver_log::SolverLog;

/// One cell of a Latin square: `1..=o`, or 0 for empty (C's `digit`).
pub type Digit = u8;

/// What one deduction step achieved (C's `int` return codes: 0, +1, -1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deduction {
    /// Nothing learnt.
    None,
    /// At least one candidate was ruled out or a digit placed.
    Progress,
    /// The grid can't be completed.
    Contradiction,
}

impl From<usize> for Deduction {
    /// A count of changes made: 0 is [`Deduction::None`], anything else
    /// [`Deduction::Progress`] (game deductions that count, like C's
    /// `nchanged`).
    fn from(changes: usize) -> Self {
        if changes == 0 {
            Self::None
        } else {
            Self::Progress
        }
    }
}

/// How a solve ended (C's difficulty return value, or `diff_impossible`,
/// `diff_ambiguous`, `diff_unfinished`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Exactly one solution, found using deductions up to `difficulty`
    /// (the highest level that made progress, at least
    /// [`Levels::simple`]); [`Levels::recursive`] if guessing was needed.
    Solved {
        /// The game's difficulty level.
        difficulty: usize,
    },
    /// No solution, or the game's validator rejected the one found.
    #[doc(alias = "diff_impossible")]
    Impossible,
    /// More than one solution; the grid holds the first one found.
    #[doc(alias = "diff_ambiguous")]
    Ambiguous,
    /// The allowed deductions ran out before the grid was full.
    #[doc(alias = "diff_unfinished")]
    Unfinished,
}

/// The game difficulty level at which each built-in deduction becomes
/// available (C's `diff_simple`, `diff_set_0`, `diff_set_1`,
/// `diff_forcing`, `diff_recursive` arguments). Levels are the game's own
/// difficulty numbers, starting at 0; several may share a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Levels {
    /// Positional and numeric elimination ([`LatinSolver::diff_simple`]).
    pub simple: usize,
    /// Set elimination in rows and columns
    /// ([`LatinSolver::diff_set`] with `extreme = false`).
    pub set: usize,
    /// Set elimination on one digit's positions
    /// ([`LatinSolver::diff_set`] with `extreme = true`).
    pub extreme_set: usize,
    /// Forcing chains ([`LatinSolver::forcing`]).
    pub forcing: usize,
    /// Guessing and recursing: the highest level.
    pub recursive: usize,
}

/// A game's own rules, plugged into the Latin solver (C's `usersolvers`
/// array, `valid` callback, and `ctx` with `ctxnew`/`ctxfree`).
///
/// Implement it on the game's solver context. Every method has a default
/// that adds nothing: [`NoRules`] solves plain Latin squares.
pub trait LatinRules: Sized {
    /// The game's own deductions at difficulty `level` (C's
    /// `usersolvers[level]`). Called for each level up to the solve's
    /// maximum before the built-in deductions of that level.
    fn deduce(&mut self, solver: &mut LatinSolver, level: usize) -> Deduction {
        let _ = (solver, level);
        Deduction::None
    }

    /// Final check of a full grid against the game's clues (C's `valid`);
    /// false makes the solve [`Outcome::Impossible`].
    fn is_valid(&mut self, solver: &mut LatinSolver) -> bool {
        let _ = solver;
        true
    }

    /// A context for one guess while recursing (C's `ctxnew`; dropping it
    /// is `ctxfree`). `None`, the default, shares this one with the guess.
    fn branch(&self) -> Option<Self> {
        None
    }
}

/// No game rules: solves plain Latin squares.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRules;

impl LatinRules for NoRules {}

/// Result of a guessing round (C's `latin_solver_recurse` return value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guessing {
    /// The grid was already full.
    Complete,
    /// No guess led to a solution.
    Impossible,
    /// Exactly one solution.
    Unique,
    /// More than one solution.
    Ambiguous,
}

/// The Latin square solver (C's `struct latin_solver`): a grid and, for
/// every cell and digit, whether the digit is still a candidate there.
///
/// ```
/// use puzzles_core::latin::{LatinSolver, Levels, NoRules, Outcome};
///
/// const LEVELS: Levels =
///     Levels { simple: 0, set: 1, extreme_set: 2, forcing: 2, recursive: 3 };
/// let grid = vec![
///     1, 2, 0,
///     0, 0, 1,
///     0, 0, 0,
/// ];
/// let mut solver = LatinSolver::new(grid, 3);
/// let outcome = solver.solve(LEVELS.recursive, &LEVELS, &mut NoRules);
/// assert_eq!(outcome, Outcome::Solved { difficulty: 0 });
/// assert_eq!(solver.grid(), [1, 2, 3, 2, 3, 1, 3, 1, 2]);
///
/// // With only the 1s given there are two solutions.
/// let mut solver = LatinSolver::new(vec![1, 0, 0, 0, 0, 1, 0, 0, 0], 3);
/// let outcome = solver.solve(LEVELS.recursive, &LEVELS, &mut NoRules);
/// assert_eq!(outcome, Outcome::Ambiguous);
/// ```
#[doc(alias = "latin_solver")]
#[derive(Clone, Debug)]
pub struct LatinSolver {
    order: usize,
    /// `cube[cube_pos(x, y, n)]`: `n` is still a candidate at `(x, y)`.
    cube: Vec<bool>,
    /// `grid[y * o + x]`: the digit placed there, or 0.
    grid: Vec<Digit>,
    /// `row[y * o + n - 1]`: `n` has been placed in row `y`.
    row: Vec<bool>,
    /// `col[x * o + n - 1]`: `n` has been placed in column `x`.
    col: Vec<bool>,
    /// False if the starting grid already broke the rules.
    consistent: bool,
    /// Where the solver (and the game's deductions) record their
    /// reasoning. Quiet unless replaced.
    pub log: SolverLog,
}

impl LatinSolver {
    /// Sets up a solver for `grid` (`order * order` digits, row by row,
    /// 0 for empty) and places its digits (C's `latin_solver_alloc`).
    ///
    /// If two given digits clash, or one is out of range (C's behaviour is
    /// undefined there), setting up stops at that digit: the solver is
    /// inconsistent and [`solve`](Self::solve) reports
    /// [`Outcome::Impossible`] (C's `latin_solver_alloc` returning false).
    ///
    /// # Panics
    /// If `grid.len()` isn't `order * order`.
    #[doc(alias = "latin_solver_alloc")]
    #[must_use]
    pub fn new(grid: Vec<Digit>, order: usize) -> Self {
        let o = order;
        assert_eq!(grid.len(), o * o, "grid must have order * order cells");
        let mut solver = Self {
            order: o,
            cube: vec![true; o * o * o],
            grid,
            row: vec![false; o * o],
            col: vec![false; o * o],
            consistent: true,
            log: SolverLog::quiet(),
        };
        'setup: for x in 0..o {
            for y in 0..o {
                let n = usize::from(solver.grid[y * o + x]);
                if n == 0 {
                    continue;
                }
                if n > o || !solver.is_possible(x, y, n) {
                    solver.consistent = false;
                    break 'setup;
                }
                solver.place(x, y, n);
            }
        }
        solver
    }

    /// The square's order `o`.
    #[must_use]
    pub fn order(&self) -> usize {
        self.order
    }

    /// The grid, row by row: the digit at `(x, y)` is `grid()[y * o + x]`,
    /// 0 if not yet placed.
    #[must_use]
    pub fn grid(&self) -> &[Digit] {
        &self.grid
    }

    /// Takes the grid out of the solver.
    #[must_use]
    pub fn into_grid(self) -> Vec<Digit> {
        self.grid
    }

    /// Whether the starting grid was free of clashes (C's
    /// `latin_solver_alloc` return value).
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.consistent
    }

    /// The candidate cube: `cube()[cube_pos(x, y, n)]` is true while `n`
    /// may go at `(x, y)`.
    #[must_use]
    pub fn cube(&self) -> &[bool] {
        &self.cube
    }

    /// The candidate cube, for game deductions that rule candidates out.
    pub fn cube_mut(&mut self) -> &mut [bool] {
        &mut self.cube
    }

    /// Index of digit `n` at `(x, y)` in [`cube`](Self::cube) (C's
    /// `cubepos`). Stepping `y` moves by `o`, stepping `x` by `o * o`.
    #[doc(alias = "cubepos")]
    #[must_use]
    pub fn cube_pos(&self, x: usize, y: usize, n: usize) -> usize {
        (x * self.order + y) * self.order + n - 1
    }

    /// True while `n` is a candidate at `(x, y)` (C's `cube(x, y, n)`).
    #[must_use]
    pub fn is_possible(&self, x: usize, y: usize, n: usize) -> bool {
        self.cube[self.cube_pos(x, y, n)]
    }

    /// Rules `n` out at `(x, y)`.
    pub fn rule_out(&mut self, x: usize, y: usize, n: usize) {
        let pos = self.cube_pos(x, y, n);
        self.cube[pos] = false;
    }

    /// True once `n` has been placed in row `y`.
    #[must_use]
    pub fn row_has(&self, y: usize, n: usize) -> bool {
        self.row[y * self.order + n - 1]
    }

    /// True once `n` has been placed in column `x`.
    #[must_use]
    pub fn col_has(&self, x: usize, n: usize) -> bool {
        self.col[x * self.order + n - 1]
    }

    /// Places `n` at `(x, y)`: rules out every other digit there and `n`
    /// everywhere else in its row and column (C's `latin_solver_place`).
    ///
    /// # Panics
    /// If `n` isn't in `1..=o` or isn't a candidate at `(x, y)` (C asserts).
    #[doc(alias = "latin_solver_place")]
    pub fn place(&mut self, x: usize, y: usize, n: usize) {
        let o = self.order;
        assert!((1..=o).contains(&n), "digit {n} out of range for order {o}");
        assert!(
            self.is_possible(x, y, n),
            "{n} is not a candidate at ({x},{y})"
        );
        for i in (1..=o).filter(|&i| i != n) {
            self.rule_out(x, y, i);
        }
        for i in (0..o).filter(|&i| i != y) {
            self.rule_out(x, i, n);
        }
        for i in (0..o).filter(|&i| i != x) {
            self.rule_out(i, y, n);
        }
        self.grid[y * o + x] = Digit::try_from(n).expect("digit exceeds 255");
        self.row[y * o + n - 1] = true;
        self.col[x * o + n - 1] = true;
    }

    /// The cell and digit of a cube index: the inverse of
    /// [`cube_pos`](Self::cube_pos).
    fn cube_cell(&self, pos: usize) -> (usize, usize, usize) {
        let o = self.order;
        let n = 1 + pos % o;
        let xy = pos / o;
        (xy / o, xy % o, n)
    }

    /// Records `what` as the heading of a deduction, as C prints its
    /// format argument followed by a colon.
    fn log_heading(&mut self, what: fmt::Arguments<'_>) {
        self.log.indent();
        self.log.raw(what);
        self.log.raw(format_args!(":\n"));
    }

    /// Positional elimination on one line of the cube: the `o` candidates
    /// at `start`, `start + step`, … (C's `latin_solver_elim`). If only one
    /// is left and its cell is empty, places it; if none is left, that's a
    /// contradiction. `what` names the line in the log.
    #[doc(alias = "latin_solver_elim")]
    pub fn elim(&mut self, start: usize, step: usize, what: fmt::Arguments<'_>) -> Deduction {
        let o = self.order;
        let mut found = None;
        let mut count = 0;
        for i in 0..o {
            let pos = start + i * step;
            if self.cube[pos] {
                found = Some(pos);
                count += 1;
            }
        }

        match (count, found) {
            (1, Some(pos)) => {
                let (x, y, n) = self.cube_cell(pos);
                if self.grid[y * o + x] == 0 {
                    if self.log.enabled() {
                        self.log_heading(what);
                        self.log
                            .line(format_args!("  placing {n} at ({},{})", x + 1, y + 1));
                    }
                    self.place(x, y, n);
                    return Deduction::Progress;
                }
                Deduction::None
            }
            (0, _) => {
                if self.log.enabled() {
                    self.log_heading(what);
                    self.log.line(format_args!("  no possibilities available"));
                }
                Deduction::Contradiction
            }
            _ => Deduction::None,
        }
    }

    /// Set elimination on an `o` × `o` slice of the cube: entry `(i, j)`
    /// is at `start + i * step1 + j * step2` (C's `latin_solver_set`).
    ///
    /// After setting aside rows with a single candidate (and their
    /// columns), it looks for a set of columns that, between them, have
    /// candidates in only as many rows as there are columns in the set.
    /// Those rows must use those columns, so every other row loses its
    /// candidates there. Sets are tried in binary counting order. `what`
    /// names the slice in the log.
    ///
    /// # Panics
    /// If the slice reaches outside the cube.
    #[doc(alias = "latin_solver_set")]
    pub fn set(
        &mut self,
        start: usize,
        step1: usize,
        step2: usize,
        what: fmt::Arguments<'_>,
    ) -> Deduction {
        let o = self.order;
        let at = |i: usize, j: usize| start + i * step1 + j * step2;

        // Set aside every row with a single candidate, and its column.
        let mut row_open = vec![true; o];
        let mut col_open = vec![true; o];
        for (i, open) in row_open.iter_mut().enumerate() {
            let mut count = 0;
            let mut first = 0;
            for j in 0..o {
                if self.cube[at(i, j)] {
                    first = j;
                    count += 1;
                }
            }
            match count {
                0 => return Deduction::Contradiction,
                1 => {
                    *open = false;
                    col_open[first] = false;
                }
                _ => {}
            }
        }
        let rows: Vec<usize> = (0..o).filter(|&i| row_open[i]).collect();
        let cols: Vec<usize> = (0..o).filter(|&j| col_open[j]).collect();
        let n = rows.len();
        assert_eq!(n, cols.len(), "rows and columns set aside differ");

        // The remaining n x n matrix; every row has at least 2 candidates.
        let matrix: Vec<bool> = rows
            .iter()
            .flat_map(|&r| cols.iter().map(move |&c| (r, c)))
            .map(|(r, c)| self.cube[at(r, c)])
            .collect();
        let cell = |i: usize, j: usize| matrix[i * n + j];

        let mut set = vec![false; n];
        let mut count = 0;
        loop {
            // Sets of 0, 1, n-1 or n columns can't tell us anything.
            if count > 1 && count + 1 < n {
                // Rows with no candidates in any of the set's columns.
                let avoids = |i: usize| (0..n).all(|j| !(set[j] && cell(i, j)));
                let rows_avoiding = (0..n).filter(|&i| avoids(i)).count();

                // More than n - count such rows: count digits squeezed
                // into fewer cells. A bad deduction or bogus clue.
                if rows_avoiding > n - count {
                    if self.log.enabled() {
                        self.log_heading(what);
                        self.log.line(format_args!("  contradiction reached"));
                    }
                    return Deduction::Contradiction;
                }

                if rows_avoiding == n - count {
                    let mut progress = false;
                    for i in (0..n).filter(|&i| !avoids(i)) {
                        for j in (0..n).filter(|&j| !set[j] && cell(i, j)) {
                            let pos = at(rows[i], cols[j]);
                            if self.log.enabled() {
                                if !progress {
                                    self.log_heading(what);
                                }
                                let (px, py, pn) = self.cube_cell(pos);
                                self.log.line(format_args!(
                                    "  ruling out {pn} at ({},{})",
                                    px + 1,
                                    py + 1
                                ));
                            }
                            progress = true;
                            self.cube[pos] = false;
                        }
                    }
                    if progress {
                        return Deduction::Progress;
                    }
                }
            }

            // Binary increment: the rightmost unset entry becomes set and
            // every entry right of it unset; stop after all ones.
            let mut i = n;
            while i > 0 && set[i - 1] {
                i -= 1;
                set[i] = false;
                count -= 1;
            }
            if i == 0 {
                break;
            }
            set[i - 1] = true;
            count += 1;
        }
        Deduction::None
    }

    /// Forcing chains (C's `latin_solver_forcing`).
    ///
    /// A chain runs through cells with exactly two candidates, each in line
    /// with the next and sharing a candidate with it, so that ruling a
    /// digit out at one end forces the other end's digit all along it. If
    /// ruling `n` out at the start forces `n` at the far end, then one of
    /// the two ends holds `n`, and a third cell in line with both can't.
    /// A breadth-first search runs from each two-candidate cell, once per
    /// candidate, and stops at the first such third cell.
    #[doc(alias = "latin_solver_forcing")]
    pub fn forcing(&mut self) -> Deduction {
        let o = self.order;
        // Cells the search has reached, and from where (for the log).
        // The queue holds each cell with the digit it is forced to take.
        let mut visited = vec![false; o * o];
        let mut prev: Vec<Option<usize>> = vec![None; o * o];
        let mut queue = Vec::with_capacity(o * o);

        for y in 0..o {
            for x in 0..o {
                let Some((first, second)) = self.two_candidates(x, y) else {
                    continue;
                };
                for orig in [first, second] {
                    let other = if orig == first { second } else { first };
                    visited.fill(false);
                    queue.clear();
                    queue.push((y * o + x, other));
                    prev[y * o + x] = None;
                    visited[y * o + x] = true;

                    let mut head = 0;
                    while head < queue.len() {
                        let (here, current) = queue[head];
                        head += 1;
                        let (xx, yy) = (here % o, here / o);

                        // Its column, then its row (itself twice).
                        let column = (0..o).map(|yt| (xx, yt));
                        let row = (0..o).map(|xt| (xt, yy));
                        for (xt, yt) in column.chain(row) {
                            let there = yt * o + xt;
                            if visited[there]
                                || !self.is_possible(xt, yt, current)
                                || (xt, yt) == (xx, yy)
                            {
                                continue;
                            }

                            if let Some((a, b)) = self.two_candidates(xt, yt) {
                                let next = if a == current { b } else { a };
                                queue.push((there, next));
                                prev[there] = Some(here);
                                visited[there] = true;
                            }

                            if current == orig && (xt == x || yt == y) {
                                if self.log.enabled() {
                                    self.log_forcing_chain(&prev, here, orig, (xt, yt));
                                }
                                self.rule_out(xt, yt, orig);
                                return Deduction::Progress;
                            }
                        }
                    }
                }
            }
        }
        Deduction::None
    }

    /// The two candidates at `(x, y)`, in increasing order, if it has
    /// exactly two.
    fn two_candidates(&self, x: usize, y: usize) -> Option<(usize, usize)> {
        let mut candidates = (1..=self.order).filter(|&n| self.is_possible(x, y, n));
        let first = candidates.next()?;
        let second = candidates.next()?;
        candidates.next().is_none().then_some((first, second))
    }

    fn log_forcing_chain(
        &mut self,
        prev: &[Option<usize>],
        end: usize,
        digit: usize,
        (xt, yt): (usize, usize),
    ) {
        let o = self.order;
        self.log.indent();
        self.log
            .raw(format_args!("forcing chain, {digit} at ends of "));
        let mut cell = Some(end);
        let mut sep = "";
        while let Some(c) = cell {
            self.log
                .raw(format_args!("{sep}({},{})", c % o + 1, c / o + 1));
            sep = "-";
            cell = prev[c];
        }
        self.log.raw(format_args!("\n"));
        self.log.line(format_args!(
            "  ruling out {digit} at ({},{})",
            xt + 1,
            yt + 1
        ));
    }

    /// Positional elimination in every row and column for every digit
    /// not yet placed there, then numeric elimination in every empty cell
    /// (C's `latin_solver_diff_simple`). Stops at the first progress or
    /// contradiction.
    #[doc(alias = "latin_solver_diff_simple")]
    pub fn diff_simple(&mut self) -> Deduction {
        let o = self.order;
        for y in 0..o {
            for n in 1..=o {
                if self.row_has(y, n) {
                    continue;
                }
                let start = self.cube_pos(0, y, n);
                let ret = self.elim(
                    start,
                    o * o,
                    format_args!("positional elimination, {n} in row {}", y + 1),
                );
                if ret != Deduction::None {
                    return ret;
                }
            }
        }
        for x in 0..o {
            for n in 1..=o {
                if self.col_has(x, n) {
                    continue;
                }
                let start = self.cube_pos(x, 0, n);
                let ret = self.elim(
                    start,
                    o,
                    format_args!("positional elimination, {n} in column {}", x + 1),
                );
                if ret != Deduction::None {
                    return ret;
                }
            }
        }
        for x in 0..o {
            for y in 0..o {
                if self.grid[y * o + x] != 0 {
                    continue;
                }
                let start = self.cube_pos(x, y, 1);
                let ret = self.elim(
                    start,
                    1,
                    format_args!("numeric elimination at ({},{})", x + 1, y + 1),
                );
                if ret != Deduction::None {
                    return ret;
                }
            }
        }
        Deduction::None
    }

    /// Set elimination (C's `latin_solver_diff_set`): on each row's and
    /// then each column's cells × digits, or if `extreme`, on each digit's
    /// rows × columns. Stops at the first progress or contradiction.
    #[doc(alias = "latin_solver_diff_set")]
    pub fn diff_set(&mut self, extreme: bool) -> Deduction {
        let o = self.order;
        if extreme {
            for n in 1..=o {
                let start = self.cube_pos(0, 0, n);
                let ret = self.set(
                    start,
                    o * o,
                    o,
                    format_args!("positional set elimination on {n}"),
                );
                if ret != Deduction::None {
                    return ret;
                }
            }
        } else {
            for y in 0..o {
                let start = self.cube_pos(0, y, 1);
                let ret = self.set(
                    start,
                    o * o,
                    1,
                    format_args!("set elimination, row {}", y + 1),
                );
                if ret != Deduction::None {
                    return ret;
                }
            }
            for x in 0..o {
                let start = self.cube_pos(x, 0, 1);
                let ret = self.set(
                    start,
                    o,
                    1,
                    format_args!("set elimination, column {}", x + 1),
                );
                if ret != Deduction::None {
                    return ret;
                }
            }
        }
        Deduction::None
    }

    /// Solves the grid using deductions up to difficulty `max_diff`,
    /// guessing too if that is `levels.recursive` (C's `latin_solver_main`,
    /// and with [`new`](Self::new) and [`into_grid`](Self::into_grid),
    /// `latin_solver`).
    ///
    /// Each round tries the levels in order, first the game's
    /// [`LatinRules::deduce`] and then this level's built-in deductions,
    /// and starts over after any progress. The grid is left as far as the
    /// solve got: on [`Outcome::Ambiguous`], the first solution found.
    ///
    /// # Panics
    /// If `max_diff > levels.recursive` (C asserts).
    #[doc(alias("latin_solver_main", "latin_solver_top"))]
    pub fn solve<R: LatinRules>(
        &mut self,
        max_diff: usize,
        levels: &Levels,
        rules: &mut R,
    ) -> Outcome {
        if !self.consistent {
            return Outcome::Impossible;
        }
        assert!(
            max_diff <= levels.recursive,
            "max_diff {max_diff} above the recursive level {}",
            levels.recursive
        );

        let mut outcome = self.deduce_all(max_diff, levels, rules);
        if outcome != Outcome::Impossible {
            if max_diff == levels.recursive {
                match self.guess(levels, rules) {
                    Guessing::Complete => {}
                    Guessing::Impossible => outcome = Outcome::Impossible,
                    Guessing::Unique => {
                        outcome = Outcome::Solved {
                            difficulty: levels.recursive,
                        };
                    }
                    Guessing::Ambiguous => outcome = Outcome::Ambiguous,
                }
            } else if self.grid.contains(&0) {
                outcome = Outcome::Unfinished;
            }
        }

        if self.log.enabled() {
            self.log_outcome(outcome);
        }

        if matches!(outcome, Outcome::Solved { .. }) && !rules.is_valid(self) {
            self.log
                .line(format_args!("solution failed final validation!"));
            outcome = Outcome::Impossible;
        }
        outcome
    }

    /// Deduction rounds until one makes no progress: `Solved` with the
    /// highest level used so far, or `Impossible`.
    fn deduce_all<R: LatinRules>(
        &mut self,
        max_diff: usize,
        levels: &Levels,
        rules: &mut R,
    ) -> Outcome {
        let mut difficulty = levels.simple;
        'round: loop {
            if self.log.verbosity() > 1 {
                log_candidates(&mut self.log, &self.cube, self.order);
            }
            for level in 0..=max_diff {
                let mut ret = rules.deduce(self, level);
                if ret == Deduction::None && level == levels.simple {
                    ret = self.diff_simple();
                }
                if ret == Deduction::None && level == levels.set {
                    ret = self.diff_set(false);
                }
                if ret == Deduction::None && level == levels.extreme_set {
                    ret = self.diff_set(true);
                }
                if ret == Deduction::None && level == levels.forcing {
                    ret = self.forcing();
                }
                match ret {
                    Deduction::Contradiction => return Outcome::Impossible,
                    Deduction::Progress => {
                        difficulty = difficulty.max(level);
                        continue 'round;
                    }
                    Deduction::None => {}
                }
            }
            return Outcome::Solved { difficulty };
        }
    }

    /// Guesses each candidate in turn in one of the most constrained empty
    /// cells, solving each guess with a fresh solver at the recursive
    /// level (C's `latin_solver_recurse`). Stops at a second solution.
    fn guess<R: LatinRules>(&mut self, levels: &Levels, rules: &mut R) -> Guessing {
        let o = self.order;
        let mut best = None;
        let mut best_count = o + 1;
        for y in 0..o {
            for x in 0..o {
                if self.grid[y * o + x] != 0 {
                    continue;
                }
                let count = (1..=o).filter(|&n| self.is_possible(x, y, n)).count();
                // Deductions would already have caught 0 or 1.
                assert!(count > 1, "unfilled cell ({x},{y}) has {count} candidates");
                if count < best_count {
                    best_count = count;
                    best = Some((x, y));
                }
            }
        }
        let Some((x, y)) = best else {
            return Guessing::Complete;
        };

        let candidates: Vec<usize> = (1..=o).filter(|&n| self.is_possible(x, y, n)).collect();
        if self.log.enabled() {
            self.log.indent();
            self.log
                .raw(format_args!("recursing on ({},{}) [", x + 1, y + 1));
            let mut sep = "";
            for &n in &candidates {
                self.log.raw(format_args!("{sep}{n}"));
                sep = " or ";
            }
            self.log.raw(format_args!("]\n"));
        }

        // Every guess starts from the grid as it is now; the first solution
        // found is copied into it.
        let start = self.grid.clone();
        let mut result = Guessing::Impossible;
        for &n in &candidates {
            let mut grid = start.clone();
            grid[y * o + x] = Digit::try_from(n).expect("digit exceeds 255");
            self.log
                .line(format_args!("guessing {n} at ({},{})", x + 1, y + 1));

            let mut branch = rules.branch();
            let context = branch.as_mut().unwrap_or(&mut *rules);
            let mut sub = Self::new(grid, o);
            // The guess's solver logs one level deeper, into our log.
            let ret = self.log.nested(|log| {
                sub.log = std::mem::take(log);
                let ret = sub.solve(levels.recursive, levels, context);
                *log = std::mem::take(&mut sub.log);
                ret
            });
            drop(branch);

            self.log
                .line(format_args!("retracting {n} at ({},{})", x + 1, y + 1));
            // Recursion goes as deep as it can, so it never gives up.
            assert_ne!(ret, Outcome::Unfinished, "recursive solve gave up");

            if result == Guessing::Impossible && ret != Outcome::Impossible {
                self.grid.copy_from_slice(&sub.grid);
            }
            result = match (ret, result) {
                (Outcome::Impossible, _) => result,
                (Outcome::Ambiguous, _) | (_, Guessing::Unique) => Guessing::Ambiguous,
                _ => Guessing::Unique,
            };
            if result == Guessing::Ambiguous {
                break;
            }
        }
        result
    }

    fn log_outcome(&mut self, outcome: Outcome) {
        let o = self.order;
        match outcome {
            Outcome::Solved { .. } => {
                self.log.line(format_args!("one solution found:"));
                for y in 0..o {
                    self.log.indent();
                    self.log.raw(format_args!(" "));
                    for x in 0..o {
                        let digit = self.grid[y * o + x];
                        assert_ne!(digit, 0, "solved grid has an empty cell");
                        self.log.raw(format_args!(" {digit}"));
                    }
                    self.log.raw(format_args!("\n"));
                }
            }
            Outcome::Impossible => self
                .log
                .line(format_args!("no solution (impossible) found")),
            Outcome::Unfinished => self
                .log
                .line(format_args!("no solution (unfinished) found")),
            Outcome::Ambiguous => self.log.line(format_args!("multiple solutions found")),
        }
    }
}

/// Records every cell's candidates, a row of cells per line, if `log`'s
/// verbosity is above 1 (C's `latin_solver_debug`). A candidate shows as
/// the character `'0' + n`, a ruled-out one as `.`.
///
/// # Panics
/// If `cube` is shorter than `order³`.
#[doc(alias = "latin_solver_debug")]
pub fn log_candidates(log: &mut SolverLog, cube: &[bool], order: usize) {
    if log.verbosity() <= 1 {
        return;
    }
    let o = order;
    let mut text = String::with_capacity(o * o * (o + 1) + o + 1);
    for y in 0..o {
        for x in 0..o {
            for n in 1..=o {
                text.push(if cube[(x * o + y) * o + n - 1] {
                    digit_char(n)
                } else {
                    '.'
                });
            }
            text.push(' ');
        }
        text.push('\n');
    }
    text.push('\n');
    log.raw(format_args!("{text}"));
}

/// C's `n + '0'`, wrapping like a C `char` would for large orders.
fn digit_char(n: usize) -> char {
    #[expect(clippy::cast_possible_truncation, reason = "C stores it in a char")]
    char::from(b'0'.wrapping_add(n as u8))
}

/// Records a square, `{:2} ` per cell and a row per line (C's
/// `latin_debug`).
///
/// # Panics
/// If `square` is shorter than `order * order`.
#[doc(alias = "latin_debug")]
pub fn log_square(log: &mut SolverLog, square: &[Digit], order: usize) {
    if !log.enabled() {
        return;
    }
    for row in square[..order * order].chunks(order.max(1)) {
        for digit in row {
            log.raw(format_args!("{digit:2} "));
        }
        log.raw(format_args!("\n"));
    }
    log.raw(format_args!("\n"));
}

/// Generates a random Latin square of order `order`, row by row (C's
/// `latin_generate`). Returns `order * order` digits, row by row.
///
/// Any `r` × `o` Latin rectangle with `r < o` extends to `r + 1` rows,
/// so each row is a perfect matching between columns and the digits not
/// yet in them, found by [`Matcher`] with a random search; every square
/// can come out. The rows are filled in a random order too.
///
/// # Panics
/// If `order` exceeds 255 (digits are bytes).
#[doc(alias = "latin_generate")]
#[must_use]
pub fn generate(order: usize, rng: &mut RandomState) -> Vec<Digit> {
    let o = order;
    assert!(o <= usize::from(Digit::MAX), "order {o} exceeds 255");
    let mut square: Vec<Digit> = vec![0; o * o];
    let mut rows: Vec<usize> = (0..o).collect();
    rng.shuffle(&mut rows);

    let mut matcher = Matcher::new();
    for (i, &row) in rows.iter().enumerate() {
        // Column j may take any digit not already in it.
        let matched = matcher.run(
            o,
            o,
            |j| {
                let mut free = vec![true; o];
                for &done in &rows[..i] {
                    free[usize::from(square[done * o + j]) - 1] = false;
                }
                (0..o).filter(move |&k| free[k])
            },
            Some(&mut *rng),
        );
        assert_eq!(matched, o, "a Latin rectangle always extends by a row");
        for (j, digit) in matcher.left_to_right().iter().enumerate() {
            let digit = digit.expect("every column is matched") + 1;
            square[row * o + j] = Digit::try_from(digit).expect("order fits in a digit");
        }
    }
    square
}

/// Generates a random `w` × `h` Latin rectangle (C's
/// `latin_generate_rect`): the top-left corner of a square of order
/// `max(w, h)` from [`generate`]. Returns `w * h` digits, row by row.
///
/// # Panics
/// If `max(w, h)` exceeds 255.
#[doc(alias = "latin_generate_rect")]
#[must_use]
pub fn generate_rect(w: usize, h: usize, rng: &mut RandomState) -> Vec<Digit> {
    let o = w.max(h);
    let square = generate(o, rng);
    (0..h)
        .flat_map(|y| square[y * o..y * o + w].iter().copied())
        .collect()
}

/// True if the first `order * order` cells hold exactly `order` distinct
/// values, each `order` times (the negation of C's `latin_check`).
///
/// Like C, this checks only how often each value occurs, not where: rows
/// and columns aren't examined.
///
/// # Panics
/// If `square` is shorter than `order * order`.
#[doc(alias = "latin_check")]
#[must_use]
pub fn has_latin_counts(square: &[Digit], order: usize) -> bool {
    let mut counts = [0_usize; 256];
    for &digit in &square[..order * order] {
        counts[usize::from(digit)] += 1;
    }
    let distinct = counts.iter().filter(|&&c| c > 0).count();
    distinct == order && counts.iter().all(|&c| c == 0 || c == order)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: Levels = Levels {
        simple: 0,
        set: 1,
        extreme_set: 2,
        forcing: 2,
        recursive: 3,
    };

    /// Every row and every column holds each digit once.
    fn is_latin(square: &[Digit], w: usize, h: usize, o: usize) -> bool {
        let rows = (0..h).all(|y| {
            let mut seen = vec![false; o + 1];
            (0..w).all(|x| !std::mem::replace(&mut seen[usize::from(square[y * w + x])], true))
        });
        let cols = (0..w).all(|x| {
            let mut seen = vec![false; o + 1];
            (0..h).all(|y| !std::mem::replace(&mut seen[usize::from(square[y * w + x])], true))
        });
        rows && cols && square.iter().all(|&d| (1..=o).contains(&usize::from(d)))
    }

    #[test]
    fn generated_squares_are_latin() {
        let mut rng = RandomState::new(b"squares");
        assert_eq!(generate(0, &mut rng), Vec::<Digit>::new());
        for o in 1..=12 {
            let square = generate(o, &mut rng);
            assert!(is_latin(&square, o, o, o), "order {o}");
            assert!(has_latin_counts(&square, o));
        }
    }

    #[test]
    fn generated_rectangles_are_latin() {
        let mut rng = RandomState::new(b"rects");
        for (w, h) in [(1, 1), (5, 3), (3, 5), (7, 7), (9, 2)] {
            let rect = generate_rect(w, h, &mut rng);
            assert_eq!(rect.len(), w * h);
            assert!(is_latin(&rect, w, h, w.max(h)), "{w}x{h}");
        }
    }

    #[test]
    fn generators_are_cs() {
        // Golden values recorded from the C oracle: the output, then the
        // generator's next 32 bits (so the draws used were the same).
        let mut rng = RandomState::new(b"golden");
        let square = generate(4, &mut rng);
        assert_eq!(square, [3, 4, 1, 2, 1, 2, 4, 3, 2, 1, 3, 4, 4, 3, 2, 1]);
        assert_eq!(rng.bits(32), 2_571_303_469);

        let mut rng = RandomState::new(b"golden");
        let rect = generate_rect(5, 3, &mut rng);
        assert_eq!(rect, [5, 4, 2, 1, 3, 3, 2, 1, 5, 4, 2, 5, 4, 3, 1]);
        assert_eq!(rng.bits(32), 2_025_904_728);
    }

    #[test]
    fn has_latin_counts_checks_counts_only() {
        assert!(has_latin_counts(&[], 0));
        assert!(has_latin_counts(&[1, 2, 2, 1], 2));
        // Rows repeat, but the counts are right: accepted, as in C.
        assert!(has_latin_counts(&[1, 1, 2, 2], 2));
        assert!(!has_latin_counts(&[1, 1, 1, 2], 2));
        assert!(!has_latin_counts(&[1, 2, 3, 1], 2));
        // Only the first order^2 cells count.
        assert!(has_latin_counts(&[1, 2, 2, 1, 9], 2));
    }

    #[test]
    fn place_rules_out_row_column_and_cell() {
        let mut solver = LatinSolver::new(vec![0; 9], 3);
        solver.place(1, 2, 3);
        assert_eq!(solver.grid()[2 * 3 + 1], 3);
        assert!(solver.row_has(2, 3) && solver.col_has(1, 3));
        assert!(!solver.row_has(2, 1) && !solver.col_has(0, 3));
        for i in 0..3 {
            assert_eq!(solver.is_possible(i, 2, 3), i == 1, "row");
            assert_eq!(solver.is_possible(1, i, 3), i == 2, "column");
            assert_eq!(solver.is_possible(1, 2, i + 1), i + 1 == 3, "cell");
        }
        assert!(solver.is_possible(0, 0, 3));
    }

    #[test]
    fn clashing_or_out_of_range_givens_are_impossible() {
        for grid in [vec![1, 1, 0, 0], vec![1, 0, 1, 0], vec![3, 0, 0, 0]] {
            let mut solver = LatinSolver::new(grid, 2);
            assert!(!solver.is_consistent());
            assert_eq!(solver.solve(3, &LEVELS, &mut NoRules), Outcome::Impossible);
        }
    }

    #[test]
    fn elim_places_a_lone_candidate_and_spots_none() {
        let mut solver = LatinSolver::new(vec![0; 9], 3);
        solver.rule_out(0, 0, 1);
        solver.rule_out(1, 0, 1);
        let start = solver.cube_pos(0, 0, 1);
        assert_eq!(
            solver.elim(start, 9, format_args!("row")),
            Deduction::Progress
        );
        assert_eq!(solver.grid()[2], 1);
        // Already placed: nothing more to learn.
        assert_eq!(solver.elim(start, 9, format_args!("row")), Deduction::None);

        solver.rule_out(2, 0, 1);
        assert_eq!(
            solver.elim(start, 9, format_args!("row")),
            Deduction::Contradiction
        );
    }

    #[test]
    fn set_elimination_finds_a_naked_pair() {
        // Row 0 of order 4: cells 0 and 1 can only hold 1 or 2, so cells 2
        // and 3 can't.
        let mut solver = LatinSolver::new(vec![0; 16], 4);
        for x in 0..2 {
            solver.rule_out(x, 0, 3);
            solver.rule_out(x, 0, 4);
        }
        let start = solver.cube_pos(0, 0, 1);
        assert_eq!(
            solver.set(start, 16, 1, format_args!("row")),
            Deduction::Progress
        );
        for x in 2..4 {
            assert!(!solver.is_possible(x, 0, 1) && !solver.is_possible(x, 0, 2));
            assert!(solver.is_possible(x, 0, 3) && solver.is_possible(x, 0, 4));
        }
    }

    #[test]
    fn forcing_chain_rules_out_the_shared_digit() {
        // Order 3: (0,0) is {1,2}, (2,0) is {2,3} and (2,2) is {1,3}. If
        // (0,0) isn't 1 it is 2, so (2,0) is 3 and (2,2) is 1. Either way
        // (0,0) or (2,2) holds 1, so (0,2), in line with both, can't.
        let mut solver = LatinSolver::new(vec![0; 9], 3);
        for (x, y, n) in [(0, 0, 3), (2, 0, 1), (2, 2, 2)] {
            solver.rule_out(x, y, n);
        }
        assert!(solver.is_possible(0, 2, 1));
        assert_eq!(solver.forcing(), Deduction::Progress);
        assert!(!solver.is_possible(0, 2, 1));
    }

    #[test]
    fn deductions_report_their_level() {
        // Solvable by elimination alone.
        let grid = vec![1, 2, 0, 0, 0, 1, 0, 0, 0];
        let mut solver = LatinSolver::new(grid.clone(), 3);
        assert_eq!(
            solver.solve(0, &LEVELS, &mut NoRules),
            Outcome::Solved { difficulty: 0 }
        );
        assert_eq!(solver.into_grid(), [1, 2, 3, 2, 3, 1, 3, 1, 2]);

        // Two solutions: unfinished without guessing, ambiguous with it.
        let grid = vec![1, 0, 0, 0, 0, 1, 0, 0, 0];
        let mut solver = LatinSolver::new(grid.clone(), 3);
        assert_eq!(solver.solve(2, &LEVELS, &mut NoRules), Outcome::Unfinished);
        let mut solver = LatinSolver::new(grid, 3);
        assert_eq!(solver.solve(3, &LEVELS, &mut NoRules), Outcome::Ambiguous);
        assert_eq!(solver.grid(), [1, 2, 3, 2, 3, 1, 3, 1, 2]);
    }

    /// Rules that count calls, reject solutions with `bad` at cell 0, and
    /// can give each guess a fresh context.
    struct Counting {
        deduced: Vec<usize>,
        validated: usize,
        bad: Digit,
        fresh: bool,
    }

    impl LatinRules for Counting {
        fn deduce(&mut self, _: &mut LatinSolver, level: usize) -> Deduction {
            self.deduced.push(level);
            Deduction::None
        }

        fn is_valid(&mut self, solver: &mut LatinSolver) -> bool {
            self.validated += 1;
            solver.grid()[0] != self.bad
        }

        fn branch(&self) -> Option<Self> {
            self.fresh.then(|| Self {
                deduced: Vec::new(),
                validated: 0,
                bad: self.bad,
                fresh: true,
            })
        }
    }

    #[test]
    fn rules_are_consulted_and_can_reject() {
        let mut rules = Counting {
            deduced: Vec::new(),
            validated: 0,
            bad: 0,
            fresh: false,
        };
        let mut solver = LatinSolver::new(vec![1, 2, 0, 0, 0, 1, 0, 0, 0], 3);
        assert_eq!(
            solver.solve(1, &LEVELS, &mut rules),
            Outcome::Solved { difficulty: 0 }
        );
        // Each round asks level 0, and after no progress also level 1.
        assert_eq!(rules.deduced.last(), Some(&1));
        assert!(rules.deduced.iter().all(|&l| l <= 1));
        assert_eq!(rules.validated, 1);

        rules.bad = 1;
        let mut solver = LatinSolver::new(vec![1, 2, 0, 0, 0, 1, 0, 0, 0], 3);
        assert_eq!(solver.solve(1, &LEVELS, &mut rules), Outcome::Impossible);
    }

    #[test]
    fn guesses_share_or_get_their_own_context() {
        // Two solutions differing at (1,0); rejecting a 2 at cell 0 never
        // fires, so guessing finds both: ambiguous.
        for fresh in [false, true] {
            let mut rules = Counting {
                deduced: Vec::new(),
                validated: 0,
                bad: 2,
                fresh,
            };
            let mut solver = LatinSolver::new(vec![1, 0, 0, 0, 0, 1, 0, 0, 0], 3);
            assert_eq!(solver.solve(3, &LEVELS, &mut rules), Outcome::Ambiguous);
            // Shared: the guesses' validations land here; fresh: they
            // land in the branch contexts, and nothing is validated here.
            assert_eq!(rules.validated, if fresh { 0 } else { 2 });
        }
    }

    #[test]
    fn log_records_placements() {
        let mut solver = LatinSolver::new(vec![1, 2, 0, 0, 0, 1, 0, 0, 0], 3);
        solver.log = SolverLog::new(1);
        let _ = solver.solve(0, &LEVELS, &mut NoRules);
        let log = solver.log.text();
        assert!(log.starts_with("positional elimination, 3 in row 1:\n  placing 3 at (3,1)\n"));
        assert!(log.ends_with("one solution found:\n  1 2 3\n  2 3 1\n  3 1 2\n"));
    }

    #[test]
    fn deduction_from_change_count() {
        assert_eq!(Deduction::from(0), Deduction::None);
        assert_eq!(Deduction::from(3), Deduction::Progress);
    }
}
