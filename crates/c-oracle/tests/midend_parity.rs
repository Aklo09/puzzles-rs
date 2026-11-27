//! Differential tests: the Rust mid-end against C's `midend.c`, both
//! driving Flip.
//!
//! * Random front-end scripts (sizing, new games, keys and mouse events
//!   with modifiers, timer ticks, undo/redo including undoing a New Game,
//!   restart, solve, game IDs, presets, parameters, configuration
//!   dialogs, preferences, save and reload), compared after every step:
//!   the whole save file, key results, timer requests, undo/redo
//!   availability, status, text format, game ID and seed, preset, tile
//!   size, cursor, status bar text, and full redraws.
//! * Loading damaged save files and preference files: same verdict and
//!   message, and the same state after.
//! * `identify_game`, and the `%g` / `atof` conversions of the `TIME`
//!   line.
//!
//! Mid-end-level draw sequences may differ from C by design (deferred
//! redraws); only full redraws at the same moment are compared.

#![cfg(feature = "oracle")]

use c_oracle::{CGame, CMidend, c_atof, c_format_g, c_identify_game};
use proptest::prelude::*;
use puzzles_core::button::Button;
use puzzles_core::config::{ConfigItem, ConfigValue};
use puzzles_core::drawing::RecordingDrawing;
use puzzles_core::game::{Game, GameStatus};
use puzzles_core::midend::{Config, ConfigKind, KeyResult, Midend, MidendOptions, identify_game};
use puzzles_core::parse::{atof, format_g};
use puzzles_games::flip::{Flip, MatrixType, Params};

fn pkr(r: KeyResult) -> i32 {
    match r {
        KeyResult::Quit => 0,
        KeyResult::SomeEffect => 1,
        KeyResult::NoEffect => 2,
        KeyResult::Unused => 3,
        // Only with `defer_new_game`, which C doesn't have.
        KeyResult::NewGame => unreachable!("not deferred here"),
    }
}

fn cfg_code(kind: ConfigKind) -> i32 {
    match kind {
        ConfigKind::Settings => 0,
        ConfigKind::Seed => 1,
        ConfigKind::Desc => 2,
        ConfigKind::Prefs => 3,
    }
}

/// A dialog as the C harness prints it.
fn config_text(config: &Config) -> String {
    use std::fmt::Write as _;
    let mut s = format!("{}\n", config.title);
    for item in &config.items {
        let _ = write!(s, "{}|{}|", item.name, item.keyword.unwrap_or("-"));
        let _ = match &item.value {
            ConfigValue::String(v) => write!(s, "string|{v}"),
            ConfigValue::Choices {
                names,
                keywords,
                selected,
            } => {
                let kws = keywords.map_or_else(String::new, |k| format!("/{}", k.join(",")));
                write!(s, "choices|{selected}|{}{kws}", names.join(","))
            }
            ConfigValue::Bool(b) => write!(s, "bool|{}", u8::from(*b)),
        };
        s += "\n";
    }
    s
}

/// Fills a dialog's items from the harness's value strings.
fn fill(mut items: Vec<ConfigItem>, values: &[String]) -> Vec<ConfigItem> {
    for (item, v) in items.iter_mut().zip(values) {
        match &mut item.value {
            ConfigValue::String(s) => s.clone_from(v),
            ConfigValue::Choices { selected, .. } => *selected = v.parse().unwrap_or(0),
            ConfigValue::Bool(b) => *b = v.starts_with('1'),
        }
    }
    items
}

#[derive(Clone, Debug)]
enum Op {
    Size {
        w: i32,
        h: i32,
        user: bool,
        dpr: f64,
    },
    NewGame,
    Key {
        x: i32,
        y: i32,
        button: i32,
    },
    Timer(f32),
    Restart,
    Solve,
    StopAnim,
    GameId(String),
    SetParams(String),
    Preset(i32),
    Config(ConfigKind, Vec<String>),
    SaveAndLoad,
    ForceRedraw,
}

struct Both {
    r: Midend<Flip>,
    c: CMidend,
    has_game: bool,
    presets_built: bool,
    /// Parameters or game ID changed since the last New Game or load.
    params_changed: bool,
}

