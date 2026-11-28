//! The mid-end with a real game, without the C oracle: what the oracle
//! tests check against C (`c-oracle/tests/midend_parity.rs`), pinned
//! with known answers.

#![cfg(feature = "flip")]

use puzzles_core::button::{Button, ButtonKind, CursorKey, MouseAction, MouseButton, UiAction};
use puzzles_core::game::GameStatus;
use puzzles_core::midend::{ConfigKind, KeyResult, Midend, MidendOptions};
use puzzles_core::preset::{PresetEntry, PresetMenu};
use puzzles_games::flip::Flip;

fn flip() -> Midend<Flip> {
    let mut me = Midend::new(b"midend test", true, MidendOptions::default());
    me.new_game();
    me.size(500, 500, false, 1.0);
    me
}

fn save(me: &Midend<Flip>) -> String {
    let mut out = Vec::new();
    me.serialise(&mut out).expect("in memory");
    String::from_utf8(out).expect("ASCII")
}

fn click(me: &mut Midend<Flip>, x: i32, y: i32) -> KeyResult {
    let press = me.process_key(x, y, Button::mouse(MouseButton::Left, MouseAction::Press));
    me.process_key(x, y, Button::mouse(MouseButton::Left, MouseAction::Release));
    press
}

/// A seed game ID generates C's puzzle (checked by the oracle tests).
#[test]
fn seed_ids_generate_known_puzzles() {
    let mut me = flip();
    me.game_id("3x3c#42").expect("valid");
    me.new_game();
    assert_eq!(
        me.get_game_id().as_deref(),
        Some("3x3:d074191345d1644c17058,f80")
    );
    assert_eq!(me.get_random_seed().as_deref(), Some("3x3c#42"));
}

#[test]
fn moves_undo_redo_and_restart() {
    let mut me = flip();
    assert!(!me.can_undo());
    assert_eq!(click(&mut me, 60, 60), KeyResult::SomeEffect);
    assert!(me.can_undo() && me.wants_timer(), "the flip animates");
    me.timer(1.0);
    assert!(!me.wants_timer());
    let after_click = save(&me);

    assert_eq!(
        me.process_key(0, 0, Button::new(ButtonKind::Ui(UiAction::Undo))),
        KeyResult::SomeEffect
    );
    assert!(me.can_redo());
    assert_eq!(
        me.process_key(0, 0, Button::key('r')),
        KeyResult::SomeEffect
    );
    me.timer(1.0);
    assert_eq!(save(&me), after_click);

    me.restart_game();
    assert!(save(&me).contains("RESTART :"));
    assert_eq!(me.process_key(0, 0, Button::key('q')), KeyResult::Quit);
}

#[test]
fn solving_marks_the_save_file() {
    let mut me = flip();
    me.solve().expect("Flip solves");
    let text = save(&me);
    assert!(text.contains("\nSOLVE   :26:S"), "{text}");
    let mv = me.current_move().expect("the solve");
    assert!(text.contains(&format!("SOLVE   :26:{mv}\n")));
    me.process_key(0, 0, Button::key('u'));
    assert_eq!(me.current_move(), None, "back at the start");
    assert_eq!(me.status(), GameStatus::Ongoing, "hints only");
}

#[test]
fn save_files_round_trip() {
    let mut me = flip();
    for (x, y) in [(60, 60), (120, 200), (300, 100)] {
        click(&mut me, x, y);
        me.timer(1.0);
    }
    me.process_key(0, 0, Button::key('u'));
    let text = save(&me);

    let mut loaded = Midend::<Flip>::new(b"other", true, MidendOptions::default());
    loaded.deserialise(&mut text.as_bytes()).expect("loads");
    assert_eq!(save(&loaded), text);
    assert!(loaded.can_redo());
    assert_eq!(
        puzzles_core::midend::identify_game(&mut text.as_bytes()).expect("a save file"),
        "Flip"
    );
}

#[test]
fn bad_save_files_are_rejected_with_cs_messages() {
    let good = save(&flip());
    for (text, message) in [
        (
            "hello".to_owned(),
            "Data does not appear to be a saved game file",
        ),
        (
            good.replace("GAME    :4:Flip", "GAME    :4:Flop"),
            "Save file is from a different game",
        ),
        (
            good.replace("VERSION :1:1", "VERSION :1:2"),
            "Cannot handle this version of the saved game file format",
        ),
        (
            good[..good.len() / 2].to_owned(),
            "Saved data ended unexpectedly",
        ),
        (
            good.replace("STATEPOS:1:1", "STATEPOS:1:2"),
            "Game position in save file is out of range",
        ),
        (
            good.replace("NSTATES :1:1", "NSTATES :2:-1"),
            "Number of states in save file was negative",
        ),
    ] {
        let mut me = flip();
        let before = save(&me);
        let err = me.deserialise(&mut text.as_bytes()).expect_err(message);
        assert_eq!(err.to_string(), message);
        assert_eq!(save(&me), before, "a rejected file changes nothing");
    }
}

