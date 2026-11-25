//! Differential tests for the core types of Phase 2 against C:
//!
//! * `Button` against C's button codes and classification macros, and
//!   `Button::label` against `button2label`;
//! * `misc::move_cursor` against `move_cursor`;
//! * `Drawing` (over `RecordingDrawing`) against `drawing.c` and the
//!   `misc.c` drawing helpers (over the C recording drawing API): random
//!   sequences of calls must give byte-identical traces. This also pins
//!   down the trace format later draw-trace tests rely on.

#![cfg(feature = "oracle")]

use c_oracle::{CRecorder, c_button_class, c_button2label, c_move_cursor};
use proptest::prelude::*;
use puzzles_core::button::{Button, ButtonKind, UiAction};
use puzzles_core::drawing::{Align, Blitter, Drawing, FontType, HAlign, RecordingDrawing, VAlign};
use puzzles_core::game::MoveResult;
use puzzles_core::misc::move_cursor;

// ---------------------------------------------------------------------
// Buttons

/// Every code Rust can decode classifies the same way under C's macros.
#[test]
fn button_predicates_match_c_macros() {
    let mut checked = 0;
    for raw in 0..0x8000 {
        let Some(button) = Button::from_raw(raw) else {
            continue;
        };
        let class = c_button_class(raw);
        let rust = i32::from(button.is_mouse_down())
            | i32::from(button.is_mouse_drag()) << 1
            | i32::from(button.is_mouse_release()) << 2
            | i32::from(button.is_cursor_move()) << 3
            | i32::from(button.is_cursor_select()) << 4
            | i32::from(matches!(button.kind, ButtonKind::Ui(_))) << 5;
        // C's macros look at the whole code, so a modifier stops them
        // matching; Rust's look at the kind (the mid-end strips
        // modifiers before games test them). Compare unmodified codes.
        if button.mods.is_empty() {
            assert_eq!(rust, class, "{raw:#x} {button:?}");
        }
        checked += 1;
    }
    assert!(checked > 0x2000);
}

/// The UI codes C's front ends send, by name.
#[test]
fn ui_codes_are_the_c_values() {
    // UI_QUIT .. UI_REDO follow UI_LOWER_BOUND = CURSOR_SELECT2 + 1.
    for (raw, action) in [
        (0x210, UiAction::Quit),
        (0x211, UiAction::NewGame),
        (0x212, UiAction::Solve),
        (0x213, UiAction::Undo),
        (0x214, UiAction::Redo),
    ] {
        assert_eq!(c_button_class(raw), 32, "{raw:#x}");
        assert_eq!(
            Button::from_raw(raw).map(|b| b.kind),
            Some(ButtonKind::Ui(action))
        );
    }
    assert_eq!(c_button_class(0x20f), 0, "UI_LOWER_BOUND is not a key");
    assert_eq!(c_button_class(0x215), 0, "UI_UPPER_BOUND is not a key");
}

/// Wherever Rust has a label, it is C's. (Where Rust has none, C calls
/// `fatal`, which can't be tested without aborting; the set is small and
/// checked by the unit tests.)
#[test]
fn button_labels_match_c() {
    let mut labelled = 0;
    for raw in 0..0x8000 {
        if let Some(label) = Button::from_raw(raw).and_then(Button::label) {
            assert_eq!(label, c_button2label(raw), "{raw:#x}");
            labelled += 1;
        }
    }
    // 62 letters and digits, backspace, four arrows, select.
    assert_eq!(labelled, 62 + 1 + 4 + 1);
}

// ---------------------------------------------------------------------
// move_cursor

