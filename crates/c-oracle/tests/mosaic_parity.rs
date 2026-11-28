//! Differential tests: the Rust `mosaic` module against C's `mosaic.c`.
//!
//! * Presets, and parameter decoding, encoding and validation of
//!   arbitrary strings.
//! * Generation: every preset over several seeds, and random sizes with
//!   and without aggressive generation, give C's description; mutated
//!   descriptions validate alike.
//! * Play: random scripts of presses, drags and releases of both buttons,
//!   arrows (with modifiers, which Mosaic doesn't take), both selects,
//!   solver calls and raw move strings (toggles, drags painting any
//!   value, solve strings, junk), compared step by step by the shared
//!   driver: move strings, states, key labels, every flash frame's draw
//!   trace and the status bar.
//! * Winning by clicks, with the completion flash.
//! * The Custom dialog through both mid-ends: C lists Height before
//!   Width, the port Width first (custom dialog field order decision), so
//!   C's first two items are swapped before comparing.

#![cfg(feature = "oracle")]

mod common;

use c_oracle::{CGame, CMidend};
use common::{Pair, Step};
use proptest::prelude::*;
use puzzles_core::config::ConfigValue;
use puzzles_core::game::{Game, GameStatus};
use puzzles_core::midend::{ConfigKind, Midend, MidendOptions};
use puzzles_core::random::RandomState;
use puzzles_games::mosaic::{Mosaic, Move, Params};

/// C's palette index for each of the port's colours: the clue text
/// colours are entries of their own in the port, aliases of the black
/// and white square colours in C (also in `theme_parity.rs`).
const C_COLOURS: &[usize] = &[0, 1, 2, 3, 4, 5, 6, 7, 3, 4];

fn rust_desc(p: &Params, seed: &[u8], interactive: bool) -> String {
    let new = Mosaic::new_desc(p, &mut RandomState::new(seed), interactive);
    assert_eq!(new.aux, None, "Mosaic has no aux data");
    new.desc
}

// ---------------------------------------------------------------------
// Parameters

#[test]
fn presets_match() {
    let rust: Vec<_> = Mosaic::presets()
        .presets()
        .map(|(name, p)| (name.to_owned(), Mosaic::encode_params(p, true)))
        .collect();
    assert_eq!(rust, CGame::mosaic().presets());
    assert_eq!(rust.len(), 6);
}

proptest! {
    /// Digit runs of at most 9 digits, so every number fits in an `int`
    /// (see flip_parity.rs).
    #[test]
    fn params_decode_encode_validate_match(
        s in "([0-9]{0,9}[xha +-]){0,4}[0-9]{0,9}([a-z,:#][a-z0-9,:#]{0,2})?",
        full: bool,
    ) {
        let c = CGame::mosaic();
        let mut p = Mosaic::default_params();
        Mosaic::decode_params(&mut p, &s);
        prop_assert_eq!(Mosaic::encode_params(&p, full), c.encode_params(&s, full));
        let rust = Mosaic::validate_params(&p, full).map_err(|e| e.message().to_owned());
        prop_assert_eq!(rust, c.validate_params(&s, full));
    }
}

// ---------------------------------------------------------------------
// Generation