impl Both {
    fn new(seed: &[u8]) -> Self {
        Self {
            r: Midend::new(seed, true, MidendOptions::default()),
            c: CMidend::new(CGame::flip(), seed, true),
            has_game: false,
            presets_built: false,
            params_changed: false,
        }
    }

    /// What C front ends do after new parameters or a game ID: start a
    /// game with them at once. (In between, C's mid-end can crash: a
    /// seed ID followed by a parameters-only one leaves it generating
    /// from a NULL seed.)
    fn start_new_game(&mut self) {
        self.r.new_game();
        self.c.new_game();
        self.params_changed = false;
    }

    /// After a key started a new game or undid or redid one. Undoing a
    /// New Game brings back the old game with its live UI in Rust and a
    /// fresh one in C (mid-end decision (c)). Check the games agree, then
    /// load C's into both so the UIs agree again.
    fn resync_after_game_swap(&mut self) -> Result<(), TestCaseError> {
        prop_assert_eq!(
            (self.r.can_undo(), self.r.can_redo()),
            self.c.can_undo_redo(),
            "can undo/redo after a game swap"
        );
        let mut r = Vec::new();
        self.r.serialise(&mut r).expect("in memory");
        let saved = self.c.serialise();
        prop_assert_eq!(
            String::from_utf8_lossy(&r),
            String::from_utf8_lossy(&saved),
            "save file after a game swap"
        );
        prop_assert!(self.r.deserialise(&mut saved.as_slice()).is_ok());
        prop_assert_eq!(self.c.deserialise(&saved), Ok(()));
        Ok(())
    }

    /// What a front end does after each event: redraw.
    fn frame(&mut self) {
        self.c.redraw();
        if self.r.is_dirty() {
            self.r.redraw(&mut RecordingDrawing::default());
        }
    }

