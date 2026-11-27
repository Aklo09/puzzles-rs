//! The [`Game`] trait every puzzle implements (C's `struct game`,
//! `puzzles.h`, upstream 616da16), and the types games and the mid-end
//! exchange.
//!
//! C's table of function pointers becomes a trait of associated
//! functions on a marker type (`struct Flip;`), so the mid-end is generic
//! over the game and each game is monomorphic (`docs/porting-plan.md`
//! §3.1). C's `game_params`, `game_state`, `game_ui` and
//! `game_drawstate` are associated types; `dup_*`/`free_*` are
//! `Clone`/`Drop`. Moves are typed (D4): the text form exists only in
//! save files, through [`Game::encode_move`] and [`Game::decode_move`].

use std::borrow::Cow;
use std::fmt;

use bitflags::bitflags;

use crate::button::{Button, MouseButton};
use crate::config::ConfigItem;
use crate::drawing::Drawing;
use crate::palette::ColourSpec;
use crate::preset::PresetMenu;
use crate::random::RandomState;

/// A puzzle: everything the mid-end needs to generate, play, save and
/// draw it (C's `struct game`).
///
/// Optional capabilities have defaults that report "unsupported", and a
/// `CAN_*` const each, so front ends can grey out menu entries without
/// calling anything. Each const must agree with its method; the
/// conformance tests in `puzzles-games` check that for every game.
///
/// Untrusted text (game IDs, descriptions, moves, save files) reaches
/// [`decode_params`](Self::decode_params),
/// [`validate_params`](Self::validate_params),
/// [`validate_desc`](Self::validate_desc) and
/// [`decode_move`](Self::decode_move); none of them may panic.
/// [`new_game`](Self::new_game) runs only on a description that passed
/// `validate_desc`, as in C.
pub trait Game: 'static {
    /// The parameters (C's `game_params`). `Send`, because front ends
    /// generate puzzles on a worker thread (threading decision).
    type Params: Clone + PartialEq + fmt::Debug + Send + 'static;
    /// One position of a game (C's `game_state`). Parts shared by every
    /// state of a game, which C reference-counts, sit behind an `Rc`.
    type State: Clone;
    /// UI state that isn't part of the position: cursor, selection,
    /// drag (C's `game_ui`).
    type Ui;
    /// What the game remembers between redraws (C's `game_drawstate`).
    type DrawState;
    /// A move (D4): what [`interpret_move`](Self::interpret_move) and
    /// [`solve`](Self::solve) produce, and
    /// [`execute_move`](Self::execute_move) applies.
    type Move: Clone + PartialEq + fmt::Debug;

    /// The puzzle's name, as shown to the user (C's `name`).
    const NAME: &'static str;
    /// Tile size in pixels at 100% scale (C's `preferred_tilesize`).
    const PREFERRED_TILESIZE: i32;
    /// The colours, as semantic specs (D7; replaces C's `colours()`).
    /// Drawing calls use indices into it.
    const PALETTE: &'static [ColourSpec];
    /// C's `flags` word.
    const FLAGS: GameFlags = GameFlags::empty();

    /// Has a "Custom" parameters dialog: [`configure`](Self::configure)
    /// and [`custom_params`](Self::custom_params) (C's `can_configure`).
    const CAN_CONFIGURE: bool = false;
    /// Has [`solve`](Self::solve) (C's `can_solve`).
    const CAN_SOLVE: bool = false;
    /// Has [`text_format`](Self::text_format), at least for some
    /// parameters (C's `can_format_as_text_ever`).
    const CAN_FORMAT_AS_TEXT_EVER: bool = false;
    /// Shows a status bar (C's `wants_statusbar`).
    const WANTS_STATUSBAR: bool = false;
    /// Shows a timer, driven by [`timing_state`](Self::timing_state)
    /// (C's `is_timed`).
    const IS_TIMED: bool = false;
    // Printing (C's can_print, print_size, print) comes with printing in
    // Phase 9.

    // ------------------------------------------------------------------
    // Parameters

    /// The parameters a new game starts with.
    fn default_params() -> Self::Params;

    /// The preset menu (C's `preset_menu`, or `fetch_preset` called
    /// until it returns false).
    #[doc(alias("fetch_preset", "preset_menu"))]
    #[must_use]
    fn presets() -> PresetMenu<Self::Params> {
        PresetMenu::new()
    }

    /// Reads parameters from text into `params`, which holds the
    /// current or default parameters; fields the text doesn't mention
    /// keep their values. Never fails: nonsense gives parameters that
    /// [`validate_params`](Self::validate_params) rejects.
    fn decode_params(params: &mut Self::Params, s: &str);

    /// Writes parameters as text. `full` includes the fields that only
    /// affect generation (difficulty, ...), which a game ID with a
    /// description doesn't need.
    fn encode_params(params: &Self::Params, full: bool) -> String;

    /// The fields of the "Custom" dialog, filled from `params` (C's
    /// `configure`); `None` if [`CAN_CONFIGURE`](Self::CAN_CONFIGURE) is
    /// false.
    fn configure(params: &Self::Params) -> Option<Vec<ConfigItem>> {
        let _ = params;
        None
    }

    /// Parameters from a filled-in "Custom" dialog (C's
    /// `custom_params`). The result still goes through
    /// [`validate_params`](Self::validate_params).
    ///
    /// # Errors
    /// [`GameError::unsupported`] by default; a game may also reject
    /// fields of the wrong kind.
    fn custom_params(config: &[ConfigItem]) -> Result<Self::Params, GameError> {
        let _ = config;
        Err(GameError::unsupported())
    }

    /// Checks parameters; `full` as in
    /// [`encode_params`](Self::encode_params).
    ///
    /// # Errors
    /// C's message for invalid parameters.
    fn validate_params(params: &Self::Params, full: bool) -> Result<(), GameError>;

    // ------------------------------------------------------------------
    // Generation and states

    /// Generates a puzzle description (C's `new_desc`). `interactive` is
    /// true when a player will see the puzzle (C's `drawing != NULL` in
    /// the mid-end), false for batch generation.
    fn new_desc(params: &Self::Params, rng: &mut RandomState, interactive: bool) -> NewDesc;

    /// Checks a description against valid parameters.
    ///
    /// # Errors
    /// C's message for an invalid description.
    fn validate_desc(params: &Self::Params, desc: &str) -> Result<(), GameError>;

    /// The starting position of a puzzle; `desc` has passed
    /// [`validate_desc`](Self::validate_desc).
    fn new_game(ctx: &mut GameCtx, params: &Self::Params, desc: &str) -> Self::State;

    /// Tells a state the public description of its game after loading a
    /// save file with a private one (C's `set_public_desc`; only Mines).
    fn set_public_desc(state: &mut Self::State, public_desc: &str) {
        let _ = (state, public_desc);
    }

    /// Solves from the current position, given the original one and the
    /// generator's `aux` hint if there is one (C's `solve`).
    ///
    /// # Errors
    /// [`GameError::unsupported`] by default; otherwise C's message,
    /// such as "No solution exists for this position".
    fn solve(
        original: &Self::State,
        current: &Self::State,
        aux: Option<&str>,
    ) -> Result<Self::Move, GameError> {
        let _ = (original, current, aux);
        Err(GameError::unsupported())
    }

    /// Whether [`text_format`](Self::text_format) works for these
    /// parameters (C's `can_format_as_text_now`).
    fn can_format_as_text_now(params: &Self::Params) -> bool {
        let _ = params;
        Self::CAN_FORMAT_AS_TEXT_EVER
    }

    /// The position as plain text, for copying (C's `text_format`).
    fn text_format(state: &Self::State) -> Option<String> {
        let _ = state;
        None
    }

    /// Whether the game is over (C's `status`).
    fn status(state: &Self::State) -> GameStatus;

    /// Whether the timer runs in this position (C's `timing_state`);
    /// only asked if [`IS_TIMED`](Self::IS_TIMED).
    fn timing_state(state: &Self::State, ui: &mut Self::Ui) -> bool {
        let _ = (state, ui);
        true
    }

    // ------------------------------------------------------------------
    // UI and moves

    /// A fresh UI for a game starting at `state`.
    ///
    /// `state` is `None` for a UI that only holds preferences (C's
    /// `new_ui(NULL)`): the mid-end reads and writes the preferences file
    /// through one before or without a game.
    fn new_ui(state: Option<&Self::State>) -> Self::Ui;

    /// The user preferences this game has, filled from `ui` (C's
    /// `get_prefs`).
    #[doc(alias = "get_prefs")]
    fn prefs(ui: &Self::Ui) -> Vec<ConfigItem> {
        let _ = ui;
        Vec::new()
    }

    /// Applies edited preferences, as [`prefs`](Self::prefs) listed
    /// them (C's `set_prefs`).
    fn set_prefs(ui: &mut Self::Ui, config: &[ConfigItem]) {
        let _ = (ui, config);
    }

    /// UI state worth keeping in a save file (C's `encode_ui`).
    fn encode_ui(ui: &Self::Ui) -> Option<String> {
        let _ = ui;
        None
    }

    /// Restores what [`encode_ui`](Self::encode_ui) saved; the text is
    /// untrusted (C's `decode_ui`).
    fn decode_ui(ui: &mut Self::Ui, encoding: &str, state: &Self::State) {
        let _ = (ui, encoding, state);
    }

    /// Keys a keyboardless front end should offer as buttons (C's
    /// `request_keys`).
    fn request_keys(params: &Self::Params) -> Vec<KeyLabel> {
        let _ = params;
        Vec::new()
    }

    /// Lets the UI react to a move, undo or redo from `old` to `new`
    /// (C's `changed_state`).
    fn changed_state(ui: &mut Self::Ui, old: &Self::State, new: &Self::State) {
        let _ = (ui, old, new);
    }

    /// What `button` would do now, for a front end's on-screen key (C's
    /// `current_key_label`, where "" is `None`).
    fn current_key_label(
        ui: &Self::Ui,
        state: &Self::State,
        button: Button,
    ) -> Option<&'static str> {
        let _ = (ui, state, button);
        None
    }

    /// Turns an input event at pixel `(x, y)` into a move, a UI change
    /// or nothing (C's `interpret_move`). `ds` gives the layout (tile
    /// size, offsets).
    fn interpret_move(
        state: &Self::State,
        ui: &mut Self::Ui,
        ds: &Self::DrawState,
        x: i32,
        y: i32,
        button: Button,
    ) -> MoveResult<Self::Move>;

    /// Applies a move: the new position, or `None` if the move is
    /// illegal here (C's `execute_move`). A move from
    /// [`interpret_move`](Self::interpret_move) or
    /// [`solve`](Self::solve) on the same state must succeed.
    fn execute_move(ctx: &mut GameCtx, state: &Self::State, mv: &Self::Move)
    -> Option<Self::State>;

    /// Writes a move as C's move string (D4). Every move C can produce
    /// must give C's exact bytes.
    fn encode_move(mv: &Self::Move, out: &mut String);

    /// Reads a move string: syntax and range checks only, legality is
    /// [`execute_move`](Self::execute_move)'s. Accepts at least every
    /// string C's games write. The text is untrusted.
    ///
    /// # Errors
    /// [`MoveParseError`] for text C's `execute_move` can't parse.
    fn decode_move(state: &Self::State, text: &str) -> Result<Self::Move, MoveParseError>;

    // ------------------------------------------------------------------
    // Drawing

    /// Size in pixels of the puzzle area for a tile size (C's
    /// `compute_size`).
    fn compute_size(params: &Self::Params, tilesize: i32, ui: &Self::Ui) -> (i32, i32);

    /// A draw state for a game starting at `state`, before any size is
    /// set (C's `new_drawstate`; built without a drawing, see the
    /// mid-end drawing decision).
    fn new_drawstate(state: &Self::State) -> Self::DrawState;

    /// Sets the tile size, and whatever layout follows from it (C's
    /// `set_size`). Blitters are created here, as plain handles.
    fn set_size(ds: &mut Self::DrawState, params: &Self::Params, tilesize: i32);

    /// Draws `new`, animating from `old` when it is given (C's
    /// `redraw`). `dir` is +1 after a move or redo and -1 after an undo;
    /// `anim_time` and `flash_time` are the seconds elapsed in the
    /// current animation and completion flash (0 if none).
    #[allow(clippy::too_many_arguments, reason = "mirrors C's redraw")]
    fn redraw(
        dr: &mut Drawing<'_>,
        ds: &mut Self::DrawState,
        old: Option<&Self::State>,
        new: &Self::State,
        dir: i32,
        ui: &Self::Ui,
        anim_time: f32,
        flash_time: f32,
    );

    /// Seconds to animate the change from `old` to `new` (C's
    /// `anim_length`); 0 for none.
    fn anim_length(old: &Self::State, new: &Self::State, dir: i32, ui: &mut Self::Ui) -> f32 {
        let _ = (old, new, dir, ui);
        0.0
    }

    /// Seconds of completion flash after the change from `old` to
    /// `new` (C's `flash_length`); 0 for none.
    fn flash_length(old: &Self::State, new: &Self::State, dir: i32, ui: &mut Self::Ui) -> f32 {
        let _ = (old, new, dir, ui);
        0.0
    }

    /// Where the keyboard cursor is, for screen readers and on-screen
    /// keyboards (C's `get_cursor_location`); `None` when it is hidden.
    fn cursor_location(
        ui: &Self::Ui,
        ds: &Self::DrawState,
        state: &Self::State,
        params: &Self::Params,
    ) -> Option<Rect> {
        let _ = (ui, ds, state, params);
        None
    }
}

