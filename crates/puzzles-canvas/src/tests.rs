use super::*;
use puzzles_core::drawing::{HAlign, VAlign};

const BG: Rgb = Rgb::new(0.9, 0.9, 0.9);
const INK: Rgb = Rgb::new(0.0, 0.0, 0.0);
const RED: Rgb = Rgb::new(1.0, 0.0, 0.0);

fn canvas(w: u32, h: u32) -> Canvas {
    Canvas::new(w, h, &[BG, INK, RED]).unwrap()
}

fn rgb(c: &Canvas, x: u32, y: u32) -> [u8; 3] {
    let p = c.pixmap().pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue()]
}

const BG8: [u8; 3] = [230, 230, 230];
const INK8: [u8; 3] = [0, 0, 0];
const RED8: [u8; 3] = [255, 0, 0];

/// Pixels of `colour` inside the rectangle `x0..x1` × `y0..y1`.
fn count(c: &Canvas, colour: [u8; 3], x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| rgb(c, x, y) == colour)
        .count()
}

#[test]
fn starts_with_the_background_and_rejects_empty_sizes() {
    let mut c = canvas(4, 3);
    assert_eq!(count(&c, BG8, 0, 0, 4, 3), 12);
    assert_eq!(c.take_dirty(), IntRect::from_xywh(0, 0, 4, 3));
    assert_eq!(c.take_dirty(), None);
    assert!(Canvas::new(0, 5, &[]).is_err());
    assert!(c.resize(5, 0).is_err());
    assert_eq!(
        (c.width(), c.height()),
        (4, 3),
        "unchanged after a failed resize"
    );
    c.resize(6, 2).unwrap();
    assert_eq!((c.width(), c.height()), (6, 2));
    assert_eq!(
        Canvas::new(2, 2, &[])
            .unwrap()
            .pixmap()
            .pixel(0, 0)
            .unwrap()
            .red(),
        255
    );
}

#[test]
fn rectangles_have_hard_edges() {
    let mut c = canvas(10, 10);
    c.draw_rect(2, 3, 4, 5, 1);
    assert_eq!(count(&c, INK8, 2, 3, 6, 8), 20);
    assert_eq!(
        count(&c, INK8, 0, 0, 10, 10),
        20,
        "nothing outside, no blur"
    );
    c.draw_rect(0, 0, 0, 5, 2);
    c.draw_rect(0, 0, 5, -1, 2);
    assert_eq!(count(&c, RED8, 0, 0, 10, 10), 0);
}

#[test]
fn one_pixel_lines_are_crisp_and_include_both_ends() {
    let mut c = canvas(10, 10);
    c.draw_line(2, 4, 7, 4, 1);
    assert_eq!(count(&c, INK8, 2, 4, 8, 5), 6);
    assert_eq!(count(&c, INK8, 0, 0, 10, 10), 6);
    assert_eq!(rgb(&c, 2, 3), BG8);
    c.draw_line(8, 1, 8, 8, 2);
    assert_eq!(count(&c, RED8, 8, 1, 9, 9), 8);
    c.draw_line(0, 0, 0, 0, 2);
    assert_eq!(rgb(&c, 0, 0), RED8, "a zero-length line is a dot");
}

#[test]
fn shapes_fill_and_outline() {
    let mut c = canvas(20, 20);
    c.draw_polygon(&[(2, 2), (12, 2), (12, 12), (2, 12)], Some(2), 1);
    assert_eq!(rgb(&c, 7, 7), RED8);
    assert_eq!(rgb(&c, 2, 7), INK8);
    assert_eq!(rgb(&c, 15, 15), BG8);
    let mut c = canvas(20, 20);
    c.draw_circle(10, 10, 6, Some(2), 1);
    assert_eq!(rgb(&c, 10, 10), RED8);
    assert_eq!(rgb(&c, 1, 1), BG8);
    c.draw_circle(1, 1, 0, None, 1);
    assert_eq!(rgb(&c, 1, 1), INK8);
    c.draw_thick_line(4.0, 2.0, 18.0, 18.0, 18.0, 1);
    assert_eq!(rgb(&c, 10, 18), INK8);
}

#[test]
fn unknown_colours_draw_nothing() {
    let mut c = canvas(8, 8);
    c.draw_rect(0, 0, 8, 8, 9);
    c.draw_line(0, 0, 7, 7, 9);
    c.draw_circle(4, 4, 3, Some(9), 9);
    c.draw_polygon(&[(0, 0), (7, 0), (0, 7)], Some(9), 9);
    c.draw_text(4, 4, FontType::Variable, 8, Align::CENTRE, 9, "x");
    assert_eq!(count(&c, BG8, 0, 0, 8, 8), 64);
}

