//! Project automation, run as `cargo xtask <command>`.
//!
//! Plain Rust with no dependencies, so it behaves the same on Windows,
//! macOS and Linux, locally and in CI.

use std::env;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <command>

Commands:
  ci [--no-oracle]   Run every check CI runs: formatting, clippy (warnings
                     are errors) and all tests. --no-oracle skips the C
                     reference tests, for machines without a C compiler or
                     the third_party/puzzles-c submodule.
  ci --build-only    Build what `ci` builds (clippy, and the tests without
                     running them), to fill CI's cache on main.
  help               Show this message.";

/// What `ci` does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Every check, the C reference tests included.
    Full,
    /// Every check but the C reference tests.
    NoOracle,
    /// Clippy and building the tests, with `Full`'s flags, so their
    /// build is the one `Full` reuses.
    BuildOnly,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["ci"] => ci(Mode::Full),
        ["ci", "--no-oracle"] => ci(Mode::NoOracle),
        ["ci", "--build-only"] => ci(Mode::BuildOnly),
        [] | ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("unknown arguments: {}\n\n{USAGE}", args.join(" "));
            ExitCode::FAILURE
        }
    }
}

/// Runs the CI checks in order, stopping at the first failure.
fn ci(mode: Mode) -> ExitCode {
    // With the oracle, enable every feature (the C reference included);
    // without it, the default features build and test everything else.
    let features: &[&str] = if mode == Mode::NoOracle {
        &[]
    } else {
        &["--all-features"]
    };
    let no_run: &[&str] = if mode == Mode::BuildOnly {
        &["--no-run"]
    } else {
        &[]
    };

    let steps: [(&str, Vec<&str>); 3] = [
        ("format", vec!["fmt", "--all", "--check"]),
        (
            "clippy",
            [
                &["clippy", "--workspace", "--all-targets", "--locked"][..],
                features,
                &["--", "-D", "warnings"],
            ]
            .concat(),
        ),
        (
            "tests",
            [&["test", "--workspace", "--locked"][..], features, no_run].concat(),
        ),
    ];

    for (name, args) in &steps {
        if mode == Mode::BuildOnly && *name == "format" {
            continue;
        }
        println!("\n==> xtask ci: {name}: cargo {}", args.join(" "));
        if !cargo(args) {
            eprintln!("\nxtask ci: {name} failed");
            return ExitCode::FAILURE;
        }
    }
    println!("\nxtask ci: all checks passed");
    ExitCode::SUCCESS
}

/// Runs the cargo that invoked us with `args`; true on success.
fn cargo(args: &[&str]) -> bool {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    match Command::new(cargo).args(args).status() {
        Ok(status) => status.success(),
        Err(err) => {
            eprintln!("failed to run cargo: {err}");
            false
        }
    }
}
