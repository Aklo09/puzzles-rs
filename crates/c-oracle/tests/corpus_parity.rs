//! The golden corpus is C's: the snapshots `puzzles-games` checks itself
//! against (`puzzles-games/tests/snapshots/corpus__<game>.snap`) must
//! equal the same corpus built from the C code (see
//! `puzzles-games/tests/corpus.rs` for the format).

#![cfg(feature = "oracle")]

use std::fmt::Write as _;

use c_oracle::{CGame, CSession};

/// Seeds per preset, as in `puzzles-games/tests/corpus.rs`.
const SEEDS: u32 = 10;

fn c_corpus(game: CGame) -> String {
    let mut out = String::new();
    for (_, encoded) in game.presets() {
        for seed in 0..SEEDS {
            let (desc, aux) = game.new_desc(&encoded, seed.to_string().as_bytes(), false);
            let mut session = CSession::new(game, &encoded, &desc, 48);
            let solve = match session.solve(aux.as_deref()) {
                Ok(mv) => mv,
                Err(e) => format!("error: {e}"),
            };
            let _ = writeln!(out, "{encoded}#{seed}");
            let _ = writeln!(out, "desc: {desc}");
            let _ = writeln!(out, "aux: {}", aux.as_deref().unwrap_or("-"));
            let _ = writeln!(out, "solve: {solve}");
            match session.text_format() {
                Some(text) => {
                    out += "text:\n";
                    for line in text.lines() {
                        let _ = writeln!(out, "  {line}");
                    }
                }
                None => out += "text: -\n",
            }
        }
    }
    out
}

/// A snapshot's content, after insta's header.
fn snapshot(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../puzzles-games/tests/snapshots/corpus__{name}.snap"
    ));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    let body = text
        .splitn(3, "---\n")
        .nth(2)
        .unwrap_or_else(|| panic!("{}: no insta header", path.display()));
    body.to_owned()
}

fn check(name: &str, game: CGame) {
    assert_eq!(
        snapshot(name).trim_end(),
        c_corpus(game).trim_end(),
        "the {name} corpus snapshot isn't C's"
    );
}

#[test]
fn fifteen_corpus_is_cs() {
    check("fifteen", CGame::fifteen());
}

#[test]
fn flip_corpus_is_cs() {
    check("flip", CGame::flip());
}

#[test]
fn guess_corpus_is_cs() {
    check("guess", CGame::guess());
}

#[test]
fn mosaic_corpus_is_cs() {
    check("mosaic", CGame::mosaic());
}
