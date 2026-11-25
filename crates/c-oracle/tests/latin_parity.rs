//! Differential tests: the Rust `latin` module against C's `latin.c`.
//!
//! * Generators: the square or rectangle, and the generator state after.
//! * Strategies: random sequences of eliminations, set eliminations,
//!   forcing chains and knocked-out candidates on both solvers, comparing
//!   each return code and the whole solver state (cube, grid, placed
//!   flags) after every step.
//! * Full solves with game rules: the same test rules run behind C's
//!   callbacks and the Rust `LatinRules` trait. Besides the outcome, grid
//!   and cube, every callback (deductions per level, validations, new
//!   contexts while guessing) is traced on both sides and the traces must
//!   be identical, so the plumbing calls the game in the same order.
//! * `latin_check` against `has_latin_counts`.

#![cfg(feature = "oracle")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use c_oracle::{
    C_DIFF_AMBIGUOUS, C_DIFF_IMPOSSIBLE, C_DIFF_UNFINISHED, CLatinRules, CLatinSolver, CLatinView,
    CLevels, CRandom, c_latin_check, c_latin_generate, c_latin_generate_rect,
};
use proptest::prelude::*;
use puzzles_core::latin::{
    Deduction, LatinRules, LatinSolver, Levels, Outcome, generate, generate_rect, has_latin_counts,
};
use puzzles_core::random::RandomState;
use puzzles_core::solver_log::SolverLog;

// ---------------------------------------------------------------------
// Generators

/// Generates each order in turn from one generator on both sides.
fn compare_generate(seed: &[u8], orders: &[usize]) -> Result<(), TestCaseError> {
    let mut rust_rng = RandomState::new(seed);
    let mut c_rng = CRandom::new(seed);
    for &o in orders {
        prop_assert_eq!(
            generate(o, &mut rust_rng),
            c_latin_generate(o, &mut c_rng),
            "order {}",
            o
        );
        prop_assert_eq!(rust_rng.encode(), c_rng.encode(), "order {}", o);
    }
    Ok(())
}

proptest! {
    #[test]
    fn generated_squares_match_c(
        orders in proptest::collection::vec(1..=16_usize, 1..6),
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        compare_generate(&seed, &orders)?;
    }

    #[test]
    fn generated_rectangles_match_c(
        w in 1..=14_usize,
        h in 1..=14_usize,
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        let mut rust_rng = RandomState::new(&seed);
        let mut c_rng = CRandom::new(&seed);
        prop_assert_eq!(generate_rect(w, h, &mut rust_rng), c_latin_generate_rect(w, h, &mut c_rng));
        prop_assert_eq!(rust_rng.encode(), c_rng.encode());
    }
}

/// Every order up to 30, and a large one.
#[test]
fn all_orders_match_c() {
    let orders: Vec<usize> = (1..=30).chain([64]).collect();
    compare_generate(b"orders", &orders).expect("parity");
}

// ---------------------------------------------------------------------
// latin_check

proptest! {
    #[test]
    fn latin_check_matches_c(
        order in 0..=6_usize,
        cells in proptest::collection::vec(0..=7_u8, 36),
    ) {
        let square = &cells[..order * order];
        prop_assert_eq!(has_latin_counts(square, order), !c_latin_check(square, order));
    }
}

#[test]
fn latin_check_accepts_generated_squares_like_c() {
    let mut rng = RandomState::new(b"check");
    for o in 1..=9 {
        let square = generate(o, &mut rng);
        assert!(has_latin_counts(&square, o));
        assert!(!c_latin_check(&square, o));
    }
}

// ---------------------------------------------------------------------
// Shared helpers

/// A puzzle grid: a generated square of order `o` with the cells whose
/// `keep` bit is false blanked.
fn puzzle(o: usize, seed: &[u8], keep: &[bool]) -> Vec<u8> {
    let mut grid = generate(o, &mut RandomState::new(seed));
    for (cell, &k) in grid.iter_mut().zip(keep.iter().cycle()) {
        if !k {
            *cell = 0;
        }
    }
    grid
}

