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
use puzzles_core::config::ConfigItem;
use puzzles_core::midend::ConfigKind;

use crate::app::command::Command;
use crate::app::paths::AppDirs;
use crate::app::session::Session;

/// A fresh folder for one test's files, never the user's own.
fn test_dirs(name: &str) -> AppDirs {
    let dir = std::env::temp_dir().join(format!("puzzles-ui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    AppDirs::at(dir)
}

/// The app in a 720 × 640 window, keeping its files in `dirs`. Only
/// screenshots need a renderer, so behaviour tests also run on machines
/// without a GPU.
fn app_with(dirs: AppDirs) -> Harness<'static, PuzzlesApp> {
    Harness::builder()
        .with_size(egui::vec2(720.0, 640.0))
        .build_eframe(move |cc| PuzzlesApp::with_dirs(cc, Some(dirs)))
}

fn app() -> Harness<'static, PuzzlesApp> {
    Harness::builder()
        .with_size(egui::vec2(720.0, 640.0))
        .build_eframe(|cc| PuzzlesApp::with_dirs(cc, None))
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
            .build_eframe(|cc| PuzzlesApp::with_dirs(cc, None)),
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
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.step();
    open(&mut h, "Flip");
    open_menu(&mut h, "Type");
    save(&mut h, "menu-type");
    h.key_press(Key::Escape);
    h.run_steps(2);
    open_menu(&mut h, "Game");
    save(&mut h, "menu-game");
    click_label(&mut h, "Enter game ID");
    h.run();
    save(&mut h, "dialog-game-id");
    h.key_press(Key::Escape);
    h.run_steps(2);
    open_menu(&mut h, "Type");
    click_label(&mut h, "Custom…");
    set_field(&mut h, "Width", "0");
    click_label(&mut h, "OK");
    h.run();
    save(&mut h, "dialog-custom-error");
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run();
    save(&mut h, "settings-light");
    h.key_press(Key::Escape);
    h.run_steps(2);

    // The same in Dark, as when the system is dark.
    h.input_mut().system_theme = Some(egui::Theme::Dark);
    h.run_steps(3);
    save(&mut h, "flip-dark");
    h.get_by_label("Settings").click();
    h.run();
    save(&mut h, "settings-dark");
    h.key_press(Key::Escape);
    h.run_steps(2);
    for game in ["Guess", "Fifteen"] {
        h.key_press_modifiers(Modifiers::COMMAND, Key::G);
        h.step();
        if game == "Guess" {
            h.run_steps(2);
            save(&mut h, "library-dark");
        }
        open(&mut h, game);
        save(&mut h, &format!("{}-dark", game.to_lowercase()));
    }

    // Classic: controls must stand out from its grey panel.
    h.state_mut().settings = crate::app::settings::Settings {
        follow_system: false,
        theme: "Classic".to_owned(),
        ..Default::default()
    };
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run();
    save(&mut h, "settings-classic");
}

// ---------------------------------------------------------------------
// Menus and dialogs

/// Opens a toolbar menu (its label is its icon, a space and its name).
fn open_menu(h: &mut Harness<'_, PuzzlesApp>, name: &str) {
    h.get_by_label_contains(&format!(" {name}")).click();
    h.run_steps(2);
}

fn click_label(h: &mut Harness<'_, PuzzlesApp>, label: &str) {
    h.get_by_label_contains(label).click();
    h.run_steps(2);
}

/// Replaces a dialog text field's contents, the way a user would:
/// click it, select all, type.
fn set_field(h: &mut Harness<'_, PuzzlesApp>, label: &str, text: &str) {
    // Let the dialog settle first: it re-centres a frame after it grows
    // (say, when an error appears), and the field moves with it.
    h.run();
    // Press and release in separate frames, as a real click arrives.
    let pos = h
        .get_by_role_and_label(Role::TextInput, label)
        .rect()
        .center();
    for pressed in [true, false] {
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
    assert!(
        h.get_by_role_and_label(Role::TextInput, label).is_focused(),
        "{label} focused"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text(text.to_owned()));
    h.run_steps(1);
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, label)
            .value()
            .as_deref(),
        Some(text)
    );
}

fn dialog_open(h: &Harness<'_, PuzzlesApp>) -> bool {
    h.state().dialog.is_some()
}

#[test]
fn presets_from_the_type_menu_start_new_games() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Flip");
    open_menu(&mut h, "Type");
    click_label(&mut h, "3x3 Random");
    wait_for_game(&mut h);
    assert!(session(&h).game_id().unwrap().starts_with("3x3:"));
    assert_eq!(session(&h).current_preset(), Some(3), "ticked in the menu");
}

#[test]
fn the_presets_file_is_read_when_the_menu_opens() {
    let dirs = test_dirs("presets");
    let mut h = app_with(dirs.clone());
    h.run_steps(2);
    open(&mut h, "Flip");
    std::fs::create_dir_all(dirs.presets().parent().unwrap()).unwrap();
    std::fs::write(
        dirs.presets(),
        "[[flip]]\ntitle = \"Mine: 7x3\"\nparams = \"7x3c\"\n\n[[flip]]\ntitle = \"Bad\"\nparams = \"0x0\"\n",
    )
    .unwrap();
    open_menu(&mut h, "Type");
    h.get_by_label_contains("presets.toml line 5: \"Bad\"");
    click_label(&mut h, "Mine: 7x3");
    wait_for_game(&mut h);
    assert!(session(&h).game_id().unwrap().starts_with("7x3:"));
}

