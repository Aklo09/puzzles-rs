//! The golden corpus (`docs/porting-plan.md` §6.2): for every preset of
//! every game and seeds 0 to 9, the puzzle generated from `params#seed`
//! (non-interactively, as C's batch mode), its aux data, the solver's
//! move from the start, and the start as text.
//!
//! The snapshots (`snapshots/corpus__<game>.snap`) are C's output:
//! `c-oracle/tests/corpus_parity.rs` builds the same text from the C code
//! and fails unless the checked-in snapshot equals it. So this test needs
//! no C compiler, yet checks generation against C on every OS. Accept a
//! new snapshot with `cargo insta test --review`, then run the oracle
//! test.

use std::fmt::Write as _;

use puzzles_core::game::{Game, GameCtx};
use puzzles_core::random::RandomState;

/// Seeds per preset.
const SEEDS: u32 = 10;

fn corpus<G: Game>() -> String {
    let mut out = String::new();
    for (_, params) in G::presets().presets() {
        let encoded = G::encode_params(params, true);
        for seed in 0..SEEDS {
            let new = G::new_desc(
                params,
                &mut RandomState::new(seed.to_string().as_bytes()),
                false,
            );
            let state = G::new_game(&mut GameCtx::new(), params, &new.desc);
            let solve = match G::solve(&state, &state, new.aux.as_deref()) {
                Ok(mv) => {
                    let mut s = String::new();
                    G::encode_move(&mv, &mut s);
                    s
                }
                Err(e) => format!("error: {e}"),
            };
            let _ = writeln!(out, "{encoded}#{seed}");
            let _ = writeln!(out, "desc: {}", new.desc);
            let _ = writeln!(out, "aux: {}", new.aux.as_deref().unwrap_or("-"));
            let _ = writeln!(out, "solve: {solve}");
            match G::can_format_as_text_now(params)
                .then(|| G::text_format(&state))
                .flatten()
            {
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

#[cfg(feature = "fifteen")]
#[test]
fn fifteen() {
    insta::assert_snapshot!("fifteen", corpus::<puzzles_games::fifteen::Fifteen>());
}

#[cfg(feature = "flip")]
#[test]
fn flip() {
    insta::assert_snapshot!("flip", corpus::<puzzles_games::flip::Flip>());
}

#[cfg(feature = "guess")]
#[test]
fn guess() {
    insta::assert_snapshot!("guess", corpus::<puzzles_games::guess::Guess>());
}

#[cfg(feature = "mosaic")]
#[test]
fn mosaic() {
    insta::assert_snapshot!("mosaic", corpus::<puzzles_games::mosaic::Mosaic>());
}