    fn step(&mut self, op: &Op) -> Result<(), TestCaseError> {
        // Parameters can also get there indirectly (a descriptive ID
        // keeps the shape type); the next generation or solve would then
        // never end, or take days.
        if unplayable(self.r.params()) {
            return Err(TestCaseError::reject("too slow to play"));
        }
        match op {
            // C's midend_size crashes before the first game (front ends
            // always start one first); Rust's doesn't care.
            &Op::Size { w, h, user, dpr } if self.has_game => {
                prop_assert_eq!(self.r.size(w, h, user, dpr), self.c.size(w, h, user, dpr));
            }
            Op::NewGame => {
                self.r.new_game();
                self.c.new_game();
                self.has_game = true;
                self.params_changed = false;
            }
            &Op::Key { x, y, button } if self.has_game => {
                let b = Button::from_raw(button).expect("scripts use valid codes");
                // Each generated game has a new seed, so this changes
                // whenever the game does.
                let id_before = (self.r.get_game_id(), self.r.get_random_seed());
                let r = pkr(self.r.process_key(x, y, b));
                prop_assert_eq!(r, self.c.process_key(x, y, button), "key {:#x}", button);
                if (self.r.get_game_id(), self.r.get_random_seed()) != id_before {
                    self.resync_after_game_swap()?;
                }
            }
            &Op::Timer(dt) if self.has_game => {
                self.r.timer(dt);
                self.c.timer(dt);
            }
            // Between new parameters or a new game ID and the next New
            // Game, C may restart from a description that doesn't match
            // the parameters (or a NULL one) and crash; Rust does nothing.
            Op::Restart if self.has_game && !self.params_changed => {
                self.r.restart_game();
                self.c.restart_game();
            }
            Op::Solve if self.has_game => {
                let r = self.r.solve().map_err(|e| e.message().to_owned());
                prop_assert_eq!(r, self.c.solve());
            }
            Op::StopAnim if self.has_game => {
                self.r.stop_anim();
                self.c.stop_anim();
            }
            Op::GameId(id) => {
                let r = self.r.game_id(id).map_err(|e| e.message().to_owned());
                prop_assert_eq!(&r, &self.c.game_id(id), "game id {:?}", id);
                if r.is_ok() {
                    self.start_new_game();
                }
            }
            Op::SetParams(p) => {
                let mut params = Flip::default_params();
                Flip::decode_params(&mut params, p);
                self.r.set_params(&params);
                self.c.set_params(p);
                self.start_new_game();
            }
            &Op::Preset(id) => {
                let n = self.c.presets();
                let menu_ids = count_ids(self.r.presets());
                prop_assert_eq!(menu_ids, n as usize);
                self.presets_built = true;
                let r_ok = self.r.preset_params(id as usize).copied().is_some_and(|p| {
                    self.r.set_params(&p);
                    true
                });
                prop_assert_eq!(r_ok, self.c.set_preset(id));
                if r_ok {
                    self.start_new_game();
                }
            }
            Op::Config(kind, values) => {
                let r = self.r.get_config(*kind);
                let c = self.c.get_config(cfg_code(*kind));
                prop_assert_eq!(
                    r.as_ref().map(config_text),
                    c.clone(),
                    "get_config {:?}",
                    kind
                );
                if let Some(config) = r {
                    let items = fill(config.items, values);
                    let r = self
                        .r
                        .set_config(*kind, &items)
                        .map_err(|e| e.message().to_owned());
                    let values: Vec<&str> = values.iter().map(String::as_str).collect();
                    prop_assert_eq!(
                        &r,
                        &self.c.set_config(cfg_code(*kind), &values),
                        "set_config {:?}",
                        kind
                    );
                    if r.is_ok() && *kind != ConfigKind::Prefs {
                        self.start_new_game();
                    }
                }
            }
            Op::SaveAndLoad if self.has_game => {
                let saved = self.c.serialise();
                let r = self
                    .r
                    .deserialise(&mut saved.as_slice())
                    .map_err(|e| e.to_string());
                prop_assert_eq!(r, Ok(()));
                prop_assert_eq!(self.c.deserialise(&saved), Ok(()));
                self.params_changed = false;
            }
            Op::ForceRedraw if self.has_game => {
                let c = self.c.force_redraw();
                self.r.force_redraw();
                let mut rec = RecordingDrawing::default();
                self.r.redraw(&mut rec);
                prop_assert_eq!(rec.to_string(), c, "force_redraw");
            }
            _ => {}
        }
        self.frame();
        self.compare()
    }

    fn compare(&mut self) -> Result<(), TestCaseError> {
        prop_assert_eq!(self.r.tilesize(), self.c.tilesize());
        prop_assert_eq!(self.r.wants_timer(), self.c.wants_timer(), "wants_timer");
        prop_assert_eq!(
            (self.r.can_undo(), self.r.can_redo()),
            self.c.can_undo_redo(),
            "can undo/redo"
        );
        let status = match self.r.status() {
            GameStatus::Ongoing => 0,
            GameStatus::Won => 1,
            GameStatus::Lost => -1,
        };
        prop_assert_eq!(status, self.c.status());
        prop_assert_eq!(self.r.get_random_seed(), self.c.get_random_seed(), "seed");
        if self.presets_built {
            let r = self.r.which_preset().map_or(-1, |id| id as i32);
            prop_assert_eq!(r, self.c.which_preset(), "which preset");
        }
        if self.has_game {
            prop_assert_eq!(self.r.text_format(), self.c.text_format(), "text");
            // C asserts when a new game ID has cleared the description
            // and no game has started from it yet; Rust says None.
            if let Some(id) = self.r.get_game_id() {
                prop_assert_eq!(Some(id), self.c.get_game_id(), "game id");
            }
            let cursor = self.r.cursor_location().map(|r| (r.x, r.y, r.w, r.h));
            prop_assert_eq!(cursor, self.c.cursor(), "cursor");
            prop_assert_eq!(
                self.r.status_text(),
                self.c.status_text().unwrap_or_default(),
                "status bar"
            );
            let mut r = Vec::new();
            self.r.serialise(&mut r).expect("in memory");
            let c = self.c.serialise();
            prop_assert_eq!(
                String::from_utf8_lossy(&r),
                String::from_utf8_lossy(&c),
                "save file"
            );
        }
        let mut r = Vec::new();
        self.r.save_prefs(&mut r).expect("in memory");
        prop_assert_eq!(r, self.c.save_prefs(), "prefs file");
        Ok(())
    }
}

