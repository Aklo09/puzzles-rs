//! Differential tests: the Rust `guess` module against C's `guess.c`.
//!
//! * Presets, and parameter decoding, encoding and validation of
//!   arbitrary strings.
//! * Generation: both presets over many seeds and random parameters
//!   give C's description; mutated descriptions validate alike.
//! * Play: random scripts of mouse presses, drags and releases (so
//!   dragged pegs and their blitter), right-clicks, arrows (with
//!   modifiers, which Guess doesn't take), digits (also `:` and keypad
//!   digits), hints, labels, select keys, solver calls and raw move
//!   strings, compared step by step by the shared driver: move strings,
//!   states, every frame's draw trace and the status.
//! * Deleting pegs with `d` (kept out of the random scripts: with the
//!   cursor on the marking pegs, C reads past its peg array).
//! * Saved UI (`encode_ui`/`decode_ui`) and the `show-labels`
//!   preference through both mid-ends.

#![cfg(feature = "oracle")]

mod common;

use c_oracle::{CGame, CMidend};
use common::{Pair, Step};
use proptest::prelude::*;
use puzzles_core::game::Game;
use puzzles_core::midend::{Midend, MidendOptions};
use puzzles_core::random::RandomState;
use puzzles_games::guess::{Guess, Params};

fn rust_desc(p: Params, seed: &[u8], interactive: bool) -> String {
    let new = Guess::new_desc(&p, &mut RandomState::new(seed), interactive);
    assert_eq!(new.aux, None);
    new.desc
}

#[test]
fn presets_match() {
    let rust: Vec<_> = Guess::presets()
        .presets()
        .map(|(name, p)| (name.to_owned(), Guess::encode_params(p, true)))
        .collect();
    assert_eq!(rust, CGame::guess().presets());
}

proptest! {
    #[test]
    fn params_decode_encode_validate_match(
        s in "([cpg][0-9]{0,9}|[bBmM]|[x +-]){0,6}",
        full: bool,
    ) {
        let c = CGame::guess();
        let mut p = Guess::default_params();
        Guess::decode_params(&mut p, &s);
        prop_assert_eq!(Guess::encode_params(&p, full), c.encode_params(&s, full));
        let rust = Guess::validate_params(&p, full).map_err(|e| e.message().to_owned());
        prop_assert_eq!(rust, c.validate_params(&s, full));
    }
}

fn any_params() -> impl Strategy<Value = Params> {
    (
        2..=10_i32,
        2..=6_i32,
        1..=12_i32,
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(ncolours, npegs, nguesses, allow_blank, allow_multiple)| Params {
                ncolours,
                npegs,
                nguesses,
                allow_blank,
                allow_multiple,
            },
        )
        .prop_filter("valid", |p| Guess::validate_params(p, true).is_ok())
}

fn seed() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..12)
}

