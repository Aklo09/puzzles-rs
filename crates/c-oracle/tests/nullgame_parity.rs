//! Differential tests: the Rust `nullgame` module against C's
//! `nullgame.c`, the template game that does nothing. Mostly a check of
//! the game harness itself on the smallest possible game.

#![cfg(feature = "oracle")]

mod common;

use c_oracle::CGame;
use common::{Pair, Step};
use puzzles_core::game::Game;
use puzzles_core::random::RandomState;
use puzzles_games::nullgame::NullGame;

#[test]
fn params_and_generation_match() {
    let c = CGame::nullgame();
    for s in ["", "FIXME", "5x5"] {
        for full in [false, true] {
            assert_eq!(NullGame::encode_params(&(), full), c.encode_params(s, full));
            assert_eq!(
                NullGame::validate_params(&(), full).is_ok(),
                c.validate_params(s, full).is_ok()
            );
        }
    }
    assert_eq!(c.presets(), []);
    assert!(NullGame::presets().is_empty());
    let new = NullGame::new_desc(&(), &mut RandomState::new(b"1"), true);
    assert_eq!((new.desc, new.aux), c.new_desc("", b"1", true));
    assert_eq!(
        NullGame::validate_desc(&(), "x").is_ok(),
        c.validate_desc("", "x").is_ok()
    );
}

/// Every input is unused (C returns NULL, which the mid-end reads as
/// `MOVE_UNUSED`), nothing is drawn and no move string is accepted.
#[test]
fn nothing_happens_alike() {
    let mut pair = Pair::<NullGame>::new(CGame::nullgame(), &(), "FIXME", 20).expect("same start");
    let mut steps: Vec<Step> = [0x200, 0x203, 0x206, 0x209, 0x20d, i32::from(b'a')]
        .into_iter()
        .map(|button| Step::Input {
            x: 15,
            y: 25,
            button,
        })
        .collect();
    steps.extend(["", "M1,1", "S"].map(|s| Step::Execute(s.to_owned())));
    pair.run(&steps).expect("same play");
}
