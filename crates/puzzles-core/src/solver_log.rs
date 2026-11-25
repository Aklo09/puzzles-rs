//! The solvers' account of their reasoning, replacing C's
//! `solver_show_working` and `solver_recurse_depth` globals.
//!
//! In C this text exists only in the `STANDALONE_SOLVER` builds (the
//! `<game>solver -v` tools) and goes straight to stdout. Here every solver
//! takes it as data: a quiet log (the default) records nothing and costs
//! one branch per message, and `puzzles-cli` prints what a verbose one
//! collected. Lines are indented by 4 spaces per level of recursion, as C
//! indents them.

use std::fmt::{self, Write as _};

/// Collects a solver's reasoning, if asked to.
///
/// ```
/// use puzzles_core::solver_log::SolverLog;
///
/// let mut log = SolverLog::new(1);
/// log.line(format_args!("recursing on (1,1)"));
/// log.nested(|log| log.line(format_args!("guessing 2 at (1,1)")));
/// assert_eq!(log.text(), "recursing on (1,1)\n    guessing 2 at (1,1)\n");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SolverLog {
    verbosity: u32,
    depth: usize,
    text: String,
}

impl SolverLog {
    /// A log at the given verbosity (C's `solver_show_working`): 0 records
    /// nothing, 1 records the reasoning, and higher levels add detail such
    /// as the Latin solver's full candidate dumps.
    #[must_use]
    pub fn new(verbosity: u32) -> Self {
        Self {
            verbosity,
            ..Self::default()
        }
    }

    /// A log that records nothing (the same as `SolverLog::default()`).
    #[must_use]
    pub fn quiet() -> Self {
        Self::default()
    }

    /// The verbosity this log was created with.
    #[must_use]
    pub fn verbosity(&self) -> u32 {
        self.verbosity
    }

    /// True if messages are recorded (verbosity 1 or more). Callers check
    /// this before doing work that only serves the log.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.verbosity > 0
    }

    /// The current recursion depth (C's `solver_recurse_depth`).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Records one line, indented for the current depth.
    pub fn line(&mut self, args: fmt::Arguments<'_>) {
        if self.enabled() {
            self.indent();
            self.write(args);
            self.text.push('\n');
        }
    }

    /// Records `args` as they are, with no indent or newline added.
    pub fn raw(&mut self, args: fmt::Arguments<'_>) {
        if self.enabled() {
            self.write(args);
        }
    }

    /// Records the indent for the current depth, for a line assembled
    /// piece by piece with [`raw`](Self::raw).
    pub fn indent(&mut self) {
        if self.enabled() {
            let width = self.depth * 4;
            self.text.extend(std::iter::repeat_n(' ', width));
        }
    }

    /// Runs `f` one recursion level deeper.
    pub fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }

    /// Everything recorded so far.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Takes the recorded text, leaving the log empty.
    #[must_use]
    pub fn take_text(&mut self) -> String {
        std::mem::take(&mut self.text)
    }

    fn write(&mut self, args: fmt::Arguments<'_>) {
        self.text
            .write_fmt(args)
            .expect("writing to a String cannot fail");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_log_records_nothing() {
        let mut log = SolverLog::quiet();
        log.line(format_args!("hidden"));
        log.indent();
        log.raw(format_args!("hidden"));
        log.nested(|log| log.line(format_args!("hidden")));
        assert_eq!(log.text(), "");
        assert!(!log.enabled());
    }

    #[test]
    fn lines_are_indented_by_depth() {
        let mut log = SolverLog::new(2);
        log.line(format_args!("a"));
        log.nested(|log| {
            log.line(format_args!("b {}", 1));
            log.nested(|log| {
                log.indent();
                log.raw(format_args!("c"));
                log.raw(format_args!("d\n"));
            });
        });
        log.line(format_args!("e"));
        assert_eq!(log.text(), "a\n    b 1\n        cd\ne\n");
        assert_eq!(log.depth(), 0);
        assert_eq!(log.verbosity(), 2);
        assert_eq!(log.take_text(), "a\n    b 1\n        cd\ne\n");
        assert_eq!(log.text(), "");
    }
}
