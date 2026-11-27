//! The mid-end (port of third_party/puzzles-c/midend.c, upstream
//! 616da16): the game-independent layer between a front end and a
//! [`Game`]. It keeps the undo chain, generates puzzles from seeds,
//! parses game IDs, runs animations and the timer, sizes the puzzle,
//! and reads and writes save files and preferences.
//!
//! Differences from C, all decided (`docs/porting-plan.md`):
//! * It never draws by itself and never calls the front end. Events mark
//!   it dirty; the front end calls [`Midend::redraw`] when
//!   [`Midend::is_dirty`], and polls [`Midend::wants_timer`] and
//!   [`Midend::take_id_changed`] (C's `activate_timer`,
//!   `deactivate_timer` and id-change callback).
//! * The random seed comes in [`Midend::new`] (C's `get_random_seed`),
//!   and environment settings in [`MidendOptions`].
//! * The undo chain holds typed moves; text exists only in save files.
//! * Undoing a New Game swaps back a typed snapshot of the old game
//!   instead of re-loading a serialised copy.
//! * The pixel ratio of a tile size the user chose is kept exactly; C
//!   keeps it in an `int`, so after a resize at 1.5× it later converts
//!   sizes as if from 1×.
//! * Calls C only survives in the order its front ends make them
//!   (sizing before the first game, restarting or starting a game right
//!   after changing parameters or the game ID) work here too.
//!
//! Everything a user can exchange with C (game IDs, generated puzzles,
//! save files, preference files) behaves as in C.

mod options;
mod prefs;
mod savefile;

pub use options::MidendOptions;
pub use savefile::{DeserialiseError, identify_game};

use crate::button::Button;
use crate::config::{ConfigItem, ConfigValue};
use crate::drawing::{Drawing, DrawingApi};
use crate::game::{Game, GameCtx, GameError, GameFlags, GameStatus, KeyLabel, MoveResult, Rect};
use crate::preset::{PresetEntry, PresetMenu};
use crate::random::RandomState;

// C's button codes, for the key handling that works on them directly.
const LEFT_BUTTON: i32 = 0x200;
const RIGHT_BUTTON: i32 = 0x202;
const LEFT_DRAG: i32 = 0x203;
const RIGHT_DRAG: i32 = 0x205;
const LEFT_RELEASE: i32 = 0x206;
const RIGHT_RELEASE: i32 = 0x208;
const CURSOR_UP: i32 = 0x209;
const CURSOR_DOWN: i32 = 0x20a;
const CURSOR_LEFT: i32 = 0x20b;
const CURSOR_RIGHT: i32 = 0x20c;
const CURSOR_SELECT: i32 = 0x20d;
const CURSOR_SELECT2: i32 = 0x20e;
const UI_LOWER_BOUND: i32 = 0x20f;
const UI_QUIT: i32 = 0x210;
const UI_NEWGAME: i32 = 0x211;
const UI_SOLVE: i32 = 0x212;
const UI_UNDO: i32 = 0x213;
const UI_REDO: i32 = 0x214;
const UI_UPPER_BOUND: i32 = 0x215;
const MOD_STYLUS: i32 = 0x0800;
const MOD_CTRL: i32 = 0x1000;
const MOD_SHFT: i32 = 0x2000;
const MOD_NUM_KEYPAD: i32 = 0x4000;
const MOD_MASK: i32 = 0x7800;

const fn strip(button: i32) -> i32 {
    button & !MOD_MASK
}
const fn is_mouse_down(b: i32) -> bool {
    b >= LEFT_BUTTON && b <= RIGHT_BUTTON
}
const fn is_mouse_drag(b: i32) -> bool {
    b >= LEFT_DRAG && b <= RIGHT_DRAG
}
const fn is_mouse_release(b: i32) -> bool {
    b >= LEFT_RELEASE && b <= RIGHT_RELEASE
}
const fn is_cursor_move(b: i32) -> bool {
    matches!(b, CURSOR_UP | CURSOR_DOWN | CURSOR_LEFT | CURSOR_RIGHT)
}
const fn is_ui_fake_key(b: i32) -> bool {
    b > UI_LOWER_BOUND && b < UI_UPPER_BOUND
}

/// What [`Midend::process_key`] did (C's `PKR_*`). Ordered as C's
/// codes, so combining two results keeps the smaller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyResult {
    /// The user asked to quit (`PKR_QUIT`).
    Quit,
    /// Something changed (`PKR_SOME_EFFECT`).
    SomeEffect,
    /// Understood, but nothing changed (`PKR_NO_EFFECT`).
    NoEffect,
    /// Not used: the front end may handle it (`PKR_UNUSED`).
    Unused,
}

/// Which configuration dialog (C's `CFG_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfigKind {
    /// The game's "Custom" parameters (`CFG_SETTINGS`).
    Settings,
    /// A random seed game ID, `params#seed` (`CFG_SEED`).
    Seed,
    /// A descriptive game ID, `params:desc` (`CFG_DESC`).
    Desc,
    /// Preferences (`CFG_PREFS`).
    Prefs,
}

/// A configuration dialog: its title and fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The window title, such as "Flip configuration".
    pub title: String,
    /// The fields.
    pub items: Vec<ConfigItem>,
}

/// How the state of an undo-chain entry was reached (C's `movetype`
/// with its `movestr`).
#[derive(Clone, Debug, PartialEq)]
enum Step<M> {
    /// The start of the game (only the first entry).
    NewGame,
    /// A move.
    Move(M),
    /// The solver's move.
    Solve(M),
    /// A restart, from this description.
    Restart(String),
}

impl<M> Step<M> {
    /// C's `special()`: anything but a plain move.
    const fn is_special(&self) -> bool {
        !matches!(self, Self::Move(_))
    }
}

struct Entry<G: Game> {
    state: G::State,
    step: Step<G::Move>,
}

impl<G: Game> Clone for Entry<G> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            step: self.step.clone(),
        }
    }
}

/// Where the next game comes from (C's `genmode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenMode {
    /// A seed was given: generate from it.
    Seed,
    /// A description was given: play it.
    Desc,
    /// Nothing: invent a seed.
    Nothing,
}

