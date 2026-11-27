//! Canvas snapshots: positions of each game drawn by `puzzles-canvas` in
//! the Classic theme, compared with checked-in PNGs (canvas details
//! decision in `docs/porting-plan.md`).
//!
//! These compare Rust with Rust: the C comparison of drawing is the
//! draw-trace oracle test. A pixel passes if every channel is within
//! [`TOLERANCE`] of the snapshot, which absorbs anti-aliasing
//! differences between CPU architectures.
//!
//! To accept new pictures, run with `UPDATE_SNAPSHOTS=1` and review the
//! PNGs in `tests/snapshots/canvas/`. A failing test writes
//! `<name>.new.png` next to the snapshot.

#![cfg(all(feature = "fifteen", feature = "flip", feature = "guess"))]

use std::path::PathBuf;

use puzzles_canvas::Canvas;
use puzzles_canvas::tiny_skia::Pixmap;
use puzzles_core::button::{Button, CursorKey, MouseAction, MouseButton};
use puzzles_core::erased::Puzzle;
use puzzles_core::midend::MidendOptions;
use puzzles_theme::ThemeSet;

/// The largest difference allowed in any channel of any pixel.
const TOLERANCE: u8 = 2;

fn snapshot_path(name: &str, suffix: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/canvas")
        .join(format!("{name}{suffix}"))
}

/// Compares `canvas` with the snapshot `name`, or writes it when
/// `UPDATE_SNAPSHOTS` is set.
fn check(name: &str, canvas: &Canvas) {
    let path = snapshot_path(name, ".png");
    let new = snapshot_path(name, ".new.png");
    let actual = canvas.pixmap();
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        actual.save_png(&path).unwrap();
        let _ = std::fs::remove_file(&new);
        return;
    }
    let fail = |why: String| -> ! {
        actual.save_png(&new).unwrap();
        panic!(
            "{name}: {why}; wrote {} (UPDATE_SNAPSHOTS=1 accepts it)",
            new.display()
        );
    };
    let Ok(expected) = Pixmap::load_png(&path) else {
        fail(format!("no snapshot at {}", path.display()));
    };
    if (expected.width(), expected.height()) != (actual.width(), actual.height()) {
        fail(format!(
            "size {}×{}, snapshot {}×{}",
            actual.width(),
            actual.height(),
            expected.width(),
            expected.height()
        ));
    }
    let (mut worst, mut differing) = (0, 0);
    for (a, e) in actual.data().iter().zip(expected.data()) {
        let d = a.abs_diff(*e);
        worst = worst.max(d);
        differing += usize::from(d > TOLERANCE);
    }
    if differing > 0 {
        fail(format!(
            "{differing} channel values differ by more than {TOLERANCE} (up to {worst})"
        ));
    }
    let _ = std::fs::remove_file(&new);
}

/// A puzzle and its canvas, sized at the game's tile size.
struct Scene {
    puzzle: Box<dyn Puzzle>,
    canvas: Canvas,
}

impl Scene {
    /// `game` is the name, as in save files ("Flip").
    fn new(game: &str, setup: impl FnOnce(&mut dyn Puzzle), scale: f64) -> Self {
        Self::new_in("classic", game, setup, scale)
    }

    fn new_in(theme: &str, game: &str, setup: impl FnOnce(&mut dyn Puzzle), scale: f64) -> Self {
        let info = puzzles_games::find(game).expect("a game");
        let mut puzzle = info.new_midend(b"canvas", true, MidendOptions::default());
        setup(puzzle.as_mut());
        let (w, h) = puzzle.size(i32::MAX, i32::MAX, false, scale);
        let colours = ThemeSet::builtin()
            .resolve(theme, puzzle.name(), puzzle.palette())
            .expect("built-in themes cover every game");
        let mut canvas = Canvas::new(w as u32, h as u32, &colours).expect("a size");
        puzzle.redraw(&mut canvas);
        Self { puzzle, canvas }
    }

    fn id(game: &str, id: &str) -> Self {
        Self::new(
            game,
            |p| {
                p.game_id(id).expect("a valid ID");
                p.new_game();
            },
            1.0,
        )
    }

    fn icon(game: &str, scale: f64) -> Self {
        Self::icon_in("classic", game, scale)
    }

    fn icon_in(theme: &str, game: &str, scale: f64) -> Self {
        let path = format!(
            "{}/../../third_party/puzzles-c/icons/{}.sav",
            env!("CARGO_MANIFEST_DIR"),
            game.to_lowercase()
        );
        let save = std::fs::read(path).expect("upstream's icon save file");
        Self::new_in(
            theme,
            game,
            |p| p.deserialise(&mut &save[..]).expect("loads"),
            scale,
        )
    }

    /// Sends a button, finishes any animation and redraws.
    fn press(&mut self, x: i32, y: i32, button: Button) -> &mut Self {
        self.puzzle.process_key(x, y, button);
        self.puzzle.stop_anim();
        self.puzzle.redraw(&mut self.canvas);
        self
    }

    fn click(&mut self, x: i32, y: i32) -> &mut Self {
        self.press(x, y, Button::mouse(MouseButton::Left, MouseAction::Press));
        self.press(x, y, Button::mouse(MouseButton::Left, MouseAction::Release))
    }
}

#[test]
fn flip() {
    let mut scene = Scene::id("Flip", "5x5c#1");
    check("flip-start", &scene.canvas);
    scene.click(48, 48);
    check("flip-after-click", &scene.canvas);
}

#[test]
fn fifteen() {
    let mut scene = Scene::id("Fifteen", "4x4#1");
    check("fifteen-start", &scene.canvas);
    scene.press(0, 0, Button::cursor(CursorKey::Left));
    check("fifteen-after-move", &scene.canvas);
    check("fifteen-icon-2x", &Scene::icon("Fifteen", 2.0).canvas);
}

#[test]
fn guess() {
    // Upstream's icon: a finished game with marked guesses.
    check("guess-icon", &Scene::icon("Guess", 1.0).canvas);
    let mut scene = Scene::id("Guess", "c6p4g10Bm#1");
    scene.press(0, 0, Button::key('h'));
    check("guess-hint", &scene.canvas);
    // Drag the first colour (a blitter saves what is under the peg).
    let before = scene.canvas.pixmap().clone();
    scene.press(
        31,
        121,
        Button::mouse(MouseButton::Left, MouseAction::Press),
    );
    scene.press(
        150,
        260,
        Button::mouse(MouseButton::Left, MouseAction::Drag),
    );
    assert_ne!(scene.canvas.pixmap(), &before, "the peg follows the mouse");
    check("guess-dragging", &scene.canvas);
    // Dropping it restores the background under the peg.
    scene.press(
        150,
        260,
        Button::mouse(MouseButton::Left, MouseAction::Release),
    );
    check("guess-dropped", &scene.canvas);
    // Labels: text on every peg.
    scene.press(0, 0, Button::key('l'));
    check("guess-labels", &scene.canvas);
}

/// Every game in the app's own themes: upstream's icon positions, which
/// show most of each game's colours.
#[test]
fn themes() {
    for theme in ["light", "dark"] {
        for game in ["Flip", "Fifteen", "Guess"] {
            let scene = Scene::icon_in(theme, game, 1.0);
            check(&format!("{}-{theme}", game.to_lowercase()), &scene.canvas);
        }
    }
}
