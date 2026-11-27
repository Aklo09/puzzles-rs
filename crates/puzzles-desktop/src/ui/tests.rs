//! The real app driven headlessly by `egui_kittest`: widgets are found
//! through the accessibility tree (AccessKit), input is synthetic, and
//! frames can be rendered with wgpu.
//!
//! `screenshots` writes PNGs of the main screens to
//! `target/ui-screenshots/` for review. It asserts nothing about pixels
//! and skips rendering where no GPU adapter is available.

use std::time::{Duration, Instant};

use eframe::egui::accesskit::Role;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::{PuzzlesApp, Screen};
use crate::app::command::Command;
use crate::app::session::Session;

/// The app in a 720 × 640 window. Only screenshots need a renderer, so
/// behaviour tests also run on machines without a GPU (Linux CI).
fn app() -> Harness<'static, PuzzlesApp> {
    Harness::builder()
        .with_size(egui::vec2(720.0, 640.0))
        .build_eframe(|cc| PuzzlesApp::new(cc))
}

/// The app with a wgpu renderer, or `None` (with a note) where wgpu
/// finds no adapter, such as CI machines without a GPU or Mesa's
/// lavapipe. The harness itself would panic there.
fn rendering_app() -> Option<Harness<'static, PuzzlesApp>> {
    let setup = egui_kittest::wgpu::default_wgpu_setup();
    let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &setup else {
        unreachable!("kittest creates its own instance")
    };
    let instance = pollster::block_on(setup.new_instance());
    let adapters =
        pollster::block_on(instance.enumerate_adapters(create.instance_descriptor.backends));
    if adapters.is_empty() {
        eprintln!("skipped: no wgpu adapter (install a GPU driver or Mesa's lavapipe)");
        return None;
    }
    Some(
        Harness::builder()
            .with_size(egui::vec2(720.0, 640.0))
            .wgpu()
            .build_eframe(|cc| PuzzlesApp::new(cc)),
    )
}

fn session<'h>(h: &'h Harness<'_, PuzzlesApp>) -> &'h Session {
    match &h.state().screen {
        Screen::Game { session, .. } => session,
        Screen::Library => panic!("no game open"),
    }
}

/// Steps frames until the New Game in progress is installed.
fn wait_for_game(h: &mut Harness<'_, PuzzlesApp>) {
    let start = Instant::now();
    loop {
        h.step();
        if let Screen::Game { session, .. } = &h.state().screen
            && !session.is_generating()
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "generation timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    h.run_steps(2);
}

fn open(h: &mut Harness<'_, PuzzlesApp>, name: &str) {
    h.get_by_role_and_label(Role::Button, name).click();
    h.step();
    assert!(
        matches!(h.state().screen, Screen::Game { .. }),
        "{name} opened"
    );
    wait_for_game(h);
}

/// Where the puzzle is, in points.
fn puzzle_rect(h: &Harness<'_, PuzzlesApp>) -> egui::Rect {
    match &h.state().screen {
        Screen::Game { view, .. } => view.shown().expect("drawn"),
        Screen::Library => panic!("no game open"),
    }
}

fn click(h: &mut Harness<'_, PuzzlesApp>, pos: Pos2) {
    for pressed in [true, false] {
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
}

#[test]
fn the_library_lists_every_game() {
    let mut h = app();
    h.run_steps(2);
    for info in puzzles_games::GAMES {
        h.get_by_role_and_label(Role::Button, info.name);
    }
    assert!(matches!(h.state().screen, Screen::Library));
}

#[test]
fn a_game_opens_and_plays_with_the_mouse() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Flip");
    assert!(!session(&h).can(Command::Undo));
    let rect = puzzle_rect(&h);
    assert!(rect.width() > 200.0, "fills the window: {rect:?}");
    click(&mut h, rect.center());
    assert!(session(&h).can(Command::Undo), "the click made a move");

    h.get_by_label("Undo").click();
    h.step();
    assert!(session(&h).can(Command::Redo) && !session(&h).can(Command::Undo));
    h.get_by_label("Redo").click();
    h.step();
    assert!(session(&h).can(Command::Undo));

    h.get_by_label("Games").click();
    h.step();
    assert!(matches!(h.state().screen, Screen::Library));
}

#[test]
fn keys_reach_the_game_and_shortcuts_the_app() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Fifteen");
    // Fifteen moves a tile with each arrow key; some arrows hit the edge.
    for key in [
        Key::ArrowLeft,
        Key::ArrowUp,
        Key::ArrowRight,
        Key::ArrowDown,
    ] {
        h.key_press(key);
        h.step();
    }
    assert!(session(&h).can(Command::Undo), "arrows moved tiles");
    while session(&h).can(Command::Undo) {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.step();
    }
    assert!(session(&h).can(Command::Redo), "Cmd/Ctrl+Z undid");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.step();
    assert!(session(&h).can(Command::Undo), "Shift+Cmd/Ctrl+Z redid");

    // 'n' is the mid-end's New Game key: it generates off the UI thread.
    let before = session(&h).game_id();
    h.event(Event::Text("n".to_owned()));
    h.step();
    wait_for_game(&mut h);
    assert_ne!(session(&h).game_id(), before, "a new game");

    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.step();
    assert!(matches!(h.state().screen, Screen::Library));
}