/// A whole game, kept to undo a New Game (what C serialises for it).
struct Snapshot<G: Game> {
    params: G::Params,
    curparams: Option<G::Params>,
    seed: Option<Vec<u8>>,
    desc: Option<String>,
    privdesc: Option<String>,
    aux: Option<String>,
    states: Vec<Entry<G>>,
    statepos: usize,
    ui: Option<G::Ui>,
    elapsed: f32,
}

/// The built preset menu, with C's ids.
struct Presets<P> {
    /// The menu, each preset carrying its id.
    menu: PresetMenu<usize>,
    /// Parameters by id (`None` for submenus), and their full encodings
    /// (C's `encoded_presets`).
    params: Vec<Option<(P, String)>>,
}

/// The mid-end for one game type (C's `midend`).
#[allow(clippy::struct_excessive_bools, reason = "C's midend flags")]
pub struct Midend<G: Game> {
    rng: RandomState,
    interactive: bool,
    options: MidendOptions,
    presets: Option<Presets<G::Params>>,

    /// Long-term parameters: what New Game uses.
    params: G::Params,
    /// The current game's full parameters (C's `curparams`).
    curparams: Option<G::Params>,
    /// The public description (C's `desc`), and the private one save
    /// files use if different (C's `privdesc`; only Mines).
    desc: Option<String>,
    privdesc: Option<String>,
    seed: Option<Vec<u8>>,
    aux: Option<String>,
    genmode: GenMode,

    states: Vec<Entry<G>>,
    /// How many entries are current: `states[statepos - 1]` is shown.
    statepos: usize,
    newgame_undo: Option<Box<Snapshot<G>>>,
    newgame_redo: Option<Box<Snapshot<G>>>,
    newgame_can_store_undo: bool,

    drawstate: Option<G::DrawState>,
    first_draw: bool,
    ui: Option<G::Ui>,

    oldstate: Option<G::State>,
    anim_time: f32,
    anim_pos: f32,
    flash_time: f32,
    flash_pos: f32,
    dir: i32,

    timing: bool,
    elapsed: f32,
    last_status: Option<String>,

    pressed_mouse_button: i32,

    /// The preferences applied to every new UI (C's `be_prefs`, kept as
    /// items rather than serialised text).
    saved_prefs: Vec<ConfigItem>,

    preferred_tilesize: i32,
    preferred_tilesize_dpr: f64,
    tilesize: i32,
    winwidth: i32,
    winheight: i32,

    one_key_shortcuts: bool,

    timer_active: bool,
    dirty: bool,
    id_changed: bool,
}

impl<G: Game> Midend<G> {
    /// A mid-end with no game yet (C's `midend_new`). `seed` seeds the
    /// generator that invents random seeds for new games (C's
    /// `get_random_seed`). `interactive` is false for batch generation
    /// (C's `midend_new` without a drawing API).
    #[must_use]
    pub fn new(seed: &[u8], interactive: bool, options: MidendOptions) -> Self {
        let mut params = G::default_params();
        if let Some(default) = &options.default_params {
            G::decode_params(&mut params, default);
        }
        let mut me = Self {
            rng: RandomState::new(seed),
            interactive,
            options,
            presets: None,
            params,
            curparams: None,
            desc: None,
            privdesc: None,
            seed: None,
            aux: None,
            genmode: GenMode::Nothing,
            states: Vec::new(),
            statepos: 0,
            newgame_undo: None,
            newgame_redo: None,
            newgame_can_store_undo: false,
            drawstate: None,
            first_draw: true,
            ui: None,
            oldstate: None,
            anim_time: 0.0,
            anim_pos: 0.0,
            flash_time: 0.0,
            flash_pos: 0.0,
            dir: 0,
            timing: false,
            elapsed: 0.0,
            last_status: None,
            pressed_mouse_button: 0,
            saved_prefs: Vec::new(),
            preferred_tilesize: 0,
            preferred_tilesize_dpr: 1.0,
            tilesize: 0,
            winwidth: 0,
            winheight: 0,
            one_key_shortcuts: true,
            timer_active: false,
            dirty: false,
            id_changed: false,
        };
        me.reset_tilesize();
        me
    }

    // ------------------------------------------------------------------
    // Front-end polling (replacing C's callbacks)

    /// Whether the front end should call [`timer`](Self::timer)
    /// regularly: an animation, a flash or the game timer is running.
    #[must_use]
    pub fn wants_timer(&self) -> bool {
        self.timer_active
    }

    /// Whether the puzzle needs redrawing.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether the game ID changed since the last call (C's
    /// `midend_request_id_changes` callback).
    pub fn take_id_changed(&mut self) -> bool {
        std::mem::take(&mut self.id_changed)
    }

    // ------------------------------------------------------------------
    // Sizes

    /// Back to the game's preferred tile size (C's
    /// `midend_reset_tilesize`), or the one in the options.
    pub fn reset_tilesize(&mut self) {
        self.preferred_tilesize = self.options.tilesize.unwrap_or(G::PREFERRED_TILESIZE);
        self.preferred_tilesize_dpr = 1.0;
    }

    /// The tile size in use.
    #[must_use]
    pub fn tilesize(&self) -> i32 {
        self.tilesize
    }

    /// `compute_size` with the current UI, or a preferences-only one
    /// before the first game (C passes NULL; no game reads it).
    fn compute_size(&self, params: &G::Params, tilesize: i32) -> (i32, i32) {
        match &self.ui {
            Some(ui) => G::compute_size(params, tilesize, ui),
            None => G::compute_size(params, tilesize, &G::new_ui(None)),
        }
    }

    fn size_new_drawstate(&mut self) {
        if self.tilesize > 0 {
            (self.winwidth, self.winheight) = self.compute_size(&self.params, self.tilesize);
            if let Some(ds) = &mut self.drawstate {
                G::set_size(ds, &self.params, self.tilesize);
            }
        }
    }

    /// C's `convert_tilesize`: the tile size at `new_dpr` giving the
    /// default parameters the size `old_tilesize` gives at `old_dpr`.
    fn convert_tilesize(&self, old_tilesize: i32, old_dpr: f64, new_dpr: f64) -> i32 {
        #[expect(clippy::float_cmp, reason = "C's exact comparison")]
        if new_dpr == old_dpr {
            return old_tilesize;
        }
        let defaults = G::default_params();
        let (x, y) = self.compute_size(&defaults, old_tilesize);
        let x = (f64::from(x) * (new_dpr / old_dpr)) as i32;
        let y = (f64::from(y) * (new_dpr / old_dpr)) as i32;
        let fits = |ts| {
            let (rx, ry) = self.compute_size(&defaults, ts);
            rx <= x && ry <= y
        };
        let (mut min, mut max) = (1, 1);
        loop {
            max *= 2;
            if !fits(max) || max >= 1 << 30 {
                break;
            }
        }
        while max - min > 1 {
            let mid = i32::midpoint(max, min);
            if fits(mid) {
                min = mid;
            } else {
                max = mid;
            }
        }
        min
    }