#[test]
fn new_game_can_be_undone() {
    let mut me = flip();
    click(&mut me, 60, 60);
    me.timer(1.0);
    let old = save(&me);
    me.process_key(0, 0, Button::key('n'));
    let new = save(&me);
    assert_ne!(old, new);
    me.process_key(0, 0, Button::key('u'));
    assert_eq!(save(&me), old);
    me.process_key(0, 0, Button::key('r'));
    me.process_key(0, 0, Button::key('r'));
    assert_eq!(save(&me), new);

    // Not across a change of parameters.
    me.set_params(&{
        let mut p = *me.params();
        p.w = 3;
        p
    });
    me.new_game();
    assert_eq!(me.process_key(0, 0, Button::key('u')), KeyResult::NoEffect);
}

#[test]
fn keyboard_play_and_shortcut_preference() {
    let mut me = flip();
    me.process_key(0, 0, Button::cursor(CursorKey::Right));
    assert!(me.cursor_location().is_some());
    assert_eq!(
        me.process_key(0, 0, Button::key(' ')),
        KeyResult::SomeEffect
    );

    let prefs = me.get_config(ConfigKind::Prefs).expect("prefs dialog");
    assert_eq!(prefs.title, "Flip preferences");
    let mut items = prefs.items;
    items[0].value = puzzles_core::config::ConfigValue::Bool(false);
    me.set_config(ConfigKind::Prefs, &items).expect("valid");
    assert_eq!(me.process_key(0, 0, Button::key('n')), KeyResult::Unused);
    let mut file = Vec::new();
    me.save_prefs(&mut file).expect("in memory");
    assert_eq!(file, b"one-key-shortcuts=false\n");
}

#[test]
fn game_ids_and_presets() {
    let mut me = flip();
    assert_eq!(
        me.game_id("0x3").unwrap_err().message(),
        "Width and height must both be greater than zero"
    );
    me.game_id("3x3:bad").expect_err("bad description");
    assert_eq!(me.presets().entries.len(), 6);
    let id = 4;
    let params = *me.preset_params(id).expect("a preset");
    me.set_params(&params);
    assert_eq!(me.which_preset(), Some(id));
    let config = me.get_config(ConfigKind::Seed).expect("a game");
    assert_eq!(config.title, "Flip random selection");
}

#[test]
fn options_come_from_cs_variables() {
    let env = |name: &str| match name {
        "FLIP_DEFAULT" => Some("3x4r".to_owned()),
        "FLIP_PRESETS" => Some("Big:6x6c:Sub:#:Tiny:2x2:Bad:0x0::#".to_owned()),
        "FLIP_TILESIZE" => Some(" 20px".to_owned()),
        _ => None,
    };
    let options = MidendOptions::from_lookup("Flip", env);
    assert_eq!(options.tilesize, Some(20));
    let mut me = Midend::<Flip>::new(b"o", true, options);
    assert_eq!(Flip_params(&me), (3, 4));
    let menu = me.presets();
    assert_eq!(menu.entries.len(), 8, "6 built in, Big and the Sub menu");
    me.new_game();
    assert_eq!(
        me.size(10_000, 10_000, false, 1.0),
        (20 * 3 + 20, 20 * 4 + 20)
    );
    assert_eq!(
        MidendOptions::from_lookup("Null Game", |n| (n == "NULLGAME_TILESIZE")
            .then(|| "-3".into()))
        .tilesize,
        None
    );
}