fn count_ids(menu: &puzzles_core::preset::PresetMenu<usize>) -> usize {
    use puzzles_core::preset::PresetEntry;
    menu.entries
        .iter()
        .map(|e| match e {
            PresetEntry::Preset { .. } => 1,
            PresetEntry::Submenu { menu, .. } => 1 + count_ids(menu),
        })
        .sum()
}

// ---------------------------------------------------------------------
// Scripts

/// Button codes a front end may send, modifiers included.
fn button() -> impl Strategy<Value = i32> {
    let mouse = (
        0x200..=0x208,
        prop::sample::select(vec![0, 0, 0, 0x1000, 0x2000]),
    )
        .prop_map(|(b, m)| b | m);
    let arrows = (
        0x209..=0x20e,
        prop::sample::select(vec![0, 0x1000, 0x2000, 0x3000]),
    )
        .prop_map(|(b, m)| b | m);
    let ui = 0x210..=0x214;
    let keys = prop::sample::select(b"nNuUrRqQsx1 \r\n\x08\x7f\x13\x0e\x1a\x12\x19*#\t".to_vec())
        .prop_map(i32::from);
    let ctrl = (
        prop::sample::select(b"nuqrszZ".to_vec()),
        prop::sample::select(vec![0x1000, 0x3000]),
    )
        .prop_map(|(c, m)| i32::from(c) | m);
    let keypad = (b'0'..=b'9').prop_map(|d| i32::from(d) | 0x4000);
    prop_oneof![5 => mouse, 4 => arrows, 1 => ui, 3 => keys, 1 => ctrl, 1 => keypad]
}

fn game_id() -> impl Strategy<Value = String> {
    prop_oneof![
        "[1-5]x[1-5][cr]?",
        "[1-5]x[1-5][cr]?#[0-9a-z]{0,8}",
        "#[0-9]{1,6}",
        "3x3:[0-9a-f]{21},[0-9a-f]{3}",
        "2x2:[0-9a-f]{4},[0-9a-f]{1}",
        // Junk, with digit runs short enough for an int: C's atoi on
        // longer ones is platform-dependent (Windows saturates).
        "([0-9]{0,4}[x:#,a-f]){0,3}[0-9]{0,4}",
    ]
}

fn op() -> impl Strategy<Value = Op> {
    // User resizes only at whole pixel ratios: C keeps the ratio of a
    // chosen size in an int, Rust exactly (see the mid-end's docs).
    let size = (
        1..400_i32,
        1..400_i32,
        any::<bool>(),
        prop::sample::select(vec![2_u8, 3, 4]),
    )
        .prop_map(|(w, h, user, half_dpr)| Op::Size {
            w,
            h,
            user,
            dpr: f64::from(if user { half_dpr & !1 } else { half_dpr }) / 2.0,
        });
    let key =
        (-20..300_i32, -20..300_i32, button()).prop_map(|(x, y, button)| Op::Key { x, y, button });
    let timer = prop::sample::select(vec![0.01_f32, 0.05, 0.1, 0.3, 1.0]).prop_map(Op::Timer);
    let config = prop_oneof![
        ("[1-6]", "[1-6]", "[01]")
            .prop_map(|(w, h, t)| Op::Config(ConfigKind::Settings, vec![w, h, t])),
        game_id().prop_map(|id| Op::Config(ConfigKind::Seed, vec![id])),
        game_id().prop_map(|id| Op::Config(ConfigKind::Desc, vec![id])),
        "[01]".prop_map(|v| Op::Config(ConfigKind::Prefs, vec![v])),
    ];
    prop_oneof![
        2 => size,
        3 => Just(Op::NewGame),
        20 => key,
        6 => timer,
        1 => Just(Op::Restart),
        1 => Just(Op::Solve),
        1 => Just(Op::StopAnim),
        2 => game_id().prop_map(Op::GameId),
        1 => "[1-5]x[1-5][cr]?".prop_map(Op::SetParams),
        1 => (-1..8_i32).prop_map(Op::Preset),
        2 => config,
        1 => Just(Op::SaveAndLoad),
        2 => Just(Op::ForceRedraw),
    ]
}