#[test]
fn presets_generate_c_descriptions() {
    let c = CGame::mosaic();
    for (_, p) in Mosaic::presets().presets() {
        let encoded = Mosaic::encode_params(p, true);
        let seeds = if p.w > 15 { 3 } else { 25 };
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
    (3..=max, 3..=max, any::<bool>()).prop_map(|(w, h, aggressive)| Params { w, h, aggressive })
}

fn seed() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..12)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_sizes_generate_c_descriptions(p in any_params(14), seed in seed(), interactive: bool) {
        let encoded = Mosaic::encode_params(&p, true);
        let (c_desc, _) = CGame::mosaic().new_desc(&encoded, &seed, interactive);
        prop_assert_eq!(rust_desc(&p, &seed, interactive), c_desc);
    }

    #[test]
    fn mutated_descriptions_validate_alike(
        p in any_params(8),
        seed in seed(),
        cut in any::<prop::sample::Index>(),
        edit in 0..4_u8,
        ch in prop::sample::select(vec!['0', '9', 'a', 'z', 'A', '/', ' ', '{']),
    ) {
        let mut desc = rust_desc(&p, &seed, false);
        let at = cut.index(desc.len() + 1);
        match edit {
            0 => desc.truncate(at),
            1 => desc.insert(at, ch),
            2 if at < desc.len() => desc.replace_range(at..=at, ch.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        let rust = Mosaic::validate_desc(&p, &desc).map_err(|e| e.message().to_owned());
        let encoded = Mosaic::encode_params(&p, true);
        prop_assert_eq!(rust, CGame::mosaic().validate_desc(&encoded, &desc), "{}", desc);
    }
}

// ---------------------------------------------------------------------
// Play

/// Button codes as the mid-end passes them on: mouse events without
/// modifiers (Mosaic has no stylus support), arrows with or without
/// Ctrl/Shift, both selects (also modified), and keys Mosaic ignores.
fn button() -> impl Strategy<Value = i32> {
    let mouse = prop_oneof![
        3 => prop::sample::select(vec![0x200, 0x202, 0x203, 0x205, 0x206, 0x208]),
        1 => prop::sample::select(vec![0x201, 0x204, 0x207]),
    ];
    let arrows = (
        0x209..=0x20c,
        prop::sample::select(vec![0, 0, 0x1000, 0x2000]),
    )
        .prop_map(|(b, m)| b | m);
    let selects =
        (0x20d..=0x20e, prop::sample::select(vec![0, 0, 0, 0x1000])).prop_map(|(b, m)| b | m);
    prop_oneof![
        8 => mouse,
        3 => arrows,
        2 => selects,
        1 => prop::sample::select(vec![i32::from(b'a'), i32::from(b'1'), 8]),
    ]
}

/// What a drag move may paint: the three marks, the clue bits, and
/// values that wrap in C's `char` (255 is -1, which is also the draw
/// state's "never drawn").
fn paint() -> impl Strategy<Value = i32> {
    prop::sample::select(vec![
        0, 1, 2, 3, 4, 5, 7, 8, 9, 12, 15, 127, 128, 255, 256, 257, -1, -2, -128, -129,
    ])
}

/// A script for a `w` × `h` grid at `tilesize`.
fn steps(w: i32, h: i32, tilesize: i32) -> impl Strategy<Value = Vec<Step>> {
    let (pw, ph) = (tilesize * (w + 1), tilesize * (h + 1));
    let input = (-tilesize..pw + tilesize, -tilesize..ph + tilesize, button())
        .prop_map(|(x, y, button)| Step::Input { x, y, button });
    let toggle = (
        prop::sample::select(vec!['t', 'T']),
        -1..=w,
        -1..=h,
        "[ ,x]?",
    )
        .prop_map(|(t, x, y, junk)| Step::Execute(format!("{t}{x},{y}{junk}")));
    let drag = (
        prop::sample::select(vec!['d', 'e']),
        (-1..=w, -1..=h, -1..=w + 2, -1..=h + 2),
        paint(),
    )
        .prop_map(|(t, (x, y, fx, fy), s)| Step::Execute(format!("{t}{x},{y},{fx},{fy},{s}")));
    // No longer than C's own solve strings plus one digit: with more, C
    // can loop forever (see mosaic.rs), which the port doesn't.
    let hex_len = ((w * h) as usize).div_ceil(8) * 2 + 1;
    let solve_text = prop::collection::vec(
        prop::sample::select(vec!['0', '7', 'a', 'f', 'F', 'g', 'z', ' ']),
        0..=hex_len,
    )
    .prop_map(|v| Step::Execute(format!("s{}", v.into_iter().collect::<String>())));
    // Never starting with `s`, for the same reason.
    let junk = "([tTdex0-9, +-][tTdesx0-9, +-]{0,8})?".prop_map(Step::Execute);
    let step = prop_oneof![
        14 => input,
        1 => Just(Step::Solve),
        3 => toggle,
        3 => drag,
        1 => solve_text,
        1 => junk,
    ];
    prop::collection::vec(step, 0..60)
}

fn game() -> impl Strategy<Value = (Params, Vec<u8>, i32, Vec<Step>)> {
    (any_params(8), seed(), 1..=48_i32).prop_flat_map(|(p, seed, tilesize)| {
        (
            Just(p),
            Just(seed),
            Just(tilesize),
            steps(p.w, p.h, tilesize),
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn play_matches((p, seed, tilesize, steps) in game()) {
        let desc = rust_desc(&p, &seed, true);
        let mut pair = Pair::<Mosaic>::with_c_colours(CGame::mosaic(), &p, &desc, tilesize, C_COLOURS)?;
        pair.run(&steps)?;
    }
}

/// A whole game won by clicking the solution in, square by square (a
/// left click for black, a right click for white), ending with the
/// completion flash; then the keyboard has no effect, as in C.
#[test]
fn winning_by_clicks_matches_c() {
    let ts = 32;
    for (_, p) in Mosaic::presets().presets().filter(|(_, p)| p.w <= 10) {
        for seed in 0..5 {
            let desc = rust_desc(p, format!("win {seed}").as_bytes(), true);
            let mut pair = Pair::<Mosaic>::with_c_colours(CGame::mosaic(), p, &desc, ts, C_COLOURS)
                .expect("same start");
            let Ok(Move::Solve(marked)) = Mosaic::solve(&pair.rust.orig, &pair.rust.orig, None)
            else {
                panic!("{p:?}: no solution");
            };
            let mut steps = Vec::new();
            for (i, &m) in marked.iter().enumerate() {
                let (x, y) = (i as i32 % p.w, i as i32 / p.w);
                let (px, py) = (ts / 2 + ts * x + 5, ts / 2 + ts * y + 5);
                let (press, release) = if m { (0x200, 0x206) } else { (0x202, 0x208) };
                steps.push(Step::Input {
                    x: px,
                    y: py,
                    button: press,
                });
                steps.push(Step::Input {
                    x: px,
                    y: py,
                    button: release,
                });
            }
            for button in [0x20d, 0x20c, 0x20d, 0x200] {
                steps.push(Step::Input {
                    x: 40,
                    y: 40,
                    button,
                });
            }
            pair.run(&steps).expect("same play");
            assert_eq!(Mosaic::status(&pair.rust.cur), GameStatus::Won, "{p:?}");
        }
    }
}

/// A drag across the grid paints like C, from a press through drags to
/// the release, along rows and columns.
#[test]
fn drags_match_c() {
    let ts = 20;
    let p = Params {
        w: 7,
        h: 6,
        aggressive: true,
    };
    for seed in 0..10 {
        let desc = rust_desc(&p, format!("drag {seed}").as_bytes(), true);
        let mut pair = Pair::<Mosaic>::with_c_colours(CGame::mosaic(), &p, &desc, ts, C_COLOURS)
            .expect("same start");
        let at = |x: i32, y: i32, button: i32| Step::Input {
            x: ts / 2 + ts * x + 3,
            y: ts / 2 + ts * y + 3,
            button,
        };
        let (press, drag, release) = if seed % 2 == 0 {
            (0x200, 0x203, 0x206)
        } else {
            (0x202, 0x205, 0x208)
        };
        let mut steps = vec![at(1, 1, press)];
        steps.extend((2..7).map(|x| at(x, 1, drag)));
        steps.extend((2..6).map(|y| at(6, y, drag)));
        steps.push(at(6, 5, release));
        steps.push(at(0, seed % 6, press));
        steps.push(at(3, (seed + 2) % 6, drag));
        steps.push(at(5, seed % 6, release));
        // A release doesn't move the drag's last square, so a drag
        // straight after it still runs from the last dragged square.
        steps.push(at(1, 1, press));
        steps.push(at(2, 1, drag));
        steps.push(at(4, 1, release));
        steps.push(at(2, 3, drag));
        steps.push(at(2, 4, release));
        pair.run(&steps).expect("same drags");
    }
}

/// A saved game whose squares hold values only a drag move from a file
/// can put there, loaded into both mid-ends and drawn from scratch: a
/// square holding -1 matches the draw state's "never drawn" and is left
/// undrawn, in C as in the port.
#[test]
fn odd_squares_from_a_save_file_draw_like_c() {
    for (seed, paint) in [(0, 255), (1, 128), (2, 7), (3, 256), (4, -2)] {
        let mut c = CMidend::new(CGame::mosaic(), format!("odd {seed}").as_bytes(), true);
        c.set_params("6x5");
        c.new_game();
        let saved = String::from_utf8(c.serialise()).expect("ASCII");
        // Paint the first row's squares, toggle one, then a junk move.
        let moves = [
            format!("d5,0,0,0,{paint}"),
            format!("d0,4,0,0,{paint}"),
            "t3,0".to_owned(),
            "x".to_owned(),
        ];
        let mut lines: Vec<String> = saved.lines().map(str::to_owned).collect();
        let n = moves.len() + 1;
        for line in &mut lines {
            if line.starts_with("NSTATES") || line.starts_with("STATEPOS") {
                let key = &line[..8];
                *line = format!("{key}:{}:{n}", n.to_string().len());
            }
        }
        lines.extend(moves.iter().map(|m| format!("MOVE    :{}:{m}", m.len())));
        let data = lines.join("\n") + "\n";

        let mut r = Midend::<Mosaic>::new(b"x", true, MidendOptions::default());
        let mut c = CMidend::new(CGame::mosaic(), b"x", true);
        assert_eq!(c.deserialise(data.as_bytes()), Ok(()), "{data}");
        r.deserialise(&mut data.as_bytes()).expect("loads");
        assert_eq!(r.size(400, 400, false, 1.0), c.size(400, 400, false, 1.0));
        let c_trace = c.force_redraw();
        r.force_redraw();
        let mut rec = puzzles_core::drawing::RecordingDrawing::default();
        r.redraw(&mut common::CColours::new(&mut rec, C_COLOURS));
        assert_eq!(rec.to_string(), c_trace, "paint {paint}");
        let mut out = Vec::new();
        r.serialise(&mut out).expect("saves");
        // The junk move is written back as the empty move (D4: canonical
        // moves), which C reads the same way.
        let c_saved = String::from_utf8(c.serialise()).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            c_saved.replace("MOVE    :1:x\n", "MOVE    :0:\n")
        );
    }
}

// ---------------------------------------------------------------------
// The Custom dialog

/// A dialog's items as the C harness prints them (title excluded).
fn rust_items(items: &[puzzles_core::config::ConfigItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| {
            let value = match &item.value {
                ConfigValue::String(v) => format!("string|{v}"),
                ConfigValue::Bool(b) => format!("bool|{}", u8::from(*b)),
                ConfigValue::Choices { .. } => unreachable!("Mosaic has no choices"),
            };
            format!("{}|-|{value}", item.name)
        })
        .collect()
}

#[test]
fn custom_dialog_matches_c_with_width_first() {
    for (w, h, aggressive) in [
        ("7", "4", "0"),
        ("12", "3", "1"),
        ("2", "9", "1"),
        ("101", "100", "0"),
        (" 8x", "+5", "1"),
    ] {
        let mut c = CMidend::new(CGame::mosaic(), b"dialog", true);
        let mut r = Midend::<Mosaic>::new(b"dialog", true, MidendOptions::default());
        let c_text = c.get_config(0).expect("C has a Custom dialog");
        let mut c_lines: Vec<String> = c_text.lines().map(str::to_owned).collect();
        // C: Height, Width, Aggressive.
        c_lines.swap(1, 2);
        let config = r.get_config(ConfigKind::Settings).expect("a Custom dialog");
        let mut r_lines = vec![config.title.clone()];
        r_lines.extend(rust_items(&config.items));
        assert_eq!(r_lines, c_lines);

        let mut items = config.items;
        for (item, v) in items.iter_mut().zip([w, h, aggressive]) {
            match &mut item.value {
                ConfigValue::String(s) => *s = v.to_owned(),
                ConfigValue::Bool(b) => *b = v == "1",
                ConfigValue::Choices { .. } => unreachable!(),
            }
        }
        let r_ok = r
            .set_config(ConfigKind::Settings, &items)
            .map_err(|e| e.message().to_owned());
        assert_eq!(r_ok, c.set_config(0, &[h, w, aggressive]), "{w} {h}");
        if r_ok.is_ok() {
            r.new_game();
            c.new_game();
            assert_eq!(r.get_game_id(), c.get_game_id(), "{w} {h}");
        }
    }
}
