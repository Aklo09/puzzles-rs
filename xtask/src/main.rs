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
  help               Show this message.";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["ci"] => ci(true),
        ["ci", "--no-oracle"] => ci(false),
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
fn ci(with_oracle: bool) -> ExitCode {
    // With the oracle, enable every feature (the C reference included);
    // without it, the default features build and test everything else.
    let features: &[&str] = if with_oracle {
        &["--all-features"]
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
            [&["test", "--workspace", "--locked"][..], features].concat(),
        ),
    ];

    for (name, args) in &steps {
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
