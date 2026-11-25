//! The Latin solver's reasoning text against C's.
//!
//! `fixtures/latin_solver_log.txt` is the stdout of a small C driver built
//! from upstream `latin.c` (616da16) with `-DSTANDALONE_SOLVER`. For each
//! case it set `solver_show_working` to the verbosity, called
//! `latin_solver(grid, o, maxdiff, 0, 1, 2, 2, 3, {NULL x 4}, NULL, NULL,
//! NULL, NULL)`, and printed `=== <label>` before and
//! `=== ret <code> grid <digits>` after. The oracle crate doesn't build C's
//! `STANDALONE_SOLVER` code (Phase 6 adds that), so the text is checked in.
//!
//! The cases cover guessing (with its indentation), multiple solutions,
//! set elimination, a forcing chain, an unfinished solve and the
//! verbosity-2 candidate dumps.

use std::fmt::Write as _;

use puzzles_core::latin::{LatinSolver, Levels, NoRules, Outcome};
use puzzles_core::solver_log::SolverLog;

const LEVELS: Levels = Levels {
    simple: 0,
    set: 1,
    extreme_set: 2,
    forcing: 2,
    recursive: 3,
};

/// Solves like the C driver and returns the same text.
fn run(out: &mut String, label: &str, o: usize, grid: &[u8], max_diff: usize, verbosity: u32) {
    let mut solver = LatinSolver::new(grid.to_vec(), o);
    solver.log = SolverLog::new(verbosity);
    let outcome = solver.solve(max_diff, &LEVELS, &mut NoRules);
    let code = match outcome {
        Outcome::Solved { difficulty } => difficulty,
        Outcome::Impossible => 10,
        Outcome::Ambiguous => 11,
        Outcome::Unfinished => 12,
    };
    writeln!(out, "=== {label}").unwrap();
    out.push_str(solver.log.text());
    write!(out, "=== ret {code} grid").unwrap();
    for digit in solver.grid() {
        write!(out, " {digit}").unwrap();
    }
    out.push('\n');
}

#[test]
fn solver_log_matches_c() {
    #[rustfmt::skip]
    let b = [
        0, 0, 0, 5, 0, 0,  0, 0, 2, 0, 3, 0,  2, 0, 0, 6, 0, 0,
        0, 4, 6, 2, 0, 0,  1, 0, 0, 3, 6, 4,  0, 3, 0, 4, 2, 0,
    ];
    #[rustfmt::skip]
    let c = [
        1, 0, 0, 0, 2, 0,  6, 4, 3, 0, 1, 0,  3, 0, 0, 0, 0, 4,
        4, 0, 0, 6, 3, 0,  0, 6, 1, 0, 0, 0,  5, 3, 0, 1, 0, 0,
    ];
    let mut out = String::new();
    run(&mut out, "a", 3, &[1, 0, 0, 0, 0, 1, 0, 0, 0], 3, 2);
    run(&mut out, "b", 6, &b, 2, 1);
    run(&mut out, "c1", 6, &c, 1, 1);
    run(&mut out, "c3", 6, &c, 3, 1);

    let expected = include_str!("fixtures/latin_solver_log.txt");
    // Compare line by line first, for a readable failure.
    for (i, (got, want)) in out.lines().zip(expected.lines()).enumerate() {
        assert_eq!(got, want, "line {}", i + 1);
    }
    assert_eq!(out, expected);
}
