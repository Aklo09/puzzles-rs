//! Command-line tools for the puzzle collection.
//!
//! Replaces the C project's batch modes: `generate` (game IDs),
//! `solve`, and `roundtrip` (C's `fuzzpuzz`). Grading, the C standalone
//! solvers' output and benchmarks come in Phase 6.

use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Command-line tools for the puzzle collection")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print descriptive game IDs (params:desc), one per line.
    Generate {
        /// The game, such as "flip".
        game: String,
        /// Parameters ("4x4r"), a seed ID ("4x4r#123") or a descriptive
        /// ID; default: the game's default parameters.
        id: Option<String>,
        /// How many games (with fresh random seeds).
        #[arg(short, default_value_t = 1)]
        n: usize,
    },
    /// Solve games: print the solved position as text, or else the solve
    /// move.
    Solve {
        /// The game.
        game: String,
        /// Game IDs; read from stdin, one per line, if none.
        ids: Vec<String>,
        /// Print the solve move even if the game has a text form.
        #[arg(long = "move")]
        move_only: bool,
    },
    /// Load a save file, redraw it and write it back (C's fuzzpuzz).
    Roundtrip {
        /// The save file; stdin if none.
        file: Option<PathBuf>,
    },
}

fn run(cli: Cli) -> anyhow::Result<bool> {
    let mut out = io::stdout().lock();
    match cli.command {
        Command::Generate { game, id, n } => {
            let game = puzzles_cli::find_game(&game)?;
            let seed: [u8; 16] = rand::random();
            for id in puzzles_cli::generate(game, id.as_deref(), n, &seed)? {
                writeln!(out, "{id}")?;
            }
            Ok(true)
        }
        Command::Solve {
            game,
            ids,
            move_only,
        } => {
            let game = puzzles_cli::find_game(&game)?;
            let ids = if ids.is_empty() {
                io::stdin().lock().lines().collect::<Result<_, _>>()?
            } else {
                ids
            };
            let mut ok = true;
            for id in ids.iter().map(|s| s.trim_end()).filter(|s| !s.is_empty()) {
                match puzzles_cli::solve(game, id, move_only) {
                    Ok(text) => out.write_all(text.as_bytes())?,
                    Err(e) => {
                        eprintln!("{e}");
                        ok = false;
                    }
                }
            }
            Ok(ok)
        }
        Command::Roundtrip { file } => {
            let input = if let Some(path) = &file {
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?
            } else {
                let mut buf = Vec::new();
                io::stdin().read_to_end(&mut buf)?;
                buf
            };
            out.write_all(&puzzles_cli::roundtrip(&input)?)?;
            Ok(true)
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}