    /// Chooses the tile size for an area of at most `max_w` × `max_h`
    /// pixels and returns the puzzle's size (C's `midend_size`). With
    /// `user_size`, the largest tile size that fits, which becomes the
    /// preferred one; otherwise at most the preferred tile size, scaled
    /// to `device_pixel_ratio`.
    pub fn size(
        &mut self,
        max_w: i32,
        max_h: i32,
        user_size: bool,
        device_pixel_ratio: f64,
    ) -> (i32, i32) {
        // A drawstate can't be sized twice: start a fresh one.
        if self.drawstate.is_some() && self.tilesize > 0 {
            self.drawstate = Some(G::new_drawstate(&self.states[0].state));
            self.first_draw = true;
        }

        let fits = |me: &Self, ts| {
            let (rx, ry) = me.compute_size(&me.params, ts);
            rx <= max_w && ry <= max_h
        };
        let mut max;
        if user_size {
            max = 1;
            loop {
                max *= 2;
                if !fits(self, max) || max >= 1 << 30 {
                    break;
                }
            }
        } else {
            max = self.convert_tilesize(
                self.preferred_tilesize,
                self.preferred_tilesize_dpr,
                device_pixel_ratio,
            ) + 1;
        }
        let mut min = 1;
        while max - min > 1 {
            let mid = i32::midpoint(max, min);
            if fits(self, mid) {
                min = mid;
            } else {
                max = mid;
            }
        }

        self.tilesize = min;
        if user_size {
            self.preferred_tilesize = self.tilesize;
            self.preferred_tilesize_dpr = device_pixel_ratio;
        }
        self.size_new_drawstate();
        self.dirty = true;
        (self.winwidth, self.winheight)
    }

    // ------------------------------------------------------------------
    // Parameters

    /// Sets the parameters for the next New Game (C's
    /// `midend_set_params`).
    pub fn set_params(&mut self, params: &G::Params) {
        self.params = params.clone();
    }

    /// The parameters for the next New Game.
    #[must_use]
    pub fn params(&self) -> &G::Params {
        &self.params
    }

    // ------------------------------------------------------------------
    // The game

    fn current(&self) -> &G::State {
        &self.states[self.statepos - 1].state
    }

    /// Applies what a game asked for through its context.
    fn apply_ctx(&mut self, mut ctx: GameCtx) {
        if let Some(superseded) = ctx.take_superseded() {
            self.desc = Some(superseded.desc);
            self.privdesc = Some(superseded.private_desc);
            self.id_changed = true;
        }
    }

    fn new_game_state(&mut self, params: &G::Params, desc: &str) -> G::State {
        let mut ctx = GameCtx::new();
        let state = G::new_game(&mut ctx, params, desc);
        self.apply_ctx(ctx);
        state
    }

    fn set_timer(&mut self) {
        self.timing = G::IS_TIMED
            && self.statepos > 0
            && match &mut self.ui {
                Some(ui) => G::timing_state(&self.states[self.statepos - 1].state, ui),
                None => false,
            };
        self.timer_active = self.timing || self.flash_time != 0.0 || self.anim_time != 0.0;
    }

    /// Drops the redo part of the undo chain (C's `midend_purge_states`).
    fn purge_states(&mut self) {
        self.states.truncate(self.statepos);
        self.newgame_redo = None;
    }

    fn take_snapshot(&mut self) -> Snapshot<G> {
        Snapshot {
            params: self.params.clone(),
            curparams: self.curparams.clone(),
            seed: self.seed.take(),
            desc: self.desc.take(),
            privdesc: self.privdesc.take(),
            aux: self.aux.take(),
            states: std::mem::take(&mut self.states),
            statepos: std::mem::take(&mut self.statepos),
            ui: self.ui.take(),
            elapsed: self.elapsed,
        }
    }

    /// Installs a game, as C's `midend_deserialise` does once a save file
    /// has been checked.
    fn install(&mut self, s: Snapshot<G>) {
        self.desc = s.desc;
        self.privdesc = s.privdesc;
        self.seed = s.seed;
        self.aux = s.aux;
        self.genmode = GenMode::Nothing;
        self.states = s.states;
        self.statepos = s.statepos;
        self.newgame_undo = None;
        self.newgame_redo = None;
        self.params = s.params;
        self.curparams = s.curparams;
        self.oldstate = None;
        self.anim_time = 0.0;
        self.anim_pos = 0.0;
        self.flash_time = 0.0;
        self.flash_pos = 0.0;
        self.dir = 0;
        self.ui = s.ui;
        self.elapsed = s.elapsed;
        self.pressed_mouse_button = 0;
        self.set_timer();
        self.drawstate = Some(G::new_drawstate(self.current()));
        self.first_draw = true;
        self.size_new_drawstate();
        self.id_changed = true;
        self.dirty = true;
    }