#[test]
fn structured_extra_presets_follow_the_built_ins() {
    // What a front end builds from its presets file, then the variable.
    let mut options = MidendOptions::from_lookup("Flip", |name| {
        (name == "FLIP_PRESETS").then(|| "From env:4x4c".to_owned())
    });
    let mut file = PresetMenu::new();
    file.add_preset("Wide: 7 by 3", "7x3r".to_owned());
    let sub = file.add_submenu("Odd");
    sub.add_preset("Broken", "0x0".to_owned());
    sub.add_preset("9x9", "9x9".to_owned());
    file.entries.append(&mut options.extra_presets.entries);
    options.extra_presets = file;

    let mut me = Midend::<Flip>::new(b"x", true, options);
    let titles: Vec<String> = me
        .presets()
        .entries
        .iter()
        .map(|e| match e {
            PresetEntry::Preset { title, .. } => title.clone(),
            PresetEntry::Submenu { title, menu } => format!("{title} ({})", menu.entries.len()),
        })
        .collect();
    assert_eq!(
        titles[6..],
        ["Wide: 7 by 3", "Odd (1)", "From env"],
        "the file, its invalid preset dropped, then the variable"
    );
    let wide = me
        .presets()
        .presets()
        .find(|(t, _)| *t == "Wide: 7 by 3")
        .map(|(_, &id)| id);
    let params = *me.preset_params(wide.expect("listed")).expect("a preset");
    assert_eq!((params.w, params.h), (7, 3));
    me.set_params(&params);
    assert_eq!(me.which_preset(), wide);
}

#[allow(non_snake_case, reason = "reads like the type")]
fn Flip_params(me: &Midend<Flip>) -> (i32, i32) {
    (me.params().w, me.params().h)
}

#[test]
fn registry_gives_erased_mid_ends() {
    let info = puzzles_games::find("Flip").expect("Flip is built in");
    let mut puzzle = info.new_midend(b"seed", true, MidendOptions::default());
    puzzle.new_game();
    puzzle.size(400, 400, false, 2.0);
    assert!(puzzle.capabilities().can_solve);
    assert!(puzzle.set_preset(0));
    assert_eq!(puzzle.which_preset(), Some(0));
    let mut out = Vec::new();
    puzzle.serialise(&mut out).expect("in memory");
    assert!(puzzle.deserialise(&mut out.as_slice()).is_ok());
    assert!(
        puzzles_games::find("Null Game").is_none(),
        "the template stays out"
    );
}

#[test]
fn new_game_splits_into_begin_run_and_finish() {
    // The same seeds, so the same random game either way.
    let mut whole = Midend::<Flip>::new(b"split", true, MidendOptions::default());
    whole.new_game();
    let mut split = Midend::<Flip>::new(b"split", true, MidendOptions::default());
    let job = split.begin_new_game();
    assert!(job.generates());
    assert_eq!(
        split.get_game_id(),
        None,
        "nothing changes before finishing"
    );
    let generated = std::thread::spawn(move || job.run()).join().unwrap();
    assert!(split.finish_new_game(generated.clone()));
    assert_eq!(split.get_game_id(), whole.get_game_id());
    assert_eq!(split.get_random_seed(), whole.get_random_seed());
    assert!(!split.finish_new_game(generated), "a job finishes once");

    // A descriptive ID needs no generation.
    split.game_id("3x3:d074191345d1644c17058,f80").unwrap();
    let job = split.begin_new_game();
    assert!(!job.generates());
    assert!(split.finish_new_game(job.run()));
    assert_eq!(
        split.get_game_id().as_deref(),
        Some("3x3:d074191345d1644c17058,f80")
    );
}

#[test]
fn stale_new_game_jobs_change_nothing() {
    let mut me = flip();
    let before = me.get_game_id();
    let params = *me.params();
    let job = me.begin_new_game();
    me.set_params(&params);
    assert!(!me.finish_new_game(job.run()), "new parameters");
    let first = me.begin_new_game();
    let _second = me.begin_new_game();
    assert!(!me.finish_new_game(first.run()), "a later New Game");
    let mut other = flip();
    assert!(
        !other.finish_new_game(me.begin_new_game().run()),
        "another mid-end's job"
    );
    // A dropped job leaves the game as it was.
    drop(me.begin_new_game());
    assert_eq!(me.get_game_id(), before);
    let generated = me.begin_new_game().run();
    assert!(me.finish_new_game(generated));
    assert_ne!(me.get_game_id(), before);

    // A new game ID while generating: the ID's game comes next.
    let job = me.begin_new_game();
    me.game_id("4x4").unwrap();
    assert!(!me.finish_new_game(job.run()));
    let generated = me.begin_new_game().run();
    assert!(me.finish_new_game(generated));
    assert!(me.get_game_id().unwrap().starts_with("4x4:"));
}