bitflags! {
    /// C's `flags` word of a game, with C's bit values.
    ///
    /// The low nine bits are mouse button priorities
    /// ([`button_beats`](Self::button_beats)).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct GameFlags: u32 {
        /// Solving is animated like a move (`SOLVE_ANIMATES`).
        const SOLVE_ANIMATES = 1 << 9;
        /// Needs a right mouse button, to be emulated on touch screens
        /// (`REQUIRE_RBUTTON`).
        const REQUIRE_RBUTTON = 1 << 10;
        /// Needs digit input (`REQUIRE_NUMPAD`).
        const REQUIRE_NUMPAD = 1 << 11;
        /// Treats stylus and mouse input differently (`STYLUS_SUPPORT`).
        const STYLUS_SUPPORT = 1 << 12;
        // The mouse button priority bits (BUTTON_BEATS).
        const _ = 0x1ff;
    }
}

impl GameFlags {
    /// The flag saying that pressing `y` while `x` is held keeps `x`'s
    /// drag going (C's `BUTTON_BEATS(x, y)`).
    #[doc(alias = "BUTTON_BEATS")]
    #[must_use]
    pub const fn button_beats(x: MouseButton, y: MouseButton) -> Self {
        Self::from_bits_retain(1 << (mouse_index(x) * 3 + mouse_index(y)))
    }
}