fn script() -> impl Strategy<Value = (Vec<u8>, Vec<Op>)> {
    (
        prop::collection::vec(any::<u8>(), 0..8),
        prop::collection::vec(op(), 0..60),
    )
}

/// Parameters a test can't play: random 1 × 2 and 2 × 1 shapes never
/// generate (in C neither), and big grids are valid but far too slow (a
/// 166 × 166 grid has a 760 MB matrix and a cubic solver, as in C).
fn unplayable(p: &Params) -> bool {
    let area = i64::from(p.w) * i64::from(p.h);
    (p.matrix_type == MatrixType::Random && area == 2) || area > 49
}

/// Whether an op would set unplayable parameters (and start a game with
/// them).
fn hangs(op: &Op) -> bool {
    let s = match op {
        Op::GameId(s) | Op::SetParams(s) => s.as_str(),
        Op::Config(ConfigKind::Seed | ConfigKind::Desc, v) => v[0].as_str(),
        Op::Config(ConfigKind::Settings, v) => {
            return v[2] == "1"
                && v[0].parse::<i32>().unwrap_or(0) * v[1].parse::<i32>().unwrap_or(0) == 2;
        }
        _ => return false,
    };
    let mut p = Flip::default_params();
    Flip::decode_params(&mut p, s.split([':', '#']).next().unwrap_or(""));
    Flip::validate_params(&p, true).is_ok() && unplayable(&p)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn scripts_match((seed, ops) in script()) {
        prop_assume!(!ops.iter().any(hangs));
        let mut both = Both::new(&seed);
        both.step(&Op::NewGame)?;
        both.step(&Op::Size { w: 300, h: 300, user: false, dpr: 1.0 })?;
        for op in &ops {
            both.step(op)?;
        }
    }
}

/// The New Game undo: back to the old game, and redo into the new one,
/// but not across a change of parameters.
#[test]
fn new_game_can_be_undone_alike() {
    let mut both = Both::new(b"undo");
    let steps = [
        Op::NewGame,
        Op::Size {
            w: 300,
            h: 300,
            user: false,
            dpr: 1.0,
        },
        Op::Key {
            x: 40,
            y: 40,
            button: 0x200,
        },
        Op::Key {
            x: 40,
            y: 40,
            button: 0x206,
        },
        Op::Timer(1.0),
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'n'),
        },
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'u'),
        },
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'u'),
        },
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'r'),
        },
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'r'),
        },
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'r'),
        },
        Op::SetParams("4x4".into()),
        Op::NewGame,
        Op::Key {
            x: 0,
            y: 0,
            button: i32::from(b'u'),
        },
        Op::ForceRedraw,
    ];
    for op in &steps {
        both.step(op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    }
}

// ---------------------------------------------------------------------
// Loading damaged files

/// A save file from a random game played on C.
fn c_save(seed: &[u8], clicks: &[(i32, i32)], solve: bool) -> Vec<u8> {
    let mut c = CMidend::new(CGame::flip(), seed, true);
    c.new_game();
    c.size(300, 300, false, 1.0);
    for &(x, y) in clicks {
        c.process_key(x, y, 0x200);
        c.process_key(x, y, 0x206);
        c.timer(1.0);
    }
    if solve {
        let _ = c.solve();
    }
    c.process_key(0, 0, i32::from(b'u'));
    c.serialise()
}

fn mutate(mut data: Vec<u8>, at: prop::sample::Index, edit: u8, byte: u8) -> Vec<u8> {
    let i = at.index(data.len() + 1);
    match edit {
        0 => data.truncate(i),
        1 => data.insert(i, byte),
        2 if i < data.len() => data[i] = byte,
        3 if i < data.len() => {
            data.remove(i);
        }
        _ => {}
    }
    data
}