#[test]
fn clipping_limits_every_primitive() {
    let mut c = canvas(20, 20);
    c.clip(5, 5, 5, 5);
    c.draw_rect(0, 0, 20, 20, 1);
    assert_eq!(count(&c, INK8, 0, 0, 20, 20), 25);
    // A new clip replaces the old one entirely.
    c.clip(12, 12, 3, 3);
    c.draw_rect(0, 0, 20, 20, 2);
    assert_eq!(count(&c, RED8, 0, 0, 20, 20), 9);
    assert_eq!(count(&c, INK8, 0, 0, 20, 20), 25);
    c.draw_text(13, 13, FontType::Variable, 30, Align::CENTRE, 1, "W");
    assert_eq!(
        count(&c, INK8, 0, 0, 20, 20) - 25,
        count(&c, INK8, 12, 12, 15, 15)
    );
    // Off the canvas or empty: nothing at all.
    let before = c.pixmap().clone();
    c.clip(30, 30, 5, 5);
    c.draw_rect(0, 0, 20, 20, 0);
    c.clip(2, 2, 0, 4);
    c.draw_line(0, 2, 19, 2, 0);
    c.draw_text(10, 10, FontType::Variable, 12, Align::CENTRE, 0, "8");
    assert_eq!(c.pixmap().data(), before.data());
    c.unclip();
    c.draw_rect(0, 0, 20, 20, 0);
    assert_eq!(count(&c, BG8, 0, 0, 20, 20), 400);
}

#[test]
fn text_draws_near_its_anchor() {
    let mut c = canvas(60, 40);
    c.draw_text(30, 20, FontType::Variable, 20, Align::CENTRE, 1, "15");
    let inked = |x0, y0, x1, y1| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| rgb(&c, x, y) != BG8)
            .count()
    };
    let all = inked(0, 0, 60, 40);
    assert!(all > 50, "some ink: {all}");
    assert_eq!(inked(18, 12, 42, 28), all, "within the centred box");
    // Left of x with right alignment.
    let mut c = canvas(60, 40);
    let right = Align {
        h: HAlign::Right,
        v: VAlign::Baseline,
    };
    c.draw_text(30, 30, FontType::Fixed, 16, right, 1, "42");
    assert_eq!(count(&c, BG8, 30, 0, 60, 40), 30 * 40);
    assert!(count(&c, BG8, 0, 0, 30, 40) < 30 * 40);
}

#[test]
fn blitters_restore_what_they_saved() {
    let mut c = canvas(10, 10);
    c.draw_rect(0, 0, 5, 5, 2);
    let bl = Blitter::new(4, 4);
    c.blitter_save(&bl, 3, 3);
    c.draw_rect(0, 0, 10, 10, 1);
    c.blitter_load(&bl, 3, 3);
    assert_eq!(count(&c, RED8, 3, 3, 5, 5), 4);
    assert_eq!(count(&c, BG8, 3, 3, 7, 7), 12);
    // Saved partly off the canvas: the off-canvas part loads as nothing.
    let edge = Blitter::new(4, 4);
    c.blitter_save(&edge, 8, 8);
    c.draw_rect(0, 0, 10, 10, 2);
    c.blitter_load(&edge, 0, 0);
    assert_eq!(count(&c, INK8, 0, 0, 2, 2), 4);
    assert_eq!(count(&c, RED8, 2, 2, 4, 4), 4);
    // Loading an unsaved blitter does nothing.
    c.blitter_load(&Blitter::new(3, 3), 0, 0);
    assert_eq!(count(&c, INK8, 0, 0, 10, 10), 4);
}

#[test]
fn dropped_blitters_are_forgotten_at_the_next_redraw() {
    let mut c = canvas(10, 10);
    let kept = Blitter::new(2, 2);
    let dropped = Blitter::new(2, 2);
    c.blitter_save(&kept, 0, 0);
    c.blitter_save(&dropped, 0, 0);
    assert_eq!(c.saved_blitters(), 2);
    drop(dropped);
    c.start_draw();
    assert_eq!(c.saved_blitters(), 1);
    c.blitter_save(&Blitter::new(0, 3), 0, 0);
    assert_eq!(c.saved_blitters(), 1, "an empty blitter keeps nothing");
}

#[test]
fn updates_accumulate_until_taken() {
    let mut c = canvas(20, 20);
    c.take_dirty();
    c.draw_update(2, 3, 4, 4);
    c.draw_update(10, 1, 2, 2);
    c.draw_update(18, 18, 10, 10);
    c.draw_update(5, 5, 0, 3);
    assert_eq!(c.take_dirty(), IntRect::from_ltrb(2, 1, 20, 20));
    assert_eq!(c.take_dirty(), None);
}

#[test]
fn text_fallback_picks_what_the_fonts_cover() {
    let c = canvas(4, 4);
    assert_eq!(c.text_fallback(&["\u{1F600}", "×", "x"]), Some("×"));
    assert_eq!(c.text_fallback(&["\u{1F600}"]), None);
}