const fn mouse_index(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// A generated puzzle (C's `new_desc` result and `aux` out-parameter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewDesc {
    /// The description, the part of a game ID after the `:`.
    pub desc: String,
    /// Private data for [`Game::solve`] (often the solution), kept by
    /// the mid-end and in save files, never shown.
    pub aux: Option<String>,
}

impl NewDesc {
    /// A description without aux data.
    #[must_use]
    pub fn new(desc: impl Into<String>) -> Self {
        Self {
            desc: desc.into(),
            aux: None,
        }
    }
}

/// The mid-end services a game can ask for while it creates or changes
/// a state, in place of C's `midend *` argument: requests are collected
/// here and the mid-end applies them after the call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameCtx {
    superseded: Option<SupersededDesc>,
}

/// A replacement description (C's `midend_supersede_game_desc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupersededDesc {
    /// The new public description.
    pub desc: String,
    /// The private one save files keep.
    pub private_desc: String,
}

impl GameCtx {
    /// A context with no requests.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the game's description (C's
    /// `midend_supersede_game_desc`): Mines, which places its mines on
    /// the first click.
    #[doc(alias = "midend_supersede_game_desc")]
    pub fn supersede_desc(&mut self, desc: impl Into<String>, private_desc: impl Into<String>) {
        self.superseded = Some(SupersededDesc {
            desc: desc.into(),
            private_desc: private_desc.into(),
        });
    }