fn edit_byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        prop::sample::select(b"0129:\n\r SMx#,-".to_vec()),
        any::<u8>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn damaged_save_files_load_alike(
        seed in prop::collection::vec(any::<u8>(), 0..6),
        clicks in prop::collection::vec((0..300_i32, 0..300_i32), 0..6),
        solve: bool,
        edits in prop::collection::vec((any::<prop::sample::Index>(), 0..5_u8, edit_byte()), 0..3),
    ) {
        let mut data = c_save(&seed, &clicks, solve);
        for (at, edit, byte) in edits {
            data = mutate(data, at, edit, byte);
        }
        let mut c = CMidend::new(CGame::flip(), b"x", true);
        let mut r = Midend::<Flip>::new(b"x", true, MidendOptions::default());
        let c_result = c.deserialise(&data);
        let r_result = r.deserialise(&mut data.as_slice()).map_err(|e| e.to_string());
        prop_assert_eq!(&r_result, &c_result);
        if r_result.is_ok() {
            let mut out = Vec::new();
            r.serialise(&mut out).expect("in memory");
            let c_out = c.serialise();
            if out != c_out {
                // C writes loaded move strings back verbatim; Rust writes
                // the moves it decoded, canonically (D4). The game must be
                // the same: C reads Rust's file back to Rust's bytes.
                let mut again = CMidend::new(CGame::flip(), b"x", true);
                prop_assert_eq!(again.deserialise(&out), Ok(()));
                prop_assert_eq!(&again.serialise(), &out);
                prop_assert_eq!(r.text_format(), c.text_format());
            }
        }
        prop_assert_eq!(
            identify_game(&mut data.as_slice()).map_err(|e| e.to_string()),
            c_identify_game(&data)
        );
    }

    #[test]
    fn damaged_prefs_load_alike(
        base in prop::sample::select(vec![
            &b"one-key-shortcuts=false\n"[..],
            b"one-key-shortcuts=true",
            b"# comment\n\none-key-shortcuts=false\nunknown=1\n",
            b"one-key-shortcuts=false\n continued\n",
        ]),
        edits in prop::collection::vec((any::<prop::sample::Index>(), 0..5_u8, edit_byte()), 0..3),
        with_game: bool,
    ) {
        let mut data = base.to_vec();
        for (at, edit, byte) in edits {
            data = mutate(data, at, edit, byte);
        }
        let mut c = CMidend::new(CGame::flip(), b"p", true);
        let mut r = Midend::<Flip>::new(b"p", true, MidendOptions::default());
        if with_game {
            c.new_game();
            c.size(300, 300, false, 1.0);
            r.new_game();
            r.size(300, 300, false, 1.0);
        }
        let c_result = c.load_prefs(&data);
        let r_result = r.load_prefs(&mut data.as_slice()).map_err(|e| e.to_string());
        prop_assert_eq!(r_result, c_result);
        let mut out = Vec::new();
        r.save_prefs(&mut out).expect("in memory");
        prop_assert_eq!(out, c.save_prefs());
        // Preferences reach the next game's UI.
        c.new_game();
        c.size(300, 300, false, 1.0);
        r.new_game();
        r.size(300, 300, false, 1.0);
        let n = Button::key('n');
        prop_assert_eq!(pkr(r.process_key(0, 0, n)), c.process_key(0, 0, i32::from(b'n')));
    }
}

/// A save file's records, as (key, value).
fn records(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut rest = data;
    while rest.len() > 9 {
        let key = String::from_utf8_lossy(&rest[..8]).trim_end().to_owned();
        let after = &rest[9..];
        let colon = after.iter().position(|&b| b == b':').expect("C wrote it");
        let len: usize = std::str::from_utf8(&after[..colon])
            .unwrap()
            .parse()
            .unwrap();
        let value = after[colon + 1..colon + 1 + len].to_vec();
        out.push((key, value));
        rest = &after[colon + 2 + len..];
    }
    out
}

