//! Differential tests: the Rust `fifteen` module against C's
//! `fifteen.c`.
//!
//! * Presets, and parameter decoding, encoding and validation of
//!   arbitrary strings.
//! * Generation: the preset over many seeds and random sizes give C's
//!   description.
//! * Description validation of mutated descriptions: same verdict and
//!   message.
//! * Play: random scripts of clicks, arrows (with modifiers, which the
//!   game strips), hint keys, solver calls and raw move strings, compared
//!   step by step by the shared driver (`common/mod.rs`): move strings,
//!   states, animation and flash lengths, every frame's draw trace and
//!   the status bar.
//! * The "Sense of arrow keys" preference through both mid-ends.

#![cfg(feature = "oracle")]

mod common;

use c_oracle::{CGame, CMidend};
use common::{Pair, Step};
use proptest::prelude::*;
use puzzles_core::button::Button;
use puzzles_core::game::Game;
use puzzles_core::midend::{Midend, MidendOptions};
use puzzles_core::random::RandomState;
use puzzles_games::fifteen::{Fifteen, Params};

fn rust_desc(p: Params, seed: &[u8], interactive: bool) -> String {
    let new = Fifteen::new_desc(&p, &mut RandomState::new(seed), interactive);
    assert_eq!(new.aux, None);
    new.desc
}

#[test]
fn presets_match() {
    let rust: Vec<_> = Fifteen::presets()
        .presets()
        .map(|(name, p)| (name.to_owned(), Fifteen::encode_params(p, true)))
        .collect();
    assert_eq!(rust, CGame::fifteen().presets());
}

proptest! {
    #[test]
    fn params_decode_encode_validate_match(
        s in "([0-9]{0,9}[x +-]){0,3}[0-9]{0,9}([a-z,:#][a-z0-9,:#]{0,2})?",
        full: bool,
    ) {
        let c = CGame::fifteen();
        let mut p = Fifteen::default_params();
        Fifteen::decode_params(&mut p, &s);
        prop_assert_eq!(Fifteen::encode_params(&p, full), c.encode_params(&s, full));
        let rust = Fifteen::validate_params(&p, full).map_err(|e| e.message().to_owned());
        prop_assert_eq!(rust, c.validate_params(&s, full));
    }
}

fn any_params(max: i32) -> impl Strategy<Value = Params> {
    (2..=max, 2..=max).prop_map(|(w, h)| Params { w, h })
}

fn seed() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..12)
}