#[test]
fn presets_generate_c_descriptions() {
    let c = CGame::guess();
    for (_, p) in Guess::presets().presets() {
        let encoded = Guess::encode_params(p, true);
        for seed in 0..50 {
            let seed = format!("{seed}");
            let (c_desc, _) = c.new_desc(&encoded, seed.as_bytes(), true);
            assert_eq!(
                rust_desc(*p, seed.as_bytes(), true),
                c_desc,
                "{encoded} {seed}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_params_generate_c_descriptions(p in any_params(), seed in seed(), interactive: bool) {
        let encoded = Guess::encode_params(&p, true);
        let (c_desc, _) = CGame::guess().new_desc(&encoded, &seed, interactive);
        prop_assert_eq!(rust_desc(p, &seed, interactive), c_desc);
    }

    #[test]
    fn mutated_descriptions_validate_alike(
        p in any_params(),
        seed in seed(),
        cut in any::<prop::sample::Index>(),
        edit in 0..4_u8,
        ch in prop::sample::select(vec!['0', '7', 'a', 'f', 'F', 'g', ',']),
    ) {
        let mut desc = rust_desc(p, &seed, false);
        let at = cut.index(desc.len() + 1);
        match edit {
            0 => desc.truncate(at),
            1 => desc.insert(at, ch),
            2 if at < desc.len() => desc.replace_range(at..=at, ch.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        let rust = Guess::validate_desc(&p, &desc).map_err(|e| e.message().to_owned());
        let encoded = Guess::encode_params(&p, true);
        prop_assert_eq!(rust, CGame::guess().validate_desc(&encoded, &desc), "{}", desc);
    }
}

/// Buttons as the mid-end passes them on (Guess has no stylus support,
/// so mouse events carry no modifiers).
fn button() -> impl Strategy<Value = i32> {
    let mouse = prop::sample::select(vec![0x200, 0x203, 0x206, 0x202, 0x205, 0x208]);
    let arrows = (
        0x209..=0x20c,
        prop::sample::select(vec![0, 0, 0x1000, 0x2000]),
    )
        .prop_map(|(b, m)| b | m);
    let digits = (b'0'..=b':').prop_map(i32::from);
    let keypad = (b'0'..=b'9').prop_map(|d| i32::from(d) | 0x4000);
    let keys = prop::sample::select(b"hH?lLa".to_vec()).prop_map(i32::from);
    prop_oneof![
        8 => mouse,
        4 => arrows,
        3 => digits,
        1 => keypad,
        2 => keys,
        2 => prop::sample::select(vec![0x20d, 0x20e]),
    ]
}

fn steps(w: i32, h: i32, npegs: i32) -> impl Strategy<Value = Vec<Step>> {
    let input = (-10..w + 10, -10..h + 10, button()).prop_map(|(x, y, button)| Step::Input {
        x,
        y,
        button,
    });
    let guess = prop::collection::vec(
        (0..=10_i32, any::<bool>()),
        npegs as usize - 1..=npegs as usize,
    )
    .prop_map(|pegs| {
        let row: Vec<String> = pegs
            .iter()
            .map(|(p, h)| format!("{p}{}", if *h { "_" } else { "" }))
            .collect();
        Step::Execute(format!("G{}", row.join(",")))
    });
    let junk = "[GSx0-9,_ -]{0,8}".prop_map(Step::Execute);
    let step = prop_oneof![
        16 => input,
        1 => Just(Step::Solve),
        3 => guess,
        1 => junk,
    ];
    prop::collection::vec(step, 0..80)
}

fn game() -> impl Strategy<Value = (Params, Vec<u8>, i32, Vec<Step>)> {
    (any_params(), seed(), 4..=48_i32).prop_flat_map(|(p, seed, tilesize)| {
        let ui = Guess::new_ui(None);
        let (w, h) = Guess::compute_size(&p, tilesize, &ui);
        (Just(p), Just(seed), Just(tilesize), steps(w, h, p.npegs))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn play_matches((p, seed, tilesize, steps) in game()) {
        let desc = rust_desc(p, &seed, true);
        let mut pair = Pair::<Guess>::new(CGame::guess(), &p, &desc, tilesize)?;
        pair.run(&steps)?;
    }
}

/// Playing by the hint key alone wins like C, guess for guess.
#[test]
fn hints_win_alike() {
    for (_, p) in Guess::presets().presets() {
        for seed in 0..10 {
            let desc = rust_desc(*p, format!("{seed}").as_bytes(), true);
            let mut pair = Pair::<Guess>::new(CGame::guess(), p, &desc, 32).expect("same start");
            let hint = Step::Input {
                x: 0,
                y: 0,
                button: i32::from(b'h'),
            };
            let submit = Step::Input {
                x: 0,
                y: 0,
                button: 0x20d,
            };
            for _ in 0..20 {
                pair.run(&[hint.clone(), submit.clone()])
                    .expect("same play");
            }
            assert_eq!(
                Guess::status(&pair.rust.cur),
                puzzles_core::game::GameStatus::Won,
                "{seed}"
            );
        }
    }
}

/// `d` deletes the peg under the cursor (at the positions where C reads
/// its own array).
#[test]
fn deleting_pegs_matches_c() {
    let p = Guess::default_params();
    for seed in 0..10 {
        let desc = rust_desc(p, format!("{seed}").as_bytes(), true);
        let mut pair = Pair::<Guess>::new(CGame::guess(), &p, &desc, 32).expect("same start");
        let key = |button: i32| Step::Input { x: 0, y: 0, button };
        let mut steps = vec![key(i32::from(b'd'))];
        for d in *b"123" {
            steps.push(key(i32::from(d)));
        }
        // Back over the pegs, deleting, and once more on an empty one.
        for _ in 0..3 {
            steps.push(key(0x20b));
            steps.push(key(i32::from(b'D')));
        }
        steps.push(key(0x08));
        pair.run(&steps).expect("same deletes");
    }
}

/// The guess in progress goes into save files and comes back, and the
/// labels preference, through both mid-ends.
#[test]
fn ui_in_save_files_and_prefs_match_c() {
    for prefs in [&b""[..], b"show-labels=true\n", b"show-labels=maybe\n"] {
        let mut c = CMidend::new(CGame::guess(), b"ui", true);
        let mut r = Midend::<Guess>::new(b"ui", true, MidendOptions::default());
        assert_eq!(
            r.load_prefs(&mut &prefs[..]).map_err(|e| e.to_string()),
            c.load_prefs(prefs)
        );
        c.new_game();
        c.size(400, 600, false, 1.0);
        r.new_game();
        r.size(400, 600, false, 1.0);
        // Two pegs placed, one held; then save and load into fresh ones.
        for key in [i32::from(b'3'), i32::from(b'5'), 0x20b, 0x20e] {
            c.process_key(0, 0, key);
            r.process_key(0, 0, puzzles_core::button::Button::from_raw(key).unwrap());
        }
        let mut saved = Vec::new();
        r.serialise(&mut saved).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&saved),
            String::from_utf8_lossy(&c.serialise())
        );
        assert!(String::from_utf8_lossy(&saved).contains("UI      :8:3,5_,0,0"));
        let mut c2 = CMidend::new(CGame::guess(), b"x", true);
        let mut r2 = Midend::<Guess>::new(b"x", true, MidendOptions::default());
        assert_eq!(c2.deserialise(&saved), Ok(()));
        r2.deserialise(&mut saved.as_slice()).unwrap();
        let mut again = Vec::new();
        r2.serialise(&mut again).unwrap();
        assert_eq!(again, c2.serialise());
        let mut prefs_out = Vec::new();
        r.save_prefs(&mut prefs_out).unwrap();
        assert_eq!(prefs_out, c.save_prefs());
    }
}

/// Damaged UI lines in a save file load alike: out-of-range colours are
/// dropped, holds kept, junk read as C's `atoi` reads it.
#[test]
fn damaged_ui_lines_load_like_c() {
    let mut c = CMidend::new(CGame::guess(), b"ui", true);
    c.new_game();
    let saved = String::from_utf8(c.serialise()).unwrap();
    let line = saved
        .lines()
        .find(|l| l.starts_with("UI"))
        .unwrap()
        .to_owned();
    for ui in [
        "7,6,0,1",
        "9_,-1,3_,10",
        "1,2",
        "x1,2_x,3",
        "6_6_6_6",
        "",
        "1,,,,2",
        " 4, 5,+6,0007",
    ] {
        let damaged = saved.replace(&line, &format!("UI      :{}:{ui}", ui.len()));
        let mut c2 = CMidend::new(CGame::guess(), b"x", true);
        let mut r2 = Midend::<Guess>::new(b"x", true, MidendOptions::default());
        let c_ok = c2.deserialise(damaged.as_bytes());
        let r_ok = r2
            .deserialise(&mut damaged.as_bytes())
            .map_err(|e| e.to_string());
        assert_eq!(r_ok, c_ok, "{ui:?}");
        let mut out = Vec::new();
        r2.serialise(&mut out).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&c2.serialise()),
            "{ui:?}"
        );
    }
}