#[test]
fn deferred_new_game_keys_ask_the_front_end() {
    let options = MidendOptions {
        defer_new_game: true,
        ..MidendOptions::default()
    };
    let mut me = Midend::<Flip>::new(b"defer", true, options);
    me.new_game();
    let id = me.get_game_id();
    assert_eq!(me.process_key(0, 0, Button::key('n')), KeyResult::NewGame);
    assert_eq!(
        me.process_key(0, 0, Button::new(ButtonKind::Ui(UiAction::NewGame))),
        KeyResult::NewGame
    );
    assert_eq!(me.get_game_id(), id, "the front end starts it");
}

/// Jobs cross threads.
const _: fn() = || {
    fn send<T: Send>() {}
    send::<puzzles_core::midend::NewGameJob>();
    send::<puzzles_core::midend::GeneratedGame>();
};

#[test]
fn extra_presets_can_be_replaced_and_checked() {
    let mut me = flip();
    assert_eq!(me.presets().entries.len(), 6);
    let mut extra = PresetMenu::new();
    extra.add_preset("Big", "7x7c".to_owned());
    me.set_extra_presets(extra);
    assert_eq!(me.presets().entries.len(), 7, "rebuilt with the new extra");
    me.set_extra_presets(PresetMenu::new());
    assert_eq!(me.presets().entries.len(), 6);
    assert!(Midend::<Flip>::check_params("7x7c").is_ok());
    assert_eq!(
        Midend::<Flip>::check_params("0x0").unwrap_err().message(),
        "Width and height must both be greater than zero"
    );
}

/// Between choosing the next game and installing it (New Game on another
/// thread), everything that describes a game describes the one on
/// screen; Cancel puts the choice back.
#[test]
fn a_pending_choice_leaves_the_shown_game_described() {
    let mut me = flip();
    click(&mut me, 50, 50);
    let (id, seed, before) = (me.get_game_id(), me.get_random_seed(), save(&me));
    let params = Flip_params(&me);

    me.game_id("4x4c#123").expect("valid");
    let job = me.begin_new_game();
    assert_eq!(me.get_game_id(), id, "not the pending game's");
    assert_eq!(me.get_random_seed(), seed);
    let dialog = me.get_config(ConfigKind::Seed).expect("a game");
    assert_eq!(dialog.items[0].as_str(), seed.as_deref());
    // The save file is the game on screen; PARAMS are the next game's,
    // as in C.
    let pending = save(&me);
    assert_eq!(
        pending
            .lines()
            .filter(|l| !l.starts_with("PARAMS"))
            .collect::<Vec<_>>(),
        before
            .lines()
            .filter(|l| !l.starts_with("PARAMS"))
            .collect::<Vec<_>>()
    );
    let mut reloaded = Midend::<Flip>::new(b"reload", true, MidendOptions::default());
    reloaded
        .deserialise(&mut pending.as_bytes())
        .expect("loads");
    assert_eq!(reloaded.get_game_id(), id);
    // Restart replays the game on screen.
    me.restart_game();
    assert_eq!(me.get_game_id(), id);

    // Cancel: the job is stale and the choice gone.
    assert!(me.cancel_new_game());
    assert!(!me.finish_new_game(job.run()));
    assert_eq!(Flip_params(&me), params);
    assert_eq!((me.get_game_id(), me.get_random_seed()), (id, seed));
    let params_line = |s: &str| {
        s.lines()
            .find(|l| l.starts_with("PARAMS"))
            .map(str::to_owned)
    };
    assert_eq!(params_line(&save(&me)), params_line(&before));
    assert!(!me.cancel_new_game(), "nothing left to cancel");
    me.new_game();
    assert_eq!(Flip_params(&me), params, "New Game keeps the old size");

    // Finishing installs the choice.
    me.game_id("4x4c#123").unwrap();
    me.new_game();
    assert_eq!(me.get_random_seed().as_deref(), Some("4x4c#123"));
    assert!(!me.cancel_new_game());
}

#[test]
fn a_cancelled_custom_size_is_forgotten() {
    let mut me = flip();
    let params = Flip_params(&me);
    let mut items = me.get_config(ConfigKind::Settings).unwrap().items;
    items[0] = puzzles_core::config::ConfigItem::string("Width", "40");
    me.set_config(ConfigKind::Settings, &items).expect("valid");
    assert_eq!(Flip_params(&me).0, 40);
    let _job = me.begin_new_game();
    assert!(me.cancel_new_game());
    assert_eq!(Flip_params(&me), params);
    let dialog = me.get_config(ConfigKind::Settings).unwrap();
    let width = params.0.to_string();
    assert_eq!(
        dialog.items[0].as_str(),
        Some(width.as_str()),
        "the dialog too"
    );
}