#[test]
fn preset_generates_c_descriptions() {
    let c = CGame::fifteen();
    for seed in 0..50 {
        let seed = format!("{seed}");
        let (c_desc, _) = c.new_desc("4x4", seed.as_bytes(), true);
        assert_eq!(
            rust_desc(Fifteen::default_params(), seed.as_bytes(), true),
            c_desc
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_sizes_generate_c_descriptions(p in any_params(9), seed in seed(), interactive: bool) {
        let encoded = Fifteen::encode_params(&p, true);
        let (c_desc, _) = CGame::fifteen().new_desc(&encoded, &seed, interactive);
        prop_assert_eq!(rust_desc(p, &seed, interactive), c_desc);
    }

    #[test]
    fn mutated_descriptions_validate_alike(
        p in any_params(4),
        seed in seed(),
        cut in any::<prop::sample::Index>(),
        edit in 0..4_u8,
        ch in prop::sample::select(vec!['0', '1', '9', ',', ' ', 'x']),
    ) {
        let mut desc = rust_desc(p, &seed, false);
        let at = cut.index(desc.len() + 1);
        match edit {
            0 => desc.truncate(at),
            1 => desc.insert(at, ch),
            2 if at < desc.len() => desc.replace_range(at..=at, ch.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        let rust = Fifteen::validate_desc(&p, &desc).map_err(|e| e.message().to_owned());
        let encoded = Fifteen::encode_params(&p, true);
        prop_assert_eq!(rust, CGame::fifteen().validate_desc(&encoded, &desc), "{}", desc);
    }
}

/// Buttons as the mid-end passes them on; Fifteen strips modifiers.
fn button() -> impl Strategy<Value = i32> {
    let mouse = 0x200..=0x208;
    let arrows =
        (0x209..=0x20c, prop::sample::select(vec![0, 0x1000, 0x2000])).prop_map(|(b, m)| b | m);
    prop_oneof![
        3 => mouse,
        4 => arrows,
        2 => prop::sample::select(vec![i32::from(b'h'), i32::from(b'H')]),
        1 => prop::sample::select(vec![0x20d, 0x20e, i32::from(b'a'), i32::from(b'1')]),
    ]
}

fn steps(w: i32, h: i32, tilesize: i32) -> impl Strategy<Value = Vec<Step>> {
    let (pw, ph) = (tilesize * (w + 1), tilesize * (h + 1));
    let input = (-tilesize..pw + tilesize, -tilesize..ph + tilesize, button())
        .prop_map(|(x, y, button)| Step::Input { x, y, button });
    let slide = (-1..=w, -1..=h, "[ ,x]?")
        .prop_map(|(x, y, junk)| Step::Execute(format!("M{x},{y}{junk}")));
    let junk = "[MSx0-9, -]{0,6}".prop_map(Step::Execute);
    let step = prop_oneof![
        14 => input,
        1 => Just(Step::Solve),
        2 => slide,
        1 => junk,
    ];
    prop::collection::vec(step, 0..60)
}

fn game() -> impl Strategy<Value = (Params, Vec<u8>, i32, Vec<Step>)> {
    (any_params(5), seed(), 1..=64_i32).prop_flat_map(|(p, seed, tilesize)| {
        (
            Just(p),
            Just(seed),
            Just(tilesize),
            steps(p.w, p.h, tilesize),
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn play_matches((p, seed, tilesize, steps) in game()) {
        let desc = rust_desc(p, &seed, true);
        let mut pair = Pair::<Fifteen>::new(CGame::fifteen(), &p, &desc, tilesize)?;
        pair.run(&steps)?;
    }
}

/// Following the hint key to the end solves the puzzle with C's moves
/// and C's completion flash.
#[test]
fn hints_solve_alike() {
    for seed in 0..10 {
        let p = Fifteen::default_params();
        let desc = rust_desc(p, format!("{seed}").as_bytes(), true);
        let mut pair = Pair::<Fifteen>::new(CGame::fifteen(), &p, &desc, 48).expect("same start");
        let hint = Step::Input {
            x: 0,
            y: 0,
            button: i32::from(b'h'),
        };
        for _ in 0..2000 {
            pair.run(std::slice::from_ref(&hint)).expect("same hint");
            if Fifteen::status(&pair.rust.cur) == puzzles_core::game::GameStatus::Won {
                break;
            }
        }
        assert_eq!(
            Fifteen::status(&pair.rust.cur),
            puzzles_core::game::GameStatus::Won,
            "seed {seed}"
        );
    }
}

/// The preference file and its effect on the arrow keys.
#[test]
fn arrow_preference_matches_c() {
    for (prefs, key) in [
        (&b""[..], 0x209),
        (b"arrow-semantics=gap\n", 0x209),
        (b"arrow-semantics=tile\none-key-shortcuts=false\n", 0x20b),
        (b"arrow-semantics=sideways\n", 0x209),
    ] {
        let mut c = CMidend::new(CGame::fifteen(), b"prefs", true);
        let mut r = Midend::<Fifteen>::new(b"prefs", true, MidendOptions::default());
        let c_load = c.load_prefs(prefs);
        let r_load = r.load_prefs(&mut &prefs[..]).map_err(|e| e.to_string());
        assert_eq!(r_load, c_load);
        c.new_game();
        c.size(400, 400, false, 1.0);
        r.new_game();
        r.size(400, 400, false, 1.0);
        let mut saved = Vec::new();
        r.save_prefs(&mut saved).unwrap();
        assert_eq!(saved, c.save_prefs(), "{}", String::from_utf8_lossy(prefs));
        let b = Button::from_raw(key).unwrap();
        r.process_key(0, 0, b);
        c.process_key(0, 0, key);
        let mut r_save = Vec::new();
        r.serialise(&mut r_save).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&r_save),
            String::from_utf8_lossy(&c.serialise())
        );
    }
}
