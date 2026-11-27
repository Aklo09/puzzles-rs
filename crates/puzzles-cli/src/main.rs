//! Command-line tools for the puzzle collection.
//!
//! Replaces the C project's batch modes: `generate` (game IDs),
//! `solve`, and `roundtrip` (C's `fuzzpuzz`); `render` draws a position
//! as a PNG. Grading, the C standalone
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
    /// Draw a new game, or a save file's position, as a PNG.
    Render {
        /// The game.
        #[arg(required_unless_present = "load", conflicts_with = "load")]
        game: Option<String>,
        /// Parameters, a seed ID or a descriptive ID; default: a random
        /// game with the default parameters.
        id: Option<String>,
        /// Draw the current position of this save file instead.
        #[arg(long, value_name = "FILE.sav")]
        load: Option<PathBuf>,
        /// Where to write the PNG ("-" for stdout); default: <game>.png.
        #[arg(short, value_name = "FILE")]
        o: Option<PathBuf>,
        /// The tile size; default: the game's (or <GAME>_TILESIZE).
        #[arg(long, conflicts_with = "size")]
        tilesize: Option<i32>,
        /// Fit into this many pixels, as large as fits.
        #[arg(long, value_name = "WxH", value_parser = parse_size)]
        size: Option<(i32, i32)>,
        /// The device pixel ratio.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// The theme.
        #[arg(long, default_value = "classic")]
        theme: String,
    },
}

/// Parses `WxH`.
fn parse_size(text: &str) -> Result<(i32, i32), String> {
    let (w, h) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("{text:?} isn't WxH"))?;
    let number = |s: &str| s.trim().parse::<i32>().map_err(|e| format!("{s:?}: {e}"));
    Ok((number(w)?, number(h)?))
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
        Command::Render {
            game,
            id,
            load,
            o,
            tilesize,
            size,
            scale,
            theme,
        } => {
            let options = puzzles_cli::RenderOptions {
                tilesize,
                fit: size,
                scale,
                theme,
            };
            let seed: [u8; 16] = rand::random();
            let (png, name) = if let Some(path) = &load {
                let input =
                    std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
                let png = puzzles_cli::render(puzzles_cli::RenderSource::Save(&input), &options)?;
                let name = path
                    .file_stem()
                    .map_or_else(|| "puzzle".to_owned(), |s| s.to_string_lossy().into_owned());
                (png, name)
            } else {
                let game = puzzles_cli::find_game(game.as_deref().unwrap_or_default())?;
                let source = puzzles_cli::RenderSource::Game {
                    game,
                    id: id.as_deref(),
                    seed: &seed,
                };
                let name: String = game
                    .name
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    .to_lowercase();
                (puzzles_cli::render(source, &options)?, name)
            };
            match o {
                Some(path) if path.as_os_str() == "-" => out.write_all(&png)?,
                path => {
                    let path = path.unwrap_or_else(|| PathBuf::from(format!("{name}.png")));
                    std::fs::write(&path, &png)
                        .with_context(|| format!("writing {}", path.display()))?;
                }
            }
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