    /// Starts a new game (C's `midend_new_game`): from the game ID set
    /// last, or else a fresh random seed with the current parameters.
    ///
    /// # Panics
    /// If the game's aux data doesn't solve its own puzzle (C's
    /// self-test).
    pub fn new_game(&mut self) {
        self.newgame_undo = None;
        let keep_for_undo = self.newgame_can_store_undo;
        if keep_for_undo {
            self.purge_states();
        }
        self.stop_anim();
        let snapshot = keep_for_undo.then(|| self.take_snapshot());
        self.newgame_undo = snapshot.map(Box::new);
        self.states.clear();
        self.drawstate = None;

        if self.genmode == GenMode::Desc {
            self.genmode = GenMode::Nothing;
        } else {
            if self.genmode == GenMode::Seed {
                self.genmode = GenMode::Nothing;
            } else {
                // 15 digits, about 48 bits, without a leading zero.
                let mut seed = Vec::with_capacity(15);
                seed.push(b'1' + self.rng.upto(9) as u8);
                for _ in 1..15 {
                    seed.push(b'0' + self.rng.upto(10) as u8);
                }
                self.seed = Some(seed);
                self.curparams = Some(self.params.clone());
            }
            let seed = self.seed.clone().unwrap_or_default();
            let curparams = self.curparams.get_or_insert_with(|| self.params.clone());
            let new = G::new_desc(curparams, &mut RandomState::new(&seed), self.interactive);
            self.desc = Some(new.desc);
            self.privdesc = None;
            self.aux = new.aux;
        }

        // C uses the long-term parameters here on purpose: they must not
        // differ from `curparams` in anything that matters after
        // generation, and a game that forgets to encode such a field
        // then misbehaves visibly.
        let params = self.params.clone();
        let desc = self.desc.clone().unwrap_or_default();
        let state = self.new_game_state(&params, &desc);

        // Self-test of the aux data, as in C.
        if G::CAN_SOLVE
            && let Some(aux) = &self.aux
        {
            let mv = G::solve(&state, &state, Some(aux))
                .unwrap_or_else(|e| panic!("{}: solving with aux failed: {e}", G::NAME));
            assert!(
                G::execute_move(&mut GameCtx::new(), &state, &mv).is_some(),
                "{}: the aux solution doesn't execute",
                G::NAME
            );
        }

        self.drawstate = Some(G::new_drawstate(&state));
        self.states.push(Entry {
            state,
            step: Step::NewGame,
        });
        self.statepos = 1;
        self.first_draw = true;
        self.size_new_drawstate();
        self.elapsed = 0.0;
        self.flash_pos = 0.0;
        self.flash_time = 0.0;
        self.anim_pos = 0.0;
        self.anim_time = 0.0;
        let mut ui = G::new_ui(Some(&self.states[0].state));
        self.apply_prefs(&mut ui);
        self.ui = Some(ui);
        self.set_timer();
        self.pressed_mouse_button = 0;
        self.id_changed = true;
        self.dirty = true;
        self.newgame_can_store_undo = true;
    }