fn deduction_code(d: Deduction) -> i32 {
    match d {
        Deduction::None => 0,
        Deduction::Progress => 1,
        Deduction::Contradiction => -1,
    }
}

fn outcome_code(o: Outcome) -> i32 {
    match o {
        Outcome::Solved { difficulty } => i32::try_from(difficulty).expect("small"),
        Outcome::Impossible => C_DIFF_IMPOSSIBLE,
        Outcome::Ambiguous => C_DIFF_AMBIGUOUS,
        Outcome::Unfinished => C_DIFF_UNFINISHED,
    }
}

/// Everything observable in a solver: grid, cube, placed-digit flags.
#[derive(Debug, PartialEq, Eq)]
struct State {
    grid: Vec<u8>,
    cube: Vec<bool>,
    row: Vec<bool>,
    col: Vec<bool>,
}

fn rust_state(s: &LatinSolver) -> State {
    let o = s.order();
    let flags = |f: &dyn Fn(usize, usize) -> bool| {
        (0..o)
            .flat_map(|a| (1..=o).map(move |n| (a, n)))
            .map(|(a, n)| f(a, n))
            .collect()
    };
    State {
        grid: s.grid().to_vec(),
        cube: s.cube().to_vec(),
        row: flags(&|y, n| s.row_has(y, n)),
        col: flags(&|x, n| s.col_has(x, n)),
    }
}

fn c_state(s: &CLatinSolver) -> State {
    let (row, col) = s.rows_cols();
    State {
        grid: s.grid().to_vec(),
        cube: s.cube(),
        row,
        col,
    }
}

// ---------------------------------------------------------------------
// Strategies

#[derive(Clone, Debug)]
enum Op {
    /// Rule out candidate `n` (1-based) at `(x, y)`.
    RuleOut(usize, usize, usize),
    /// Positional elimination in a row (0), column (1) or cell (2).
    Elim(u8, usize, usize, usize),
    /// Set elimination on a row (0), column (1) or digit (2).
    Set(u8, usize),
    Forcing,
    DiffSimple,
    DiffSet(bool),
}

fn op(o: usize) -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..o, 0..o, 1..=o).prop_map(|(x, y, n)| Op::RuleOut(x, y, n)),
        2 => (0..3_u8, 0..o, 0..o, 1..=o).prop_map(|(k, a, b, n)| Op::Elim(k, a, b, n)),
        2 => (0..3_u8, 0..o).prop_map(|(k, a)| Op::Set(k, a)),
        1 => Just(Op::Forcing),
        1 => Just(Op::DiffSimple),
        1 => any::<bool>().prop_map(Op::DiffSet),
    ]
}

/// Applies `op` to both solvers and compares the results and states.
/// Set eliminations C would assert on are skipped on both sides.
fn apply(op: &Op, rust: &mut LatinSolver, c: &mut CLatinSolver) -> Result<(), TestCaseError> {
    let o = rust.order();
    let (r, k) = match *op {
        Op::RuleOut(x, y, n) => {
            rust.rule_out(x, y, n);
            c.view().rule_out(x, y, n);
            (0, 0)
        }
        Op::Elim(kind, a, b, n) => {
            let (start, step) = match kind {
                0 => (rust.cube_pos(0, a, n), o * o),
                1 => (rust.cube_pos(a, 0, n), o),
                _ => (rust.cube_pos(a, b, 1), 1),
            };
            (
                deduction_code(rust.elim(start, step, format_args!("test"))),
                c.elim(start, step),
            )
        }
        Op::Set(kind, a) => {
            let (start, step1, step2) = match kind {
                0 => (rust.cube_pos(0, a, 1), o * o, 1),
                1 => (rust.cube_pos(a, 0, 1), o, 1),
                _ => (rust.cube_pos(0, 0, a + 1), o * o, o),
            };
            if !c.set_would_succeed(start, step1, step2) {
                return Ok(());
            }
            (
                deduction_code(rust.set(start, step1, step2, format_args!("test"))),
                c.set(start, step1, step2),
            )
        }
        Op::Forcing => (deduction_code(rust.forcing()), c.forcing()),
        Op::DiffSimple => (deduction_code(rust.diff_simple()), c.diff_simple()),
        Op::DiffSet(extreme) => {
            let o = rust.order();
            let slices: Vec<(usize, usize, usize)> = if extreme {
                (0..o).map(|n| (n, o * o, o)).collect()
            } else {
                (0..o)
                    .map(|y| (y * o, o * o, 1))
                    .chain((0..o).map(|x| (x * o * o, o, 1)))
                    .collect()
            };
            if !slices.iter().all(|&(s, a, b)| c.set_would_succeed(s, a, b)) {
                return Ok(());
            }
            (deduction_code(rust.diff_set(extreme)), c.diff_set(extreme))
        }
    };
    prop_assert_eq!(r, k, "{:?}", op);
    prop_assert_eq!(rust_state(rust), c_state(c), "{:?}", op);
    Ok(())
}

