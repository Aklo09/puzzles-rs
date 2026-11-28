//! Differential tests: the Rust `unruly` module against C's `unruly.c`.
//!
//! * Presets, and parameter decoding, encoding and validation of
//!   arbitrary strings (encoding only with known difficulty letters: C
//!   reads past its table for an unknown one).
//! * Generation: every preset over several seeds, and random sizes,
//!   difficulties and unique-rows modes, give C's description; mutated
//!   descriptions validate alike.
//! * Play: random scripts of clicks with every button (with modifiers,
//!   which Unruly strips), drags and releases, arrows, selects, the `0`,
//!   `1`, `2` and backspace keys (also on the keypad), solver calls and
//!   raw move strings, compared step by step by the shared driver: move
//!   strings, states, key labels, every flash frame's draw trace.
//! * Winning by clicks, with the completion flash.
//! * The Custom dialog through both mid-ends.

#![cfg(feature = "oracle")]

mod common;

use std::fmt::Write as _;

use c_oracle::{CGame, CMidend};
use common::{Pair, Step};
use proptest::prelude::*;
use puzzles_core::config::ConfigValue;
use puzzles_core::game::{Game, GameStatus};
use puzzles_core::midend::{ConfigKind, Midend, MidendOptions};
use puzzles_core::random::RandomState;
use puzzles_games::unruly::{Difficulty, Move, Params, Unruly};

fn rust_desc(p: &Params, seed: &[u8], interactive: bool) -> String {
    let new = Unruly::new_desc(p, &mut RandomState::new(seed), interactive);
    assert_eq!(new.aux, None, "Unruly has no aux data");
    new.desc
}

// ---------------------------------------------------------------------
// Parameters

#[test]
fn presets_match() {
    let rust: Vec<_> = Unruly::presets()
        .presets()
        .map(|(name, p)| (name.to_owned(), Unruly::encode_params(p, true)))
        .collect();
    assert_eq!(rust, CGame::unruly().presets());
    assert_eq!(rust.len(), 7);
}

proptest! {
    /// Digit runs of at most 9 digits (see flip_parity.rs); a `d` is
    /// always followed by a known letter, and nothing after the
    /// parameters can be read as one.
    #[test]
    fn params_decode_encode_validate_match(
        s in "([0-9]{0,9}[xu +-]){0,3}[0-9]{0,9}u?(d[ten])?([abcfz,:#][a-z0-9,:#]{0,2})?",
        full: bool,
    ) {
        let c = CGame::unruly();
        let mut p = Unruly::default_params();
        Unruly::decode_params(&mut p, &s);
        prop_assert_eq!(Unruly::encode_params(&p, full), c.encode_params(&s, full));
        let rust = Unruly::validate_params(&p, full).map_err(|e| e.message().to_owned());
        prop_assert_eq!(rust, c.validate_params(&s, full));
    }

    /// Any difficulty letter, valid or not: the same verdict.
    #[test]
    fn unknown_difficulties_validate_alike(
        s in "[0-9]{1,3}(x[0-9]{1,3})?u?d[ -~]?",
        full: bool,
    ) {
        let mut p = Unruly::default_params();
        Unruly::decode_params(&mut p, &s);
        let rust = Unruly::validate_params(&p, full).map_err(|e| e.message().to_owned());
        prop_assert_eq!(rust, CGame::unruly().validate_params(&s, full));
    }
}

// ---------------------------------------------------------------------
// Generation

#[test]
fn presets_generate_c_descriptions() {
    let c = CGame::unruly();
    for (_, p) in Unruly::presets().presets() {
        let encoded = Unruly::encode_params(p, true);
        let seeds = if p.w2 > 10 { 5 } else { 20 };
        for seed in 0..seeds {
            let seed = format!("{seed}");
            let (c_desc, c_aux) = c.new_desc(&encoded, seed.as_bytes(), true);
            assert_eq!(c_aux, None);
            assert_eq!(
                rust_desc(p, seed.as_bytes(), true),
                c_desc,
                "{encoded} seed {seed}"
            );
        }
    }
}