fn render(records: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in records {
        out.extend(format!("{key:<8}:{}:", value.len()).as_bytes());
        out.extend(value);
        out.push(b'\n');
    }
    out
}

#[derive(Clone, Debug)]
enum LineEdit {
    Delete(prop::sample::Index),
    Duplicate(prop::sample::Index),
    Swap(prop::sample::Index, prop::sample::Index),
    SetValue(prop::sample::Index, Vec<u8>),
    Insert(prop::sample::Index, String, Vec<u8>),
    /// NSTATES or STATEPOS moved by -2..=2 from its value.
    Nudge(bool, i32),
}

fn line_edit() -> impl Strategy<Value = LineEdit> {
    let value = prop_oneof![
        prop::sample::select(vec![
            &b""[..],
            b"0",
            b"-1",
            b"1",
            b"2",
            b"3",
            b"12",
            // Big but allocatable: C allocates NSTATES entries up front,
            // and an int-sized count makes it abort (Windows' atoi
            // saturates where Linux and macOS wrap).
            b"1000000",
            b"abc",
            b"5x5c",
            b"3x3",
            b"0x0",
            b"M1,1",
            b"S",
            b"\x01",
            b"1.5",
            b"nan",
            b"8,8",
            b"Flip",
            b"Null Game",
        ])
        .prop_map(<[u8]>::to_vec),
        "[ -~]{0,8}".prop_map(String::into_bytes),
    ];
    let key = prop::sample::select(vec![
        "SAVEFILE", "VERSION", "GAME", "PARAMS", "CPARAMS", "SEED", "HEXSEED", "DESC", "PRIVDESC",
        "AUXINFO", "UI", "TIME", "NSTATES", "STATEPOS", "MOVE", "SOLVE", "RESTART", "JUNK",
    ])
    .prop_map(str::to_owned);
    prop_oneof![
        any::<prop::sample::Index>().prop_map(LineEdit::Delete),
        any::<prop::sample::Index>().prop_map(LineEdit::Duplicate),
        (any::<prop::sample::Index>(), any::<prop::sample::Index>())
            .prop_map(|(a, b)| LineEdit::Swap(a, b)),
        (any::<prop::sample::Index>(), value.clone()).prop_map(|(i, v)| LineEdit::SetValue(i, v)),
        (any::<prop::sample::Index>(), key, value).prop_map(|(i, k, v)| LineEdit::Insert(i, k, v)),
        (any::<bool>(), -2..=2_i32).prop_map(|(n, d)| LineEdit::Nudge(n, d)),
    ]
}

fn apply(recs: &mut Vec<(String, Vec<u8>)>, edit: &LineEdit) {
    let n = recs.len();
    match edit {
        LineEdit::Delete(i) if n > 0 => {
            recs.remove(i.index(n));
        }
        LineEdit::Duplicate(i) if n > 0 => {
            let r = recs[i.index(n)].clone();
            recs.insert(i.index(n), r);
        }
        LineEdit::Swap(a, b) if n > 0 => recs.swap(a.index(n), b.index(n)),
        LineEdit::SetValue(i, v) if n > 0 => recs[i.index(n)].1.clone_from(v),
        LineEdit::Insert(i, k, v) => recs.insert(i.index(n + 1), (k.clone(), v.clone())),
        &LineEdit::Nudge(nstates, d) => {
            let key = if nstates { "NSTATES" } else { "STATEPOS" };
            if let Some((_, v)) = recs.iter_mut().find(|(k, _)| k == key) {
                let old: i32 = std::str::from_utf8(v)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                *v = (old + d).to_string().into_bytes();
            }
        }
        _ => {}
    }
}