#[test]
fn solve_from_the_toolbar() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Guess");
    h.get_by_label("Solve").click();
    h.step();
    assert!(session(&h).can(Command::Undo), "solving is a move");
}

/// Drags pegs around Guess through the whole app (input batched per
/// frame, one redraw, partial texture uploads), then checks that the
/// pixels on screen are the canvas's: nothing a drag drew is left behind.
#[test]
fn dragging_leaves_nothing_behind_on_screen() {
    let Some(mut h) = rendering_app() else {
        return;
    };
    h.run_steps(2);
    open(&mut h, "Guess");
    let rect = puzzle_rect(&h);
    let at = |fx: f32, fy: f32| {
        Pos2::new(
            rect.min.x + rect.width() * fx,
            rect.min.y + rect.height() * fy,
        )
    };
    // From the colour pegs (left) to the guess holes and beyond, with
    // several moves per frame.
    let drags = [
        ((0.08, 0.30), (0.40, 0.06)),
        ((0.08, 0.38), (0.55, 0.06)),
        ((0.08, 0.46), (0.95, 0.50)),
        ((0.08, 0.54), (0.30, 0.95)),
    ];
    for (from, to) in drags {
        let (a, b) = (at(from.0, from.1), at(to.0, to.1));
        h.event(Event::PointerMoved(a));
        h.event(Event::PointerButton {
            pos: a,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        for i in 1..=24 {
            let t = i as f32 / 24.0;
            h.event(Event::PointerMoved(a + (b - a) * t));
            if i % 3 == 0 {
                h.step();
            }
        }
        h.event(Event::PointerButton {
            pos: b,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.run_steps(2);
    }
    // The harness draws its own pointer; keep it out of the comparison.
    h.remove_cursor();
    h.run_steps(2);
    let frame = h.render().expect("rendered");
    let Screen::Game { session, .. } = &mut h.state_mut().screen else {
        panic!("a game")
    };
    let (canvas, _) = session.canvas_mut().expect("a canvas");
    let ppp = 1.0; // the harness's pixels per point
    let (ox, oy) = ((rect.min.x * ppp) as u32, (rect.min.y * ppp) as u32);
    let mut differ = Vec::new();
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            let c = canvas.pixmap().pixel(x, y).unwrap();
            let f = frame.get_pixel(ox + x, oy + y).0;
            let close = [c.red(), c.green(), c.blue()]
                .iter()
                .zip(f)
                .all(|(a, b)| a.abs_diff(b) <= 2);
            if !close {
                differ.push((x, y));
            }
        }
    }
    if !differ.is_empty() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-screenshots");
        std::fs::create_dir_all(&dir).unwrap();
        canvas
            .pixmap()
            .save_png(dir.join("drag-canvas.png"))
            .unwrap();
        frame.save(dir.join("drag-screen.png")).unwrap();
    }
    assert!(
        differ.is_empty(),
        "{} screen pixels differ from the canvas, e.g. {:?}",
        differ.len(),
        &differ[..differ.len().min(8)]
    );
}

/// Saves what the app looks like; see the module docs.
#[test]
fn screenshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-screenshots");
    let Some(mut h) = rendering_app() else {
        return;
    };
    h.run_steps(2);
    let save = |h: &mut Harness<'_, PuzzlesApp>, name: &str| {
        let image = h.render().expect("rendered");
        let size = puzzles_canvas::tiny_skia::IntSize::from_wh(image.width(), image.height())
            .expect("a frame");
        let pixmap = puzzles_canvas::tiny_skia::Pixmap::from_vec(image.into_raw(), size)
            .expect("RGBA pixels");
        std::fs::create_dir_all(&dir).unwrap();
        pixmap.save_png(dir.join(format!("{name}.png"))).unwrap();
    };
    save(&mut h, "library");
    for game in ["Flip", "Fifteen", "Guess"] {
        open(&mut h, game);
        save(&mut h, &game.to_lowercase());
        h.key_press_modifiers(Modifiers::COMMAND, Key::G);
        h.step();
    }
    open(&mut h, "Guess");
    h.event(Event::Text("h".to_owned()));
    h.step();
    h.run_steps(2);
    save(&mut h, "guess-hint");
}
