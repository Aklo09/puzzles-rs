//! Differential tests: the Rust `flip` module against C's `flip.c`.
//!
//! * Presets, and parameter decoding, encoding and validation of
//!   arbitrary strings.
//! * Generation: every preset over many seeds, and random sizes of both
//!   shape types, give C's description.
//! * Description validation of mutated descriptions: same verdict and
//!   message.
//! * Play: random scripts of clicks, keys, solver calls and raw move
//!   strings, compared step by step by the shared driver
//!   (`common/mod.rs`): move strings, states, animation lengths, every
//!   animation and flash frame's draw trace, and the status bar.

#![cfg(feature = "oracle")]

mod common;

use c_oracle::CGame;
use common::{Pair, Step};
use proptest::prelude::*;
use puzzles_core::game::Game;
use puzzles_core::random::RandomState;
use puzzles_games::flip::{Flip, MatrixType, Params};

fn params(w: i32, h: i32, random: bool) -> Params {
    Params {
        w,
        h,
        matrix_type: if random {
            MatrixType::Random
        } else {
            MatrixType::Crosses
        },
    }
}

/// Random shapes on 1 × 2 and 2 × 1 never generate, in C as in Rust.
fn generates(p: &Params) -> bool {
    !(p.matrix_type == MatrixType::Random && p.w * p.h == 2)
}

fn rust_desc(p: &Params, seed: &[u8], interactive: bool) -> String {
    let new = Flip::new_desc(p, &mut RandomState::new(seed), interactive);
    assert_eq!(new.aux, None, "Flip has no aux data");
    new.desc
}

// ---------------------------------------------------------------------
// Parameters

#[test]
fn presets_match() {
    let rust: Vec<_> = Flip::presets()
        .presets()
        .map(|(name, p)| (name.to_owned(), Flip::encode_params(p, true)))
        .collect();
    assert_eq!(rust, CGame::flip().presets());
    assert_eq!(rust.len(), 6);
}

/// Parameter strings with digit runs short enough to fit in an `int`
/// (longer ones overflow, which C leaves to the platform).
fn param_string() -> impl Strategy<Value = String> {
    "([0-9]{0,9}[xrc +-]?){0,4}[a-z0-9,:#]{0,3}"
}

proptest! {
    #[test]
    fn params_decode_encode_validate_match(s in param_string(), full: bool) {
        let c = CGame::flip();
        let mut p = Flip::default_params();
        Flip::decode_params(&mut p, &s);
        prop_assert_eq!(Flip::encode_params(&p, full), c.encode_params(&s, full));
        let rust = Flip::validate_params(&p, full).map_err(|e| e.message().to_owned());
        prop_assert_eq!(rust, c.validate_params(&s, full));
    }
}

// ---------------------------------------------------------------------
// Generation