fn ops_case() -> impl Strategy<Value = (usize, Vec<u8>, Vec<bool>, Vec<Op>)> {
    (2..=7_usize).prop_flat_map(|o| {
        (
            Just(o),
            proptest::collection::vec(any::<u8>(), 0..6),
            proptest::collection::vec(any::<bool>(), 1..64),
            proptest::collection::vec(op(o), 1..40),
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn strategies_match_c((o, seed, keep, ops) in ops_case()) {
        let grid = puzzle(o, &seed, &keep);
        let mut rust = LatinSolver::new(grid.clone(), o);
        let mut c = CLatinSolver::new(&grid, o);
        prop_assert!(rust.is_consistent() && c.is_consistent());
        prop_assert_eq!(rust_state(&rust), c_state(&c));
        for op in &ops {
            apply(op, &mut rust, &mut c)?;
        }
    }

    /// Clashing digits: both stop setting up at the same digit.
    #[test]
    fn setup_matches_c(
        o in 1..=6_usize,
        cells in proptest::collection::vec(0..=6_u8, 36),
    ) {
        let grid: Vec<u8> = cells[..o * o]
            .iter()
            .map(|&d| if usize::from(d) > o { 0 } else { d })
            .collect();
        let rust = LatinSolver::new(grid.clone(), o);
        let c = CLatinSolver::new(&grid, o);
        prop_assert_eq!(rust.is_consistent(), c.is_consistent());
        prop_assert_eq!(rust_state(&rust), c_state(&c));
    }
}

// ---------------------------------------------------------------------
// Full solves with game rules

/// A callback as seen by the test rules, with the context it ran in.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Deduce { ctx: usize, level: usize, ret: i32 },
    Valid { ctx: usize, ret: bool },
    Branch { from: usize, to: usize },
}

/// The test game's clues, shared by every context of one solve.
#[derive(Debug)]
struct Clues {
    /// `(level, cell, digit)`: at `level`, the game deduces that `digit`
    /// can't go at `cell` (a contradiction if it is already there).
    forbidden: Vec<(usize, usize, u8)>,
    /// `(cell, digit)`: a full grid with `digit` at `cell` is invalid.
    reject: Vec<(usize, u8)>,
}

/// Test rules, the same on both sides. Each context has an id; the trace
/// and id counter are shared by all contexts of one solve.
#[derive(Clone)]
struct Rules {
    clues: Rc<Clues>,
    trace: Rc<RefCell<Vec<Event>>>,
    next_id: Rc<Cell<usize>>,
    id: usize,
    branching: bool,
}

/// What the rules need from a solver, on either side.
trait View {
    fn order(&self) -> usize;
    fn possible(&self, x: usize, y: usize, n: usize) -> bool;
    fn knock_out(&mut self, x: usize, y: usize, n: usize);
    fn digit(&self, x: usize, y: usize) -> u8;
}

impl View for LatinSolver {
    fn order(&self) -> usize {
        LatinSolver::order(self)
    }
    fn possible(&self, x: usize, y: usize, n: usize) -> bool {
        self.is_possible(x, y, n)
    }
    fn knock_out(&mut self, x: usize, y: usize, n: usize) {
        self.rule_out(x, y, n);
    }
    fn digit(&self, x: usize, y: usize) -> u8 {
        self.grid()[y * LatinSolver::order(self) + x]
    }
}

impl View for CLatinView<'_> {
    fn order(&self) -> usize {
        CLatinView::order(self)
    }
    fn possible(&self, x: usize, y: usize, n: usize) -> bool {
        self.is_possible(x, y, n)
    }
    fn knock_out(&mut self, x: usize, y: usize, n: usize) {
        self.rule_out(x, y, n);
    }
    fn digit(&self, x: usize, y: usize) -> u8 {
        CLatinView::digit(self, x, y)
    }
}

impl Rules {
    fn new(clues: Rc<Clues>, branching: bool) -> Self {
        Self {
            clues,
            trace: Rc::default(),
            next_id: Rc::new(Cell::new(1)),
            id: 0,
            branching,
        }
    }

    fn deduce_on(&mut self, view: &mut impl View, level: usize) -> i32 {
        let o = view.order();
        let mut ret = 0;
        for &(l, cell, digit) in &self.clues.forbidden {
            if l != level {
                continue;
            }
            let (x, y, n) = (cell % o, cell / o, usize::from(digit));
            if view.digit(x, y) == digit {
                ret = -1;
                break;
            }
            if view.possible(x, y, n) {
                view.knock_out(x, y, n);
                ret += 1;
            }
        }
        self.trace.borrow_mut().push(Event::Deduce {
            ctx: self.id,
            level,
            ret,
        });
        ret
    }

    fn valid_on(&mut self, view: &impl View) -> bool {
        let o = view.order();
        let ret = !self
            .clues
            .reject
            .iter()
            .any(|&(cell, digit)| view.digit(cell % o, cell / o) == digit);
        self.trace
            .borrow_mut()
            .push(Event::Valid { ctx: self.id, ret });
        ret
    }

    fn new_branch(&self) -> Self {
        let to = self.next_id.get();
        self.next_id.set(to + 1);
        self.trace
            .borrow_mut()
            .push(Event::Branch { from: self.id, to });
        Self {
            id: to,
            ..self.clone()
        }
    }
}

impl LatinRules for Rules {
    fn deduce(&mut self, solver: &mut LatinSolver, level: usize) -> Deduction {
        match self.deduce_on(solver, level) {
            0 => Deduction::None,
            r if r < 0 => Deduction::Contradiction,
            _ => Deduction::Progress,
        }
    }

    fn is_valid(&mut self, solver: &mut LatinSolver) -> bool {
        self.valid_on(solver)
    }

    fn branch(&self) -> Option<Self> {
        self.branching.then(|| self.new_branch())
    }
}

impl CLatinRules for Rules {
    fn deduce(&mut self, solver: &mut CLatinView<'_>, level: usize) -> i32 {
        self.deduce_on(solver, level)
    }

    fn is_valid(&mut self, solver: &mut CLatinView<'_>) -> bool {
        self.valid_on(solver)
    }

    fn branch(&self) -> Box<dyn CLatinRules> {
        Box::new(self.new_branch())
    }
}

/// Random difficulty levels: the built-ins at non-decreasing levels from
/// `simple = 0` (as every game has them), `recursive` at least 1.
fn random_levels(rng: &mut RandomState) -> Levels {
    let mut step = || rng.upto(3) as usize;
    let set = step();
    let extreme_set = set + step();
    let forcing = extreme_set.max(set + step());
    let recursive = (forcing + step()).max(1);
    Levels {
        simple: 0,
        set,
        extreme_set,
        forcing,
        recursive,
    }
}

#[derive(Debug, Clone)]
struct SolveCase {
    o: usize,
    grid: Vec<u8>,
    levels: Levels,
    max_diff: usize,
    forbidden: Vec<(usize, usize, u8)>,
    reject: Vec<(usize, u8)>,
    branching: bool,
}

/// A random solve of order `o`: a generated square with a random share
/// of cells blanked, and clues that mostly rule out digits that aren't the
/// solution's (as real clues do), so many puzzles stay solvable but need
/// the harder strategies; the rest are inconsistent on purpose. Small
/// orders also get a validator that rejects some full grids.
fn random_case(o: usize, rng: &mut RandomState) -> SolveCase {
    let levels = random_levels(rng);
    let solution = generate(o, rng);
    let keep = rng.upto(8);
    let grid = solution
        .iter()
        .map(|&d| if rng.upto(10) < keep { d } else { 0 })
        .collect();
    let o32 = u32::try_from(o).expect("small");
    let cells = o32 * o32;
    let levels32 = u32::try_from(levels.recursive).expect("small") + 1;
    let forbidden = (0..rng.upto(3 * o32))
        .map(|_| {
            let level = rng.upto(levels32) as usize;
            let cell = rng.upto(cells) as usize;
            let offset = if rng.upto(8) == 0 {
                0
            } else {
                1 + rng.upto(o32 - 1)
            };
            let digit = (u32::from(solution[cell]) - 1 + offset) % o32 + 1;
            (level, cell, u8::try_from(digit).expect("small"))
        })
        .collect();
    // A rejected full grid can make the search enumerate every square
    // below a guess (validation only happens at the leaves), so keep
    // rejections to orders with few squares (576 at order 4).
    let rejections = if o <= 4 { rng.upto(3) } else { 0 };
    let reject = (0..rejections)
        .map(|_| {
            let cell = rng.upto(cells) as usize;
            (cell, u8::try_from(1 + rng.upto(o32)).expect("small"))
        })
        .collect();
    SolveCase {
        o,
        grid,
        max_diff: rng.upto(levels32) as usize,
        levels,
        forbidden,
        reject,
        branching: rng.upto(2) == 0,
    }
}

/// What one solve did, from the Rust side, for coverage checks.
struct Solved {
    outcome: Outcome,
    log: String,
    trace: Vec<Event>,
}

fn compare_solve(case: &SolveCase) -> Result<Solved, TestCaseError> {
    let clues = Rc::new(Clues {
        forbidden: case.forbidden.clone(),
        reject: case.reject.clone(),
    });
    let mut rust_rules = Rules::new(Rc::clone(&clues), case.branching);
    let c_rules = Rules::new(Rc::clone(&clues), case.branching);
    let c_trace = Rc::clone(&c_rules.trace);
    let l = case.levels;

    // Logging on the Rust side only: it must not change what the solver
    // does, and its text tells the coverage test which paths ran.
    let mut rust = LatinSolver::new(case.grid.clone(), case.o);
    rust.log = SolverLog::new(1);
    let rust_outcome = rust.solve(case.max_diff, &l, &mut rust_rules);

    let mut c = CLatinSolver::new(&case.grid, case.o);
    prop_assert_eq!(rust.is_consistent(), c.is_consistent());
    let c_outcome = if c.is_consistent() {
        let c_levels = CLevels {
            simple: l.simple,
            set: l.set,
            extreme_set: l.extreme_set,
            forcing: l.forcing,
            recursive: l.recursive,
        };
        c.solve(case.max_diff, c_levels, Box::new(c_rules), case.branching)
    } else {
        // What every C caller does when latin_solver_alloc fails.
        C_DIFF_IMPOSSIBLE
    };

    prop_assert_eq!(outcome_code(rust_outcome), c_outcome);
    prop_assert_eq!(rust_state(&rust), c_state(&c));
    let trace = rust_rules.trace.borrow().clone();
    prop_assert_eq!(&trace, &*c_trace.borrow());
    Ok(Solved {
        outcome: rust_outcome,
        log: rust.log.take_text(),
        trace,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn solves_match_c(
        o in 2..=6_usize,
        seed in proptest::collection::vec(any::<u8>(), 0..8),
    ) {
        compare_solve(&random_case(o, &mut RandomState::new(&seed)))?;
    }
}

/// Plain Latin squares at the levels Towers and Keen use (Easy, Hard,
/// Extreme, Unreasonable) on every order up to 9, at every maximum level,
/// with no game rules.
#[test]
fn game_level_solves_match_c() {
    let levels = Levels {
        simple: 0,
        set: 1,
        extreme_set: 2,
        forcing: 2,
        recursive: 3,
    };
    let mut rng = RandomState::new(b"plain");
    for o in 2..=9 {
        for round in 0..20 {
            let keep = rng.upto(8);
            let grid: Vec<u8> = generate(o, &mut rng)
                .into_iter()
                .map(|d| if rng.upto(10) < keep { d } else { 0 })
                .collect();
            for max_diff in 0..=3 {
                compare_solve(&SolveCase {
                    o,
                    grid: grid.clone(),
                    levels,
                    max_diff,
                    forbidden: Vec::new(),
                    reject: Vec::new(),
                    branching: round % 2 == 0,
                })
                .expect("parity");
            }
        }
    }
}

/// Thousands of random solves, checked for parity, and checked to have
/// reached every path of the solver and its plumbing at least once (so
/// the parity tests above aren't passing on easy cases only).
#[test]
fn solves_cover_every_path() {
    let mut rng = RandomState::new(b"coverage");
    let mut outcomes = [0_usize; 4];
    let mut branches = 0;
    let mut log = String::new();
    for i in 0..4000 {
        let case = random_case(2 + i % 6, &mut rng);
        let solved = compare_solve(&case).expect("parity");
        outcomes[match solved.outcome {
            Outcome::Solved { difficulty: 0 } => 0,
            Outcome::Solved { .. } => 1,
            Outcome::Ambiguous => 2,
            Outcome::Impossible | Outcome::Unfinished => 3,
        }] += 1;
        branches += solved
            .trace
            .iter()
            .filter(|e| matches!(e, Event::Branch { .. }))
            .count();
        log.push_str(&solved.log);
    }
    let paths = [
        "positional elimination",
        "numeric elimination",
        "no possibilities available",
        "set elimination, row",
        "set elimination, column",
        "positional set elimination",
        // "contradiction reached" (set elimination's pigeonhole case) is
        // almost always caught earlier in a solve; see
        // `set_contradiction_matches_c`.
        "forcing chain",
        "recursing on",
        "no solution (impossible)",
        "no solution (unfinished)",
        "multiple solutions",
        "failed final validation",
    ];
    let counts: Vec<(&str, usize)> = paths.iter().map(|p| (*p, log.matches(p).count())).collect();
    eprintln!("outcomes {outcomes:?}, branches {branches}, paths {counts:?}");
    assert!(outcomes.iter().all(|&n| n > 0), "outcomes {outcomes:?}");
    assert!(branches > 0, "no guess got its own context");
    for (path, count) in counts {
        assert!(count > 0, "no solve reached {path:?}");
    }
}

/// Set elimination's pigeonhole contradiction: four cells of a row of
/// order 5 can only hold 1, 2 or 3. (The first set tried, digits {4, 5},
/// is avoided by those four cells: more than the 3 left for the others.)
#[test]
fn set_contradiction_matches_c() {
    let o = 5;
    let grid = vec![0; o * o];
    let mut rust = LatinSolver::new(grid.clone(), o);
    rust.log = SolverLog::new(1);
    let mut c = CLatinSolver::new(&grid, o);
    for x in 0..4 {
        for n in 4..=5 {
            rust.rule_out(x, 0, n);
            c.view().rule_out(x, 0, n);
        }
    }
    let start = rust.cube_pos(0, 0, 1);
    let r = rust.set(start, o * o, 1, format_args!("set elimination, row 1"));
    assert_eq!(r, Deduction::Contradiction);
    assert_eq!(c.set(start, o * o, 1), -1);
    assert_eq!(rust_state(&rust), c_state(&c));
    assert_eq!(
        rust.log.text(),
        "set elimination, row 1:\n  contradiction reached\n"
    );
}