#[test]
fn custom_parameters_show_the_games_error_and_stay_open() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Flip");
    open_menu(&mut h, "Type");
    click_label(&mut h, "Custom…");
    assert!(dialog_open(&h));
    set_field(&mut h, "Width", "0");
    click_label(&mut h, "OK");
    assert!(dialog_open(&h), "rejected");
    h.get_by_label_contains("must both be greater than zero");

    set_field(&mut h, "Width", "4");
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(!dialog_open(&h), "Enter applies: {:?}", h.state().dialog);
    wait_for_game(&mut h);
    assert!(session(&h).game_id().unwrap().starts_with("4x5:"));
}

/// Found in the real window: typing "0" into a fresh dialog made the
/// width 80, as the caret sat after the old value.
#[test]
fn typing_into_a_fresh_dialog_replaces_the_first_field() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Flip");
    open_menu(&mut h, "Type");
    click_label(&mut h, "Custom…");
    h.event(Event::Text("3".to_owned()));
    h.run_steps(1);
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Width")
            .value()
            .as_deref(),
        Some("3")
    );
}

#[test]
fn game_ids_can_be_entered_and_copied() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Flip");
    open_menu(&mut h, "Game");
    click_label(&mut h, "Enter game ID");
    set_field(&mut h, "Game ID", "3x3:d074191345d1644c17058,f80");
    click_label(&mut h, "OK");
    wait_for_game(&mut h);
    assert_eq!(
        session(&h).game_id().as_deref(),
        Some("3x3:d074191345d1644c17058,f80")
    );

    open_menu(&mut h, "Game");
    h.get_by_label("Copy game ID").click();
    h.step();
    let copied = h.output().platform_output.commands.iter().any(
        |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "3x3:d074191345d1644c17058,f80"),
    );
    assert!(copied, "copied to the clipboard");
    h.run_steps(1);
    assert_eq!(session(&h).status_line(), "Game ID copied.");

    // Esc cancels a dialog without changing anything.
    open_menu(&mut h, "Game");
    click_label(&mut h, "Enter random seed");
    assert!(dialog_open(&h));
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!dialog_open(&h));
}

#[test]
fn preferences_are_saved_in_cs_format() {
    let dirs = test_dirs("prefs");
    let mut h = app_with(dirs.clone());
    h.run_steps(2);
    open(&mut h, "Fifteen");
    open_menu(&mut h, "Game");
    click_label(&mut h, "Preferences");
    h.get_by_role(Role::ComboBox).click();
    h.run_steps(2);
    h.get_by_label("Move the gap").click();
    h.run_steps(1);
    click_label(&mut h, "OK");
    assert!(!dialog_open(&h));
    let saved = std::fs::read_to_string(dirs.prefs("Fifteen")).expect("saved");
    assert!(saved.contains("arrow-semantics"), "{saved}");
    assert!(saved.contains("gap"), "{saved}");

    // A new session of the game loads them.
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.step();
    open(&mut h, "Fifteen");
    let prefs = session(&h)
        .config(puzzles_core::midend::ConfigKind::Prefs)
        .unwrap();
    assert!(matches!(
        prefs.items[1].value,
        puzzles_core::config::ConfigValue::Choices { selected: 1, .. }
    ));
    let _ = std::fs::remove_dir_all(dirs.presets().parent().unwrap());
}

/// Found by a flaky run: egui moved keyboard focus off the puzzle on an
/// arrow key, so the next arrow was lost. On this board the first two
/// arrows do nothing, and the third must still arrive.
#[test]
fn arrows_keep_working_after_one_that_does_nothing() {
    let mut h = app();
    h.run_steps(2);
    open(&mut h, "Fifteen");
    let Screen::Game { session: s, .. } = &mut h.state_mut().screen else {
        panic!("a game")
    };
    let id = ConfigItem::string("Game ID", "4x4:8,3,13,11,15,4,2,14,12,10,5,7,1,9,6,0");
    s.apply_config(ConfigKind::Desc, &[id]).expect("a valid ID");
    wait_for_game(&mut h);
    for key in [Key::ArrowLeft, Key::ArrowUp] {
        h.key_press(key);
        h.step();
    }
    assert!(!session(&h).can(Command::Undo), "the gap is in the corner");
    h.key_press(Key::ArrowRight);
    h.step();
    assert!(
        session(&h).can(Command::Undo),
        "the third arrow moved a tile"
    );
    // Also after clicking a toolbar button.
    h.get_by_label("Undo").click();
    h.run_steps(2);
    h.key_press(Key::ArrowRight);
    h.step();
    assert!(
        session(&h).can(Command::Undo),
        "arrows work after a toolbar click"
    );
}

// ---------------------------------------------------------------------
// Themes