    /// The replacement description requested since the last call, if
    /// any.
    pub fn take_superseded(&mut self) -> Option<SupersededDesc> {
        self.superseded.take()
    }
}

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

/// What `interpret_move` made of an input event (C's move string, or one
/// of `MOVE_UNUSED`, `MOVE_NO_EFFECT`, `MOVE_UI_UPDATE`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveResult<M> {
    /// The game has no use for this input (C's `MOVE_UNUSED`); the front
    /// end may handle it.
    #[doc(alias = "MOVE_UNUSED")]
    Unused,
    /// Understood, but it changed nothing, not even the UI (C's
    /// `MOVE_NO_EFFECT`).
    #[doc(alias = "MOVE_NO_EFFECT")]
    NoEffect,
    /// Only the UI changed (cursor, highlight); nothing goes on the undo
    /// chain (C's `MOVE_UI_UPDATE`).
    #[doc(alias = "MOVE_UI_UPDATE")]
    UiUpdate,
    /// A move to execute and record.
    Move(M),
}

/// A user-facing error from a game: invalid parameters, description,
/// move or solve request. The messages are C's, word for word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameError(pub Cow<'static, str>);

impl GameError {
    /// An error with a fixed message, as most C messages are.
    #[must_use]
    pub const fn new(message: &'static str) -> Self {
        Self(Cow::Borrowed(message))
    }

    /// What the trait's default methods return for a capability the
    /// game doesn't have. The mid-end checks the `CAN_*` consts first, so
    /// users never see it.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self::new("This game does not support this operation")
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GameError {}

impl From<String> for GameError {
    fn from(message: String) -> Self {
        Self(Cow::Owned(message))
    }
}

/// A move string a game can't read: C's `execute_move` returning NULL
/// for text it can't parse. C gives no reason, and users see the
/// mid-end's generic message ("Save file contained an invalid move")
/// whatever the cause, so the error carries none either.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveParseError;

impl fmt::Display for MoveParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid move string")
    }
}

impl std::error::Error for MoveParseError {}

/// Whether a game is over (C's `status`: 0, +1, -1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameStatus {
    /// Still being played (0).
    Ongoing,
    /// Solved (+1).
    Won,
    /// Lost, e.g. a mine was hit (-1).
    Lost,
}

/// A key a game asks a keyboardless front end to offer as a button (C's
/// `key_label`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyLabel {
    /// What to show; `None` asks for the generic label
    /// ([`Button::label`]).
    pub label: Option<Cow<'static, str>>,
    /// What pressing it sends.
    pub button: Button,
}