fn code(result: &MoveResult<()>) -> i32 {
    match result {
        MoveResult::Unused => 0,
        MoveResult::NoEffect => 1,
        MoveResult::UiUpdate => 2,
        MoveResult::Move(()) => unreachable!("move_cursor never moves"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn move_cursor_matches_c(
        // Mostly cursor keys (with and without modifiers), some others.
        raw in prop_oneof![
            4 => (0x209..=0x20e_i32, prop_oneof![Just(0), Just(0x1000), Just(0x2000), Just(0x800)])
                .prop_map(|(c, m)| c | m),
            1 => prop_oneof![Just(i32::from(b'a')), Just(0x200), Just(0x205), Just(0x212)],
        ],
        x in -1..7_i32,
        y in -1..7_i32,
        w in 1..6_i32,
        h in 1..6_i32,
        wrap in any::<bool>(),
        visible in proptest::option::of(any::<bool>()),
    ) {
        let button = Button::from_raw(raw).expect("defined code");
        let (mut rx, mut ry) = (x, y);
        let mut rvis = visible.unwrap_or(false);
        let result = move_cursor::<()>(
            button, &mut rx, &mut ry, w, h, wrap, visible.map(|_| &mut rvis),
        );
        let (c, (cx, cy), cvis) = c_move_cursor(raw, (x, y), (w, h), wrap, visible);
        prop_assert_eq!(code(&result), c);
        prop_assert_eq!((rx, ry), (cx, cy));
        prop_assert_eq!(visible.map(|_| rvis), cvis);
    }
}

// ---------------------------------------------------------------------
// Drawing

#[derive(Clone, Debug)]
enum Op {
    Text(i32, i32, bool, i32, u8, usize, String),
    Rect(i32, i32, i32, i32, usize),
    Line(i32, i32, i32, i32, usize),
    Polygon(Vec<(i32, i32)>, Option<usize>, usize),
    Circle(i32, i32, i32, Option<usize>, usize),
    ThickLine(f32, f32, f32, f32, f32, usize),
    Update(i32, i32, i32, i32),
    Clip(i32, i32, i32, i32),
    Unclip,
    Start,
    End,
    BlitterSave(usize, i32, i32),
    BlitterLoad(usize, i32, i32),
    RectOutline(i32, i32, i32, i32, usize),
    RectCorners(i32, i32, i32, usize),
    TextOutline(i32, i32, bool, i32, u8, usize, Option<usize>, String),
}

/// C's six alignment combinations, indexed 0..6.
fn align(index: u8) -> Align {
    let h = [HAlign::Left, HAlign::Centre, HAlign::Right][usize::from(index % 3)];
    let v = if index < 3 {
        VAlign::Baseline
    } else {
        VAlign::Centre
    };
    Align { h, v }
}

fn font(fixed: bool) -> FontType {
    if fixed {
        FontType::Fixed
    } else {
        FontType::Variable
    }
}

fn c_colour(c: Option<usize>) -> i32 {
    c.map_or(-1, |c| i32::try_from(c).expect("small"))
}

const BLITTER_SIZES: [(i32, i32); 3] = [(8, 8), (13, 5), (1, 30)];

fn op() -> impl Strategy<Value = Op> {
    let coord = -50..300_i32;
    let colour = 0..12_usize;
    let fill = proptest::option::of(0..12_usize);
    let text = "[ -~]{0,12}";
    let float = prop_oneof![-50.0..300.0_f32, Just(0.0_f32), Just(0.5), Just(1.0)];
    prop_oneof![
        (
            coord.clone(),
            coord.clone(),
            any::<bool>(),
            1..40_i32,
            0..6_u8,
            colour.clone(),
            text
        )
            .prop_map(|(x, y, f, s, a, c, t)| Op::Text(x, y, f, s, a, c, t)),
        (
            coord.clone(),
            coord.clone(),
            -5..80_i32,
            -5..80_i32,
            colour.clone()
        )
            .prop_map(|(x, y, w, h, c)| Op::Rect(x, y, w, h, c)),
        (
            coord.clone(),
            coord.clone(),
            coord.clone(),
            coord.clone(),
            colour.clone()
        )
            .prop_map(|(a, b, c, d, k)| Op::Line(a, b, c, d, k)),
        (
            proptest::collection::vec((coord.clone(), coord.clone()), 0..8),
            fill.clone(),
            colour.clone()
        )
            .prop_map(|(p, f, o)| Op::Polygon(p, f, o)),
        (
            coord.clone(),
            coord.clone(),
            0..60_i32,
            fill.clone(),
            colour.clone()
        )
            .prop_map(|(x, y, r, f, o)| Op::Circle(x, y, r, f, o)),
        (
            -2.0..12.0_f32,
            float.clone(),
            float.clone(),
            float.clone(),
            float,
            colour.clone()
        )
            .prop_map(|(t, a, b, c, d, k)| Op::ThickLine(t, a, b, c, d, k)),
        (coord.clone(), coord.clone(), 0..80_i32, 0..80_i32)
            .prop_map(|(x, y, w, h)| Op::Update(x, y, w, h)),
        (coord.clone(), coord.clone(), 0..80_i32, 0..80_i32)
            .prop_map(|(x, y, w, h)| Op::Clip(x, y, w, h)),
        Just(Op::Unclip),
        Just(Op::Start),
        Just(Op::End),
        (0..BLITTER_SIZES.len(), coord.clone(), coord.clone())
            .prop_map(|(b, x, y)| Op::BlitterSave(b, x, y)),
        (0..BLITTER_SIZES.len(), coord.clone(), coord.clone())
            .prop_map(|(b, x, y)| Op::BlitterLoad(b, x, y)),
        (
            coord.clone(),
            coord.clone(),
            -5..80_i32,
            -5..80_i32,
            colour.clone()
        )
            .prop_map(|(x, y, w, h, c)| Op::RectOutline(x, y, w, h, c)),
        (coord.clone(), coord.clone(), -9..40_i32, colour.clone())
            .prop_map(|(x, y, r, c)| Op::RectCorners(x, y, r, c)),
        (
            coord.clone(),
            coord,
            any::<bool>(),
            1..40_i32,
            0..6_u8,
            colour,
            fill,
            "[ -~]{0,12}"
        )
            .prop_map(|(x, y, f, s, a, c, o, t)| Op::TextOutline(x, y, f, s, a, c, o, t)),
    ]
}

/// Applies the ops through Rust's `Drawing` and C's `drawing.c`, and
/// returns both traces.
fn traces(ops: &[Op]) -> (String, String) {
    let mut rec = RecordingDrawing::default();
    let mut c = CRecorder::new();
    let blitters: Vec<Blitter> = BLITTER_SIZES
        .iter()
        .map(|&(w, h)| Blitter::new(w, h))
        .collect();
    let c_blitters: Vec<usize> = BLITTER_SIZES
        .iter()
        .map(|&(w, h)| c.blitter_new(w, h))
        .collect();
    {
        let mut dr = Drawing::new(&mut rec);
        for op in ops {
            let ci = |k: usize| i32::try_from(k).expect("small");
            match op.clone() {
                Op::Text(x, y, f, s, a, k, t) => {
                    dr.text(x, y, font(f), s, align(a), k, &t);
                    c.text(x, y, i32::from(!f), s, align(a).to_raw(), ci(k), &t);
                }
                Op::Rect(x, y, w, h, k) => {
                    dr.rect(x, y, w, h, k);
                    c.rect(x, y, w, h, ci(k));
                }
                Op::Line(a, b, cc, d, k) => {
                    dr.line(a, b, cc, d, k);
                    c.line(a, b, cc, d, ci(k));
                }
                Op::Polygon(p, f, o) => {
                    dr.polygon(&p, f, o);
                    c.polygon(&p, c_colour(f), ci(o));
                }
                Op::Circle(x, y, r, f, o) => {
                    dr.circle(x, y, r, f, o);
                    c.circle(x, y, r, c_colour(f), ci(o));
                }
                Op::ThickLine(t, a, b, cc, d, k) => {
                    dr.thick_line(t, a, b, cc, d, k);
                    c.thick_line(t, a, b, cc, d, ci(k));
                }
                Op::Update(x, y, w, h) => {
                    dr.update(x, y, w, h);
                    c.update(x, y, w, h);
                }
                Op::Clip(x, y, w, h) => {
                    dr.clip(x, y, w, h);
                    c.clip(x, y, w, h);
                }
                Op::Unclip => {
                    dr.unclip();
                    c.unclip();
                }
                Op::Start => {
                    dr.api().start_draw();
                    c.start_draw();
                }
                Op::End => {
                    dr.api().end_draw();
                    c.end_draw();
                }
                Op::BlitterSave(b, x, y) => {
                    dr.blitter_save(&blitters[b], x, y);
                    c.blitter_save(c_blitters[b], x, y);
                }
                Op::BlitterLoad(b, x, y) => {
                    dr.blitter_load(&blitters[b], x, y);
                    c.blitter_load(c_blitters[b], x, y);
                }
                Op::RectOutline(x, y, w, h, k) => {
                    dr.rect_outline(x, y, w, h, k);
                    c.rect_outline(x, y, w, h, ci(k));
                }
                Op::RectCorners(x, y, r, k) => {
                    dr.rect_corners(x, y, r, k);
                    c.rect_corners(x, y, r, ci(k));
                }
                Op::TextOutline(x, y, f, s, a, k, o, t) => {
                    dr.text_outline(x, y, font(f), s, align(a), k, o, &t);
                    c.text_outline(
                        x,
                        y,
                        i32::from(!f),
                        s,
                        align(a).to_raw(),
                        ci(k),
                        c_colour(o),
                        &t,
                    );
                }
            }
        }
    }
    (rec.to_string(), c.trace())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn drawing_traces_match_c(ops in proptest::collection::vec(op(), 1..40)) {
        let (rust, c) = traces(&ops);
        prop_assert_eq!(rust, c);
    }

    #[test]
    fn text_fallback_matches_c(
        strings in proptest::collection::vec(
            prop_oneof!["[a-z]{0,4}", "[a-z]{0,2}[\u{e9}\u{2212}\u{221a}][a-z]{0,2}"],
            1..5,
        ),
    ) {
        prop_assume!(strings.iter().any(|s| s.is_ascii()));
        let refs: Vec<&str> = strings.iter().map(String::as_str).collect();
        let mut rec = RecordingDrawing::default();
        let dr = Drawing::new(&mut rec);
        let rust = dr.text_fallback(&refs);
        let c = CRecorder::new().text_fallback(&refs);
        prop_assert_eq!(rust, refs[c]);
    }
}

/// Each helper and edge case at least once, with a readable expected
/// trace: the format is part of the contract with C.
#[test]
fn trace_format_is_cs() {
    let ops = [
        Op::Text(10, 20, true, 12, 4, 3, "Hi there".to_owned()),
        Op::Polygon(vec![(1, 2), (3, 4), (5, 6)], None, 2),
        Op::Circle(5, 5, 4, Some(1), 2),
        Op::ThickLine(0.5, 1.0, 2.0, 3.5, 4.0, 7),
        Op::RectCorners(10, 10, 5, 1),
        Op::TextOutline(1, 1, false, 8, 0, 2, Some(9), "x".to_owned()),
        Op::BlitterSave(1, -3, 4),
        Op::ThickLine(2.5, 0.0, 0.0, 1.0, 1.0, 7),
        Op::TextOutline(1, 1, true, 8, 4, 2, None, "y".to_owned()),
        Op::RectOutline(2, 3, 4, 5, 6),
    ];
    let (rust, c) = traces(&ops);
    assert_eq!(rust, c);
    let expected = "\
text 10 20 0 12 257 3 Hi there
polygon -1 2 3 1 2 3 4 5 6
circle 5 5 4 1 2
thick 3f800000 3f800000 40000000 40600000 40800000 7
line 5 5 5 8 1
line 5 5 8 5 1
line 5 15 5 12 1
line 5 15 8 15 1
line 15 5 15 8 1
line 15 5 12 5 1
line 15 15 15 12 1
line 15 15 12 15 1
text 0 1 1 8 0 9 x
text 2 1 1 8 0 9 x
text 1 0 1 8 0 9 x
text 1 2 1 8 0 9 x
text 1 1 1 8 0 2 x
blitter_save 13 5 -3 4
thick 40200000 00000000 00000000 3f800000 3f800000 7
text 1 1 0 8 257 2 y
polygon -1 6 4 2 3 2 7 5 7 5 3
";
    assert_eq!(rust, expected);
}