    /// Whether there is anything to undo (C's `midend_can_undo`).
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.statepos > 1 || self.newgame_undo.is_some()
    }

    /// Whether there is anything to redo (C's `midend_can_redo`).
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.statepos < self.states.len() || self.newgame_redo.is_some()
    }

    /// C's check before undoing or redoing a New Game: only if it
    /// wouldn't change the parameters.
    fn same_params(&self, other: &Snapshot<G>) -> bool {
        let enc = |p: &G::Params| G::encode_params(p, true);
        enc(&self.params) == enc(&other.params)
            && self.curparams.as_ref().map(enc) == other.curparams.as_ref().map(enc)
    }

    fn undo(&mut self) -> bool {
        if self.statepos > 1 {
            if let Some(ui) = &mut self.ui {
                let (prev, cur) = (
                    &self.states[self.statepos - 2],
                    &self.states[self.statepos - 1],
                );
                G::changed_state(ui, &cur.state, &prev.state);
            }
            self.statepos -= 1;
            self.dir = -1;
            true
        } else if let Some(snapshot) = self.newgame_undo.take() {
            if !self.same_params(&snapshot) {
                self.newgame_undo = Some(snapshot);
                return false;
            }
            let current = self.take_snapshot();
            self.install(*snapshot);
            self.newgame_redo = Some(Box::new(current));
            true
        } else {
            false
        }
    }

    fn redo(&mut self) -> bool {
        if self.statepos < self.states.len() {
            if let Some(ui) = &mut self.ui {
                let (cur, next) = (&self.states[self.statepos - 1], &self.states[self.statepos]);
                G::changed_state(ui, &cur.state, &next.state);
            }
            self.statepos += 1;
            self.dir = 1;
            true
        } else if let Some(snapshot) = self.newgame_redo.take() {
            if !self.same_params(&snapshot) {
                self.newgame_redo = Some(snapshot);
                return false;
            }
            let current = self.take_snapshot();
            self.install(*snapshot);
            self.newgame_undo = Some(Box::new(current));
            true
        } else {
            false
        }
    }

    /// C's `midend_finish_move`: ends an animation, starting the flash
    /// if the move completed the puzzle.
    fn finish_move(&mut self) {
        let pos = self.statepos;
        // No flash if the later of the two states is special: a forward
        // Solve or a backward Restart.
        let forward = self.dir > 0 && !self.states[pos - 1].step.is_special();
        let backward =
            self.dir < 0 && pos < self.states.len() && !self.states[pos].step.is_special();
        if (self.oldstate.is_some() || pos > 1) && (forward || backward) {
            let (old, dir) = match &self.oldstate {
                Some(old) => (old, self.dir),
                None => (&self.states[pos - 2].state, 1),
            };
            let ui = self.ui.as_mut().expect("a game has a UI");
            let flash = G::flash_length(old, &self.states[pos - 1].state, dir, ui);
            if flash > 0.0 {
                self.flash_pos = 0.0;
                self.flash_time = flash;
            }
        }
        self.oldstate = None;
        self.anim_pos = 0.0;
        self.anim_time = 0.0;
        self.dir = 0;
        self.set_timer();
    }

    /// Finishes any animation at once (C's `midend_stop_anim`).
    pub fn stop_anim(&mut self) {
        if self.oldstate.is_some() || self.anim_time != 0.0 {
            self.finish_move();
            self.dirty = true;
        }
    }

    /// Goes back to the start, as a move that can be undone (C's
    /// `midend_restart_game`).
    ///
    /// Like C, it rebuilds the start from the current parameters and
    /// description. Between changing them ([`set_params`](Self::set_params),
    /// [`game_id`](Self::game_id)) and the next New Game they may not
    /// match, and C then crashes or reads out of bounds; this does
    /// nothing instead.
    pub fn restart_game(&mut self) {
        if self.statepos <= 1 {
            return;
        }
        // From the public description, so Mines restarts after the first
        // click.
        let Some(desc) = self.desc.clone() else {
            return;
        };
        let params = self.params.clone();
        if G::validate_desc(&params, &desc).is_err() {
            return;
        }
        let state = self.new_game_state(&params, &desc);
        self.stop_anim();
        self.purge_states();
        self.states.push(Entry {
            state,
            step: Step::Restart(desc),
        });
        self.statepos = self.states.len();
        if let Some(ui) = &mut self.ui {
            let n = self.statepos;
            G::changed_state(ui, &self.states[n - 2].state, &self.states[n - 1].state);
        }
        self.flash_pos = 0.0;
        self.flash_time = 0.0;
        self.finish_move();
        self.dirty = true;
        self.set_timer();
    }

    /// Feeds an input event to the game (C's `midend_process_key`): mouse
    /// events are made consistent (press, drags, release), keys are
    /// normalised, and undo, redo, new game, solve and quit keys are
    /// handled when the game doesn't use them.
    pub fn process_key(&mut self, x: i32, y: i32, button: Button) -> KeyResult {
        let Some(mut button) = button.try_to_raw() else {
            return KeyResult::Unused;
        };
        if self.statepos == 0 {
            return KeyResult::Unused;
        }
        let mut ret = KeyResult::Unused;
        let mut stylus = button & MOD_STYLUS;
        button &= !MOD_STYLUS;

        // Drags and releases belong to the pressed button; a second
        // press releases the first, unless the first beats it.
        if is_mouse_drag(button) || is_mouse_release(button) {
            if self.pressed_mouse_button == 0 {
                return ret;
            }
            stylus = self.pressed_mouse_button & MOD_STYLUS;
            let offset = if is_mouse_drag(button) {
                LEFT_DRAG - LEFT_BUTTON
            } else {
                LEFT_RELEASE - LEFT_BUTTON
            };
            button = strip(self.pressed_mouse_button) + offset;
        } else if is_mouse_down(button) && self.pressed_mouse_button != 0 {
            let pressed = self.pressed_mouse_button & !MOD_STYLUS;
            let beats = 1_u32 << ((pressed - LEFT_BUTTON) * 3 + button - LEFT_BUTTON);
            if G::FLAGS.bits() & beats != 0 {
                return ret;
            }
            let release = self.pressed_mouse_button + (LEFT_RELEASE - LEFT_BUTTON);
            ret = ret.min(self.really_process_key(x, y, release));
        }

        // Ctrl+letter is the control character; Ctrl+Shift+Z is redo.
        if button & MOD_CTRL != 0 && (0x40..0x80).contains(&strip(button)) {
            button &= 0x1f | (MOD_MASK & !MOD_CTRL);
        }
        if button & (!MOD_MASK | MOD_SHFT) == (MOD_SHFT | 0x1a) {
            button = UI_REDO;
        }
        // Games see Ctrl and Shift only on arrows (and Tab).
        if !is_cursor_move(strip(button)) {
            if button & MOD_CTRL != 0 && strip(button) >= 0x20 {
                return KeyResult::Unused;
            }
            if strip(button) != i32::from(b'\t') {
                button &= !(MOD_CTRL | MOD_SHFT);
            }
        }
        // And the keypad flag only on digits.
        if !(i32::from(b'0')..=i32::from(b'9')).contains(&strip(button)) {
            button &= !MOD_NUM_KEYPAD;
        }
        if button == i32::from(b'\n') || button == i32::from(b'\r') {
            button = CURSOR_SELECT;
        }
        if button == i32::from(b' ') {
            button = CURSOR_SELECT2;
        }
        if button == 0x7f {
            button = 0x08;
        }

        ret = ret.min(self.really_process_key(x, y, button | stylus));

        if is_mouse_release(button) {
            self.pressed_mouse_button = 0;
        } else if is_mouse_down(button) {
            self.pressed_mouse_button = button | stylus;
        }
        ret
    }

    /// C's `midend_really_process_key`.
    fn really_process_key(&mut self, x: i32, y: i32, mut button: i32) -> KeyResult {
        let oldstate = self.current().clone();

        if G::FLAGS.contains(GameFlags::STYLUS_SUPPORT)
            && (is_mouse_down(strip(button))
                || is_mouse_drag(strip(button))
                || is_mouse_release(strip(button)))
        {
            if self.options.stylus_based {
                button |= MOD_STYLUS;
            }
        } else {
            button &= !MOD_STYLUS;
        }

        let result = if is_ui_fake_key(button) {
            MoveResult::Unused
        } else if let Some(b) = Button::from_raw(button) {
            let state = &self.states[self.statepos - 1].state;
            let ui = self.ui.as_mut().expect("a game has a UI");
            let ds = self.drawstate.as_ref().expect("a game has a draw state");
            G::interpret_move(state, ui, ds, x, y, b)
        } else {
            MoveResult::Unused
        };

        let mut undone_step = None;
        match result {
            MoveResult::Unused => {
                let key = |c: u8| button == i32::from(c);
                let one_key =
                    |c: u8| self.one_key_shortcuts && (key(c) || key(c.to_ascii_uppercase()));
                if one_key(b'n') || key(0x0e) || button == UI_NEWGAME {
                    self.new_game();
                    self.dirty = true;
                    return KeyResult::SomeEffect;
                } else if one_key(b'u') || key(b'*') || key(0x1a) || key(0x1f) || button == UI_UNDO
                {
                    self.stop_anim();
                    undone_step = Some(self.states[self.statepos - 1].step.clone());
                    if !self.undo() {
                        return KeyResult::NoEffect;
                    }
                } else if one_key(b'r') || key(b'#') || key(0x12) || key(0x19) || button == UI_REDO
                {
                    self.stop_anim();
                    if !self.redo() {
                        return KeyResult::NoEffect;
                    }
                } else if (key(0x13) || button == UI_SOLVE) && G::CAN_SOLVE {
                    if self.solve().is_err() {
                        return KeyResult::SomeEffect;
                    }
                } else if one_key(b'q') || key(0x11) || button == UI_QUIT {
                    return KeyResult::Quit;
                } else {
                    return KeyResult::Unused;
                }
            }
            MoveResult::NoEffect => return KeyResult::NoEffect,
            MoveResult::UiUpdate => {
                self.dirty = true;
                self.set_timer();
                return KeyResult::SomeEffect;
            }
            MoveResult::Move(mv) => {
                let mut ctx = GameCtx::new();
                let state = G::execute_move(&mut ctx, self.current(), &mv)
                    .unwrap_or_else(|| panic!("{}: a move from interpret_move failed", G::NAME));
                self.apply_ctx(ctx);
                self.stop_anim();
                self.purge_states();
                self.states.push(Entry {
                    state,
                    step: Step::Move(mv),
                });
                self.statepos = self.states.len();
                self.dir = 1;
                if let Some(ui) = &mut self.ui {
                    let n = self.statepos;
                    G::changed_state(ui, &self.states[n - 2].state, &self.states[n - 1].state);
                }
            }
        }

        let step = undone_step.unwrap_or_else(|| self.states[self.statepos - 1].step.clone());
        let animates = !step.is_special()
            || (matches!(step, Step::Solve(_)) && G::FLAGS.contains(GameFlags::SOLVE_ANIMATES));
        let anim_time = if animates {
            let ui = self.ui.as_mut().expect("a game has a UI");
            G::anim_length(
                &oldstate,
                &self.states[self.statepos - 1].state,
                self.dir,
                ui,
            )
        } else {
            0.0
        };

        self.oldstate = Some(oldstate);
        if anim_time > 0.0 {
            self.anim_time = anim_time;
        } else {
            self.anim_time = 0.0;
            self.finish_move();
        }
        self.anim_pos = 0.0;
        self.dirty = true;
        self.set_timer();
        KeyResult::SomeEffect
    }

    /// Advances animations, the flash and the game timer by `tplus`
    /// seconds (C's `midend_timer`).
    pub fn timer(&mut self, tplus: f32) {
        let need_redraw = self.anim_time > 0.0 || self.flash_time > 0.0;

        self.anim_pos += tplus;
        if (self.anim_pos >= self.anim_time || self.anim_time == 0.0 || self.oldstate.is_none())
            && self.anim_time > 0.0
        {
            self.finish_move();
        }

        self.flash_pos += tplus;
        if self.flash_pos >= self.flash_time || self.flash_time == 0.0 {
            self.flash_pos = 0.0;
            self.flash_time = 0.0;
        }

        if need_redraw {
            self.dirty = true;
        }
        if self.timing {
            self.elapsed += tplus;
        }
        if self.statepos > 0 {
            self.set_timer();
        }
    }

    /// Draws the puzzle (C's `midend_redraw`); the first draw after a new
    /// draw state fills the whole area with colour 0 first.
    ///
    /// # Panics
    /// Only if the mid-end's invariants fail (a game without a UI).
    pub fn redraw(&mut self, api: &mut dyn DrawingApi) {
        self.dirty = false;
        if self.statepos == 0 {
            return;
        }
        let Some(ds) = &mut self.drawstate else {
            return;
        };
        let first_draw = std::mem::replace(&mut self.first_draw, false);
        let mut dr = Drawing::new(api);
        dr.api().start_draw();
        if first_draw {
            dr.rect(0, 0, self.winwidth, self.winheight, 0_usize);
        }
        let ui = self.ui.as_ref().expect("a game has a UI");
        let state = &self.states[self.statepos - 1].state;
        match &self.oldstate {
            Some(old) if self.anim_time > 0.0 && self.anim_pos < self.anim_time => {
                G::redraw(
                    &mut dr,
                    ds,
                    Some(old),
                    state,
                    self.dir,
                    ui,
                    self.anim_pos,
                    self.flash_pos,
                );
            }
            _ => G::redraw(&mut dr, ds, None, state, 1, ui, 0.0, self.flash_pos),
        }
        if first_draw {
            dr.update(0, 0, self.winwidth, self.winheight);
        }
        dr.api().end_draw();
        if let Some(status) = dr.take_status() {
            self.last_status = Some(status);
        }
    }

    /// Throws away the draw state so the next redraw draws everything
    /// (C's `midend_force_redraw`).
    pub fn force_redraw(&mut self) {
        if self.statepos == 0 {
            return;
        }
        self.drawstate = Some(G::new_drawstate(&self.states[0].state));
        self.first_draw = true;
        self.size_new_drawstate();
        self.dirty = true;
    }

    /// The status bar text, with the timer in front for timed games (C's
    /// `midend_rewrite_statusbar`).
    #[must_use]
    pub fn status_text(&self) -> String {
        let text = self.last_status.as_deref().unwrap_or("");
        if G::IS_TIMED {
            let sec = self.elapsed as i32;
            format!("[{}:{:02}] {text}", sec / 60, sec % 60)
        } else {
            text.to_owned()
        }
    }

    /// Solves the puzzle, as a move that can be undone (C's
    /// `midend_solve`).
    ///
    /// # Errors
    /// C's message: the game can't solve, or this position has no
    /// solution.
    ///
    /// # Panics
    /// If the solver's move doesn't execute (a bug in the game).
    pub fn solve(&mut self) -> Result<(), GameError> {
        if !G::CAN_SOLVE {
            return Err(GameError::new(
                "This game does not support the Solve operation",
            ));
        }
        if self.statepos < 1 {
            return Err(GameError::new("No game set up to solve"));
        }
        let mv = G::solve(&self.states[0].state, self.current(), self.aux.as_deref())?;
        let mut ctx = GameCtx::new();
        let state = G::execute_move(&mut ctx, self.current(), &mv)
            .unwrap_or_else(|| panic!("{}: the solver's move failed", G::NAME));
        self.apply_ctx(ctx);

        self.stop_anim();
        self.purge_states();
        self.states.push(Entry {
            state,
            step: Step::Solve(mv),
        });
        self.statepos = self.states.len();
        let n = self.statepos;
        if let Some(ui) = &mut self.ui {
            G::changed_state(ui, &self.states[n - 2].state, &self.states[n - 1].state);
        }
        self.dir = 1;
        if G::FLAGS.contains(GameFlags::SOLVE_ANIMATES) {
            self.oldstate = Some(self.states[n - 2].state.clone());
            let ui = self.ui.as_mut().expect("a game has a UI");
            self.anim_time =
                G::anim_length(&self.states[n - 2].state, &self.states[n - 1].state, 1, ui);
            self.anim_pos = 0.0;
        } else {
            self.anim_time = 0.0;
            self.finish_move();
        }
        self.dirty = true;
        self.set_timer();
        Ok(())
    }

    /// The text of the step that led to the current position, as in save
    /// files (C's `movestr`): the move, the solver's move, or the
    /// description a restart went back to; `None` at the start.
    #[doc(alias = "movestr")]
    #[must_use]
    pub fn current_move(&self) -> Option<String> {
        let entry = self.states.get(self.statepos.checked_sub(1)?)?;
        let mut text = String::new();
        match &entry.step {
            Step::NewGame => return None,
            Step::Move(mv) | Step::Solve(mv) => G::encode_move(mv, &mut text),
            Step::Restart(desc) => text.clone_from(desc),
        }
        Some(text)
    }

    /// Whether the game is over (C's `midend_status`); a mid-end without
    /// a game counts as solved.
    #[must_use]
    pub fn status(&self) -> GameStatus {
        if self.statepos == 0 {
            GameStatus::Won
        } else {
            G::status(self.current())
        }
    }

    /// Whether [`text_format`](Self::text_format) works now (C's
    /// `midend_can_format_as_text_now`).
    #[must_use]
    pub fn can_format_as_text_now(&self) -> bool {
        G::CAN_FORMAT_AS_TEXT_EVER && G::can_format_as_text_now(&self.params)
    }

    /// The current position as text (C's `midend_text_format`).
    #[must_use]
    pub fn text_format(&self) -> Option<String> {
        if self.statepos > 0 && self.can_format_as_text_now() {
            G::text_format(self.current())
        } else {
            None
        }
    }

    /// The keys a keyboardless front end should offer, labelled (C's
    /// `midend_request_keys`).
    #[must_use]
    pub fn request_keys(&self) -> Vec<KeyLabel> {
        let mut keys = G::request_keys(&self.params);
        for key in &mut keys {
            if key.label.is_none() {
                key.label = key.button.label().map(Into::into);
            }
        }
        keys
    }

    /// What a select key would do now (C's `midend_current_key_label`).
    #[must_use]
    pub fn current_key_label(&self, button: Button) -> Option<&'static str> {
        let ui = self.ui.as_ref()?;
        let state = &self.states.get(self.statepos.checked_sub(1)?)?.state;
        G::current_key_label(ui, state, button)
    }

    /// Where the keyboard cursor is (C's `midend_get_cursor_location`).
    #[must_use]
    pub fn cursor_location(&self) -> Option<Rect> {
        let ui = self.ui.as_ref()?;
        let ds = self.drawstate.as_ref()?;
        G::cursor_location(ui, ds, self.current(), &self.params)
    }

    // ------------------------------------------------------------------
    // Game IDs

    /// Sets the game for the next New Game from a game ID (C's
    /// `midend_game_id`): `params`, `params:desc` or `params#seed`.
    ///
    /// # Errors
    /// The game's message for invalid parameters or description; the
    /// mid-end is then unchanged.
    pub fn game_id(&mut self, id: &str) -> Result<(), GameError> {
        self.game_id_int(id, DefMode::Params)
    }

    fn game_id_int(&mut self, id: &str, defmode: DefMode) -> Result<(), GameError> {
        let seed_at = id.find('#');
        let desc_at = id.find(':');
        let (par, desc, seed) = match (desc_at, seed_at) {
            (Some(d), s) if s.is_none_or(|s| d < s) => (Some(&id[..d]), Some(&id[d + 1..]), None),
            (_, Some(s)) => (Some(&id[..s]), None, Some(&id[s + 1..])),
            _ => match defmode {
                DefMode::Seed => (None, None, Some(id)),
                DefMode::Desc => (None, Some(id), None),
                DefMode::Params => (Some(id), None, None),
            },
        };

        let (newcurparams, newparams) = if let Some(par) = par {
            // A descriptive ID may underspecify on purpose (difficulty
            // isn't needed to play), so it builds on the current
            // parameters; a seed or bare parameters build on the real
            // defaults, so they mean the same everywhere.
            let mut cur = if desc.is_some() {
                self.params.clone()
            } else {
                G::default_params()
            };
            G::decode_params(&mut cur, par);
            G::validate_params(&cur, desc.is_none())?;
            // Only the parts that persist go into the long-term
            // parameters, unless only parameters were given.
            let newparams = if seed.is_some() || desc.is_some() {
                let mut p = self.params.clone();
                G::decode_params(&mut p, &G::encode_params(&cur, false));
                p
            } else {
                cur.clone()
            };
            (Some(cur), newparams)
        } else {
            (self.curparams.clone(), self.params.clone())
        };

        if let Some(desc) = desc {
            G::validate_desc(&newparams, desc)?;
        }

        self.params = newparams;
        self.curparams = newcurparams;
        self.desc = None;
        self.privdesc = None;
        self.seed = None;
        if let Some(desc) = desc {
            self.desc = Some(desc.to_owned());
            self.genmode = GenMode::Desc;
            self.aux = None;
        }
        if let Some(seed) = seed {
            self.seed = Some(seed.as_bytes().to_vec());
            self.genmode = GenMode::Seed;
        }
        self.newgame_can_store_undo = false;
        Ok(())
    }

    /// The current game's descriptive ID, `params:desc` (C's
    /// `midend_get_game_id`); `None` before the first game.
    #[must_use]
    pub fn get_game_id(&self) -> Option<String> {
        let params = G::encode_params(self.curparams.as_ref()?, false);
        Some(format!("{params}:{}", self.desc.as_deref()?))
    }

    /// The current game's random seed ID, `params#seed` (C's
    /// `midend_get_random_seed`); `None` if it wasn't generated from a
    /// seed.
    #[must_use]
    pub fn get_random_seed(&self) -> Option<String> {
        let seed = self.seed.as_ref()?;
        let params = G::encode_params(self.curparams.as_ref()?, true);
        Some(format!("{params}#{}", String::from_utf8_lossy(seed)))
    }

    // ------------------------------------------------------------------
    // Configuration

    /// A configuration dialog (C's `midend_get_config`); `None` if there
    /// is nothing to configure (no custom parameters, or no game yet for
    /// a game ID).
    #[must_use]
    pub fn get_config(&self, kind: ConfigKind) -> Option<Config> {
        let name = G::NAME;
        match kind {
            ConfigKind::Settings => Some(Config {
                title: format!("{name} configuration"),
                items: G::configure(&self.params)?,
            }),
            ConfigKind::Seed | ConfigKind::Desc => {
                let curparams = self.curparams.as_ref()?;
                let seed = kind == ConfigKind::Seed;
                let params = G::encode_params(curparams, seed);
                let value = if seed {
                    let s = self.seed.as_deref().unwrap_or_default();
                    format!("{params}#{}", String::from_utf8_lossy(s))
                } else {
                    format!("{params}:{}", self.desc.as_deref().unwrap_or(""))
                };
                Some(Config {
                    title: format!("{name} {} selection", if seed { "random" } else { "game" }),
                    items: vec![ConfigItem::string(
                        if seed { "Game random seed" } else { "Game ID" },
                        value,
                    )],
                })
            }
            ConfigKind::Prefs => Some(Config {
                title: format!("{name} preferences"),
                items: self.get_prefs(None),
            }),
        }
    }

    /// Applies a filled-in configuration dialog (C's
    /// `midend_set_config`).
    ///
    /// # Errors
    /// The game's message for invalid parameters or game ID.
    pub fn set_config(&mut self, kind: ConfigKind, items: &[ConfigItem]) -> Result<(), GameError> {
        match kind {
            ConfigKind::Settings => {
                let params = G::custom_params(items)?;
                G::validate_params(&params, true)?;
                self.params = params;
            }
            ConfigKind::Seed | ConfigKind::Desc => {
                let id = items.first().and_then(ConfigItem::as_str).unwrap_or("");
                let mode = if kind == ConfigKind::Seed {
                    DefMode::Seed
                } else {
                    DefMode::Desc
                };
                self.game_id_int(id, mode)?;
            }
            ConfigKind::Prefs => {
                let mut ui = self.ui.take();
                self.set_prefs(ui.as_mut(), items);
                self.ui = ui;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Presets

    /// The preset menu, each preset carrying its id (C's
    /// `midend_get_presets`): the game's presets plus any from the
    /// options.
    ///
    /// # Panics
    /// Never: the menu is built on first use.
    pub fn presets(&mut self) -> &PresetMenu<usize> {
        if self.presets.is_none() {
            let mut menu = G::presets();
            if let Some(extra) = self.options.extra_presets.clone() {
                options::add_presets_from_text::<G>(&mut menu, &extra);
            }
            self.presets = Some(number_presets::<G>(menu));
        }
        &self.presets.as_ref().expect("built above").menu
    }

    /// The parameters of a preset, by id.
    #[must_use]
    pub fn preset_params(&self, id: usize) -> Option<&G::Params> {
        let presets = self.presets.as_ref()?;
        presets.params.get(id)?.as_ref().map(|(p, _)| p)
    }

    /// The preset matching the current parameters (C's
    /// `midend_which_preset`); `None` if none does, or before
    /// [`presets`](Self::presets) was first called.
    #[must_use]
    pub fn which_preset(&self) -> Option<usize> {
        let encoding = G::encode_params(&self.params, true);
        self.presets
            .as_ref()?
            .params
            .iter()
            .position(|p| p.as_ref().is_some_and(|(_, e)| *e == encoding))
    }
}

/// How a lone string in a game ID is read (C's `DEF_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefMode {
    Params,
    Seed,
    Desc,
}

/// Gives every menu entry C's id (each menu's entries first, then its
/// submenus', recursively) and replaces the parameters by ids.
fn number_presets<G: Game>(menu: PresetMenu<G::Params>) -> Presets<G::Params> {
    fn count<P>(menu: &PresetMenu<P>) -> usize {
        menu.entries
            .iter()
            .map(|e| match e {
                PresetEntry::Preset { .. } => 1,
                PresetEntry::Submenu { menu, .. } => 1 + count(menu),
            })
            .sum()
    }

    fn number<G: Game>(
        menu: PresetMenu<G::Params>,
        next: &mut usize,
        params: &mut [Option<(G::Params, String)>],
    ) -> PresetMenu<usize> {
        let first = *next;
        *next += menu.entries.len();
        let mut out = PresetMenu::new();
        for (i, entry) in menu.entries.into_iter().enumerate() {
            let id = first + i;
            match entry {
                PresetEntry::Preset { title, params: p } => {
                    let encoded = G::encode_params(&p, true);
                    params[id] = Some((p, encoded));
                    out.entries.push(PresetEntry::Preset { title, params: id });
                }
                PresetEntry::Submenu { title, menu } => {
                    out.entries.push(PresetEntry::Submenu {
                        title,
                        menu: number::<G>(menu, next, params),
                    });
                }
            }
        }
        out
    }

    let mut params = Vec::new();
    params.resize_with(count(&menu), || None);
    let mut next = 0;
    let menu = number::<G>(menu, &mut next, &mut params);
    Presets { menu, params }
}

impl<G: Game> Midend<G> {
    // ------------------------------------------------------------------
    // Preferences

    /// The mid-end's own preference, then the game's, from `ui`, the
    /// current UI or a preferences-only one (C's `midend_get_prefs`).
    fn get_prefs(&self, ui: Option<&G::Ui>) -> Vec<ConfigItem> {
        let mut items = vec![ConfigItem {
            name: "Keyboard shortcuts without Ctrl",
            keyword: Some("one-key-shortcuts"),
            value: ConfigValue::Bool(self.one_key_shortcuts),
        }];
        match ui.or(self.ui.as_ref()) {
            Some(ui) => items.extend(G::prefs(ui)),
            None => items.extend(G::prefs(&G::new_ui(None))),
        }
        items
    }

    /// C's `midend_set_prefs`: applies `items` (from
    /// [`get_prefs`](Self::get_prefs)) to `ui`, or to a preferences-only
    /// UI, and remembers the result for new UIs.
    fn set_prefs(&mut self, ui: Option<&mut G::Ui>, items: &[ConfigItem]) {
        if let Some(b) = items.first().and_then(ConfigItem::as_bool) {
            self.one_key_shortcuts = b;
        }
        let game_items = items.get(1..).unwrap_or(&[]);
        let saved = if let Some(ui) = ui {
            G::set_prefs(ui, game_items);
            self.get_prefs(Some(ui))
        } else {
            let mut tmp = G::new_ui(None);
            G::set_prefs(&mut tmp, game_items);
            self.get_prefs(Some(&tmp))
        };
        self.saved_prefs = saved;
    }

    /// Gives a new UI the saved preferences (C's `midend_apply_prefs`).
    fn apply_prefs(&mut self, ui: &mut G::Ui) {
        let mut items = self.get_prefs(Some(ui));
        for item in &mut items {
            if let Some(saved) = self.saved_prefs.iter().rfind(|s| s.keyword == item.keyword) {
                item.value = saved.value.clone();
            }
        }
        self.set_prefs(Some(ui), &items);
    }
}