fn background(h: &Harness<'_, PuzzlesApp>) -> puzzles_theme::Rgb {
    session(h).background()
}

fn dark_ui(h: &Harness<'_, PuzzlesApp>) -> bool {
    h.ctx.global_style().visuals.dark_mode
}

#[test]
fn themes_follow_the_systems_appearance() {
    let mut h = app();
    h.input_mut().system_theme = Some(egui::Theme::Dark);
    h.run_steps(2);
    open(&mut h, "Flip");
    assert!(dark_ui(&h), "the app's own colours are dark");
    assert!(
        puzzles_theme::lightness(background(&h)) < 0.3,
        "and the puzzle's"
    );

    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run_steps(2);
    assert!(!dark_ui(&h));
    assert!(puzzles_theme::lightness(background(&h)) > 0.9);
}

#[test]
fn settings_choose_a_theme_and_save_it() {
    let dirs = test_dirs("settings");
    let mut h = app_with(dirs.clone());
    h.run_steps(2);
    open(&mut h, "Flip");
    h.get_by_label("Settings").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Follow system appearance")
        .click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Theme").click();
    h.run_steps(2);
    h.get_by_label("Classic").click();
    h.run_steps(1);
    click_label(&mut h, "OK");
    h.run_steps(2);
    assert!(!dialog_open(&h));
    assert_eq!(background(&h), puzzles_theme::Rgb::grey(0.9), "Classic's");
    let saved = std::fs::read_to_string(dirs.settings()).expect("saved");
    assert!(
        saved.contains("follow_system = false") && saved.contains("theme = \"Classic\""),
        "{saved}"
    );

    // The next start uses it.
    let mut h = app_with(dirs.clone());
    h.run_steps(2);
    open(&mut h, "Flip");
    assert_eq!(background(&h), puzzles_theme::Rgb::grey(0.9));
    let _ = std::fs::remove_dir_all(dirs.themes().parent().unwrap());
}

#[test]
fn user_themes_apply_while_the_app_runs() {
    let dirs = test_dirs("user-themes");
    std::fs::create_dir_all(dirs.themes()).unwrap();
    std::fs::write(
        dirs.settings(),
        "follow_system = false\ntheme = \"Sepia\"\n",
    )
    .unwrap();
    let mut h = app_with(dirs.clone());
    h.run_steps(2);
    open(&mut h, "Flip");
    let light = background(&h);
    std::fs::write(
        dirs.themes().join("sepia.toml"),
        "name = \"Sepia\"\nextends = \"Light\"\n[roles]\nbackground = [0.95, 0.9, 0.8]\n",
    )
    .unwrap();
    // The watcher wakes the app; give the OS's file events time.
    let start = Instant::now();
    while background(&h) == light {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the theme applied"
        );
        std::thread::sleep(Duration::from_millis(20));
        h.step();
    }
    assert_eq!(background(&h), puzzles_theme::Rgb::new(0.95, 0.9, 0.8));
    let _ = std::fs::remove_dir_all(dirs.themes().parent().unwrap());
}

/// Clicks a node the way a real mouse does: press and release in
/// separate frames.
fn real_click(h: &mut Harness<'_, PuzzlesApp>, pos: Pos2) {
    for pressed in [true, false] {
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(2);
}

/// A drop-down opened in a dialog is drawn above it, also the second
/// time. In the real macOS window a second opening drew it hidden behind
/// the dialog (see `dialogs::keep_popups_above`); the harness's input
/// timing doesn't reproduce that, so this only guards the layer order.
#[test]
fn drop_downs_stay_above_their_dialog() {
    let mut h = app();
    h.run_steps(2);
    let order = |h: &Harness<'_, PuzzlesApp>| -> Vec<egui::Id> {
        h.ctx.memory(|m| {
            m.layer_ids()
                .filter(|l| l.order == egui::Order::Foreground)
                .map(|l| l.id)
                .collect()
        })
    };
    open(&mut h, "Guess");
    for round in 0..2 {
        let gear = h.get_by_label("Settings").rect().center();
        real_click(&mut h, gear);
        // Cancelled each round, so "Follow system" is ticked again.
        let tick = h
            .get_by_role_and_label(Role::CheckBox, "Follow system appearance")
            .rect()
            .center();
        real_click(&mut h, tick);
        // Press and release in one frame, as synthetic clicks arrive.
        h.get_by_role_and_label(Role::ComboBox, "Theme").click();
        h.run_steps(3);
        assert!(egui::Popup::is_any_open(&h.ctx), "round {round}: open");
        let layers = order(&h);
        let dialog = layers
            .iter()
            .position(|&id| id == egui::Id::new("settings-dialog"));
        assert_eq!(layers.len(), 2, "round {round}: {layers:?}");
        assert_eq!(
            dialog,
            Some(0),
            "round {round}: the list is drawn above the dialog"
        );
        h.key_press(Key::Escape);
        h.run_steps(2);
        h.key_press(Key::Escape);
        h.run_steps(2);
        // As when the themes folder changes: the theme is applied again.
        h.state_mut().applied_theme = None;
        h.run_steps(2);
    }
}