fn any_params(max: i32) -> impl Strategy<Value = Params> {
    (
        3..=max / 2,
        3..=max / 2,
        any::<bool>(),
        prop::sample::select(vec![
            Difficulty::Trivial,
            Difficulty::Easy,
            Difficulty::Normal,
        ]),
    )
        .prop_map(|(w, h, unique, diff)| Params {
            w2: 2 * w,
            h2: 2 * h,
            unique,
            diff: Some(diff),
        })
        .prop_filter("valid", |p| Unruly::validate_params(p, true).is_ok())
}

fn seed() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..12)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    #[test]
    fn random_params_generate_c_descriptions(p in any_params(12), seed in seed(), interactive: bool) {
        let encoded = Unruly::encode_params(&p, true);
        let (c_desc, _) = CGame::unruly().new_desc(&encoded, &seed, interactive);
        prop_assert_eq!(rust_desc(&p, &seed, interactive), c_desc);
    }

    #[test]
    fn mutated_descriptions_validate_alike(
        p in any_params(10),
        seed in seed(),
        cut in any::<prop::sample::Index>(),
        edit in 0..4_u8,
        ch in prop::sample::select(vec!['a', 'y', 'z', 'A', 'Y', 'Z', '0', ' ', '{']),
    ) {
        let mut desc = rust_desc(&p, &seed, false);
        let at = cut.index(desc.len() + 1);
        match edit {
            0 => desc.truncate(at),
            1 => desc.insert(at, ch),
            2 if at < desc.len() => desc.replace_range(at..=at, ch.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        let rust = Unruly::validate_desc(&p, &desc).map_err(|e| e.message().to_owned());
        let encoded = Unruly::encode_params(&p, true);
        prop_assert_eq!(rust, CGame::unruly().validate_desc(&encoded, &desc), "{}", desc);
    }
}

// ---------------------------------------------------------------------
// Play

/// Button codes as the mid-end passes them on, with any modifiers
/// (Unruly strips them all).
fn button() -> impl Strategy<Value = i32> {
    let mods = prop::sample::select(vec![0, 0, 0, 0x1000, 0x2000, 0x4000, 0x0800]);
    let base = prop_oneof![
        6 => 0x200..=0x208,
        3 => 0x209..=0x20c,
        2 => prop::sample::select(vec![0x20d, 0x20e]),
        3 => prop::sample::select(b"0120\x08a".map(i32::from).to_vec()),
    ];
    (base, mods).prop_map(|(b, m)| b | m)
}

/// A script for a `w2` × `h2` grid at `tilesize`.
fn steps(w2: i32, h2: i32, tilesize: i32) -> impl Strategy<Value = Vec<Step>> {
    let (pw, ph) = (tilesize * (w2 + 1), tilesize * (h2 + 1));
    let s = (w2 * h2) as usize;
    let input = (-tilesize..pw + tilesize, -tilesize..ph + tilesize, button())
        .prop_map(|(x, y, button)| Step::Input { x, y, button });
    let place = (
        prop::sample::select(vec!['0', '1', '-', '2', ' ']),
        -1..=w2,
        -1..=h2,
        "[ ,x]?",
    )
        .prop_map(|(c, x, y, junk)| Step::Execute(format!("P{c},{x},{y}{junk}")));
    let solve_text = prop::collection::vec(
        prop::sample::select(vec!['0', '1', '1', '0', '2']),
        s - 1..=s + 1,
    )
    .prop_map(|v| Step::Execute(format!("S{}", v.into_iter().collect::<String>())));
    let junk = "[PSx0-9, -]{0,8}".prop_map(Step::Execute);
    let step = prop_oneof![
        14 => input,
        1 => Just(Step::Solve),
        4 => place,
        1 => solve_text,
        1 => junk,
    ];
    prop::collection::vec(step, 0..60)
}

fn game() -> impl Strategy<Value = (Params, Vec<u8>, i32, Vec<Step>)> {
    (any_params(10), seed(), 1..=48_i32).prop_flat_map(|(p, seed, tilesize)| {
        (
            Just(p),
            Just(seed),
            Just(tilesize),
            steps(p.w2, p.h2, tilesize),
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn play_matches((p, seed, tilesize, steps) in game()) {
        let desc = rust_desc(&p, &seed, true);
        let mut pair = Pair::<Unruly>::new(CGame::unruly(), &p, &desc, tilesize)?;
        pair.run(&steps)?;
    }
}

/// A whole game won by clicking the solution in (a left click for
/// black, a right click for white), ending with the completion flash;
/// a mistake first, so error marks show and go.
#[test]
fn winning_by_clicks_matches_c() {
    let ts = 32;
    for (_, p) in Unruly::presets().presets().filter(|(_, p)| p.w2 <= 10) {
        for seed in 0..4 {
            let desc = rust_desc(p, format!("win {seed}").as_bytes(), true);
            let mut pair = Pair::<Unruly>::new(CGame::unruly(), p, &desc, ts).expect("same start");
            let Ok(Move::Solve(ones)) = Unruly::solve(&pair.rust.orig, &pair.rust.orig, None)
            else {
                panic!("{p:?}: no solution");
            };
            let at = |i: usize| {
                let (x, y) = (i as i32 % p.w2, i as i32 / p.w2);
                (ts / 2 + ts * x + 5, ts / 2 + ts * y + 5)
            };
            let mut steps = Vec::new();
            // Every square black first, then the solution.
            for i in 0..ones.len() {
                let (x, y) = at(i);
                steps.push(Step::Input {
                    x,
                    y,
                    button: 0x200,
                });
            }
            for (i, &one) in ones.iter().enumerate() {
                let (x, y) = at(i);
                // Black → white is a left click; white → empty → black
                // two right clicks... so clear with the middle button,
                // then set.
                steps.push(Step::Input {
                    x,
                    y,
                    button: 0x201,
                });
                steps.push(Step::Input {
                    x,
                    y,
                    button: if one { 0x200 } else { 0x202 },
                });
            }
            pair.run(&steps).expect("same play");
            assert_eq!(Unruly::status(&pair.rust.cur), GameStatus::Won, "{p:?}");
        }
    }
}

// ---------------------------------------------------------------------
// The Custom dialog

#[test]
fn custom_dialog_matches_c() {
    for values in [
        ["8", "10", "1", "2"],
        ["6", "16", "1", "0"],
        ["7", "8", "0", "1"],
        ["12", "6", "0", "3"],
        [" 10x", "+6", "0", "1"],
    ] {
        let mut c = CMidend::new(CGame::unruly(), b"dialog", true);
        let mut r = Midend::<Unruly>::new(b"dialog", true, MidendOptions::default());
        let config = r.get_config(ConfigKind::Settings).expect("a Custom dialog");
        let mut text = format!("{}\n", config.title);
        for item in &config.items {
            let value = match &item.value {
                ConfigValue::String(v) => format!("string|{v}"),
                ConfigValue::Bool(b) => format!("bool|{}", u8::from(*b)),
                ConfigValue::Choices {
                    names, selected, ..
                } => format!("choices|{selected}|{}", names.join(",")),
            };
            let _ = writeln!(text, "{}|-|{value}", item.name);
        }
        assert_eq!(Some(text), c.get_config(0));

        let mut items = config.items;
        for (item, v) in items.iter_mut().zip(values) {
            match &mut item.value {
                ConfigValue::String(s) => *s = v.to_owned(),
                ConfigValue::Bool(b) => *b = v == "1",
                ConfigValue::Choices { selected, .. } => *selected = v.parse().unwrap(),
            }
        }
        let r_ok = r
            .set_config(ConfigKind::Settings, &items)
            .map_err(|e| e.message().to_owned());
        assert_eq!(r_ok, c.set_config(0, &values), "{values:?}");
        if r_ok.is_ok() {
            r.new_game();
            c.new_game();
            assert_eq!(r.get_game_id(), c.get_game_id(), "{values:?}");
        }
    }
}