/// Loads `data` into fresh C and Rust mid-ends and compares the verdict
/// and the game after.
fn compare_load(data: &[u8]) -> Result<(), TestCaseError> {
    let mut c = CMidend::new(CGame::flip(), b"x", true);
    let mut r = Midend::<Flip>::new(b"x", true, MidendOptions::default());
    let c_result = c.deserialise(data);
    let r_result = r.deserialise(&mut &data[..]).map_err(|e| e.to_string());
    prop_assert_eq!(&r_result, &c_result);
    if r_result.is_ok() {
        let mut out = Vec::new();
        r.serialise(&mut out).expect("in memory");
        if out != c.serialise() {
            // Loaded move strings are written back canonically (D4).
            let mut again = CMidend::new(CGame::flip(), b"x", true);
            prop_assert_eq!(again.deserialise(&out), Ok(()));
            prop_assert_eq!(&again.serialise(), &out);
            prop_assert_eq!(r.text_format(), c.text_format());
        }
    }
    prop_assert_eq!(
        identify_game(&mut &data[..]).map_err(|e| e.to_string()),
        c_identify_game(data)
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(800))]

    /// Damage by whole lines (deleted, repeated, swapped, replaced,
    /// inserted, counts off by one), which reaches every check C makes.
    #[test]
    fn line_damaged_save_files_load_alike(
        seed in prop::collection::vec(any::<u8>(), 0..6),
        clicks in prop::collection::vec((0..300_i32, 0..300_i32), 0..5),
        solve: bool,
        edits in prop::collection::vec(line_edit(), 1..4),
    ) {
        let mut recs = records(&c_save(&seed, &clicks, solve));
        for edit in &edits {
            apply(&mut recs, edit);
        }
        compare_load(&render(&recs))?;
    }
}

/// The save files shipped with upstream (its icon sources) load, and
/// write back byte for byte; Flip's seed regenerates its description.
#[test]
fn upstream_save_files_round_trip() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/puzzles-c/icons");
    // Git may check the submodule out with CRLF line ends (Windows);
    // loaders skip CRs between lines, writers use LF.
    let read = |name: &str| {
        let bytes = std::fs::read(dir.join(name)).expect("an icon save file");
        String::from_utf8(bytes)
            .expect("ASCII")
            .replace("\r\n", "\n")
            .into_bytes()
    };
    let flip = read("flip.sav");
    let mut r = Midend::<Flip>::new(b"x", true, MidendOptions::default());
    r.deserialise(&mut flip.as_slice()).expect("loads");
    let mut out = Vec::new();
    r.serialise(&mut out).expect("in memory");
    assert_eq!(
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&flip)
    );

    let seed = r.get_random_seed().expect("flip.sav has a seed");
    let desc = r
        .get_game_id()
        .expect("a game")
        .split_once(':')
        .unwrap()
        .1
        .to_owned();
    let mut fresh = Midend::<Flip>::new(b"y", true, MidendOptions::default());
    fresh.game_id(&seed).expect("valid seed ID");
    fresh.new_game();
    assert_eq!(
        fresh.get_game_id().unwrap().split_once(':').unwrap().1,
        desc
    );

    let null = read("nullgame.sav");
    let mut r =
        Midend::<puzzles_games::nullgame::NullGame>::new(b"x", true, MidendOptions::default());
    r.deserialise(&mut null.as_slice()).expect("loads");
    let mut out = Vec::new();
    r.serialise(&mut out).expect("in memory");
    assert_eq!(out, null);
}

// ---------------------------------------------------------------------
// The TIME line

proptest! {
    #[test]
    fn format_g_matches_printf(bits: u32) {
        let x = f64::from(f32::from_bits(bits));
        // C's NaN output depends on the platform ("-nan" on glibc).
        prop_assume!(!x.is_nan());
        prop_assert_eq!(format_g(x), c_format_g(x));
    }

    #[test]
    fn format_g_matches_printf_on_times(t in 0.0_f32..100_000.0) {
        prop_assert_eq!(format_g(f64::from(t)), c_format_g(f64::from(t)));
    }

    #[test]
    fn atof_matches_c(s in "[ +-]?[0-9]{0,6}(\\.[0-9]{0,6})?([eE][+-]?[0-9]{0,3})?[x.e]?") {
        let (r, c) = (atof(s.as_bytes()), c_atof(s.as_bytes()));
        prop_assert!(r.to_bits() == c.to_bits() || (r.is_nan() && c.is_nan()), "{} vs {}", r, c);
    }
}