#[test]
fn presets_generate_c_descriptions() {
    let c = CGame::flip();
    for (_, p) in Flip::presets().presets() {
        let encoded = Flip::encode_params(p, true);
        for seed in 0..25 {
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
    (1..=max, 1..=max, any::<bool>())
        .prop_map(|(w, h, random)| params(w, h, random))
        .prop_filter("generates", generates)
}

fn seed() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..12)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_sizes_generate_c_descriptions(
        p in any_params(8),
        seed in seed(),
        interactive: bool,
    ) {
        let encoded = Flip::encode_params(&p, true);
        let (c_desc, _) = CGame::flip().new_desc(&encoded, &seed, interactive);
        prop_assert_eq!(rust_desc(&p, &seed, interactive), c_desc);
    }

    #[test]
    fn mutated_descriptions_validate_alike(
        p in any_params(5),
        seed in seed(),
        cut in any::<prop::sample::Index>(),
        edit in 0..4_u8,
        ch in prop::sample::select(vec!['0', '9', 'a', 'F', 'g', ',', ' ', 'x']),
    ) {
        let mut desc = rust_desc(&p, &seed, false);
        let at = cut.index(desc.len() + 1);
        match edit {
            0 => desc.truncate(at),
            1 => desc.insert(at, ch),
            2 if at < desc.len() => desc.replace_range(at..=at, ch.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        let rust = Flip::validate_desc(&p, &desc).map_err(|e| e.message().to_owned());
        let encoded = Flip::encode_params(&p, true);
        prop_assert_eq!(rust, CGame::flip().validate_desc(&encoded, &desc), "{}", desc);
    }
}

// ---------------------------------------------------------------------
// Play

/// Button codes as the mid-end passes them on: mouse events without
/// modifiers, arrows with or without Ctrl/Shift, both selects, and a few
/// keys Flip ignores.
fn button() -> impl Strategy<Value = i32> {
    let mouse = 0x200..=0x208;
    let arrows =
        (0x209..=0x20c, prop::sample::select(vec![0, 0x1000, 0x2000])).prop_map(|(b, m)| b | m);
    prop_oneof![
        3 => mouse,
        3 => arrows,
        2 => prop::sample::select(vec![0x20d, 0x20e]),
        1 => prop::sample::select(vec![i32::from(b'a'), i32::from(b'1'), 8]),
    ]
}

/// A script for a grid of `w` × `h` squares at `tilesize`.
fn steps(w: i32, h: i32, tilesize: i32) -> impl Strategy<Value = Vec<Step>> {
    let (pw, ph) = (tilesize * (w + 1), tilesize * (h + 1));
    let wh = (w * h) as usize;
    let input = (-tilesize..pw + tilesize, -tilesize..ph + tilesize, button())
        .prop_map(|(x, y, button)| Step::Input { x, y, button });
    let flip_text = (-1..=w, -1..=h, "[ ,x]?")
        .prop_map(|(x, y, junk)| Step::Execute(format!("M{x},{y}{junk}")));
    let solve_text =
        prop::collection::vec(prop::sample::select(vec!['0', '1', '7']), wh - 1..=wh + 1)
            .prop_map(|v| Step::Execute(format!("S{}", v.into_iter().collect::<String>())));
    let junk = "[MSx0-9, -]{0,6}".prop_map(Step::Execute);
    let step = prop_oneof![
        12 => input,
        1 => Just(Step::Solve),
        2 => flip_text,
        1 => solve_text,
        1 => junk,
    ];
    prop::collection::vec(step, 0..40)
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
        let desc = rust_desc(&p, &seed, true);
        let mut pair = Pair::<Flip>::new(CGame::flip(), &p, &desc, tilesize)?;
        pair.run(&steps)?;
    }
}

/// A whole game solved by following the hints: the solver's move, then
/// a click on each hinted square, ends with the completion flash.
#[test]
fn solving_by_hints_completes_alike() {
    for (_, p) in Flip::presets().presets() {
        let desc = rust_desc(p, b"hints", true);
        let mut pair = Pair::<Flip>::new(CGame::flip(), p, &desc, 48).expect("same start");
        pair.run(&[Step::Solve]).expect("same solve");
        let mut steps = Vec::new();
        let hints = match Flip::solve(&pair.rust.orig, &pair.rust.orig, None) {
            Ok(puzzles_games::flip::Move::Solve(hints)) => hints,
            other => panic!("unexpected solve result {other:?}"),
        };
        for (i, &hint) in hints.iter().enumerate() {
            if hint {
                let (x, y) = (i as i32 % p.w, i as i32 / p.w);
                steps.push(Step::Input {
                    x: 24 + 48 * x + 10,
                    y: 24 + 48 * y + 10,
                    button: 0x200,
                });
            }
        }
        pair.run(&steps).expect("same play");
        assert_eq!(
            Flip::status(&pair.rust.cur),
            puzzles_core::game::GameStatus::Won,
            "{p:?}"
        );
    }
}
