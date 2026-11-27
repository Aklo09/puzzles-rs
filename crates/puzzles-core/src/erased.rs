//! The type-erased mid-end front ends use: [`Puzzle`], implemented by
//! every [`Midend<G>`], so a front end can hold any game as a
//! `Box<dyn Puzzle>` ("box at the boundary", `docs/porting-plan.md`
//! §3.1). Everything game-specific crosses it as text or as
//! [`ConfigItem`]s, as with C's game-agnostic `midend_*` functions;
//! presets are referred to by id.

use std::io::{Read, Write};

use crate::button::Button;
use crate::config::ConfigItem;
use crate::drawing::DrawingApi;
use crate::game::{Game, GameError, GameFlags, GameStatus, KeyLabel, Rect};
use crate::midend::{Config, ConfigKind, DeserialiseError, KeyResult, Midend};
use crate::palette::PaletteEntry;
use crate::preset::PresetMenu;

/// What a game can do, for enabling menu entries (the `CAN_*` consts
/// and `FLAGS` of its [`Game`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "one per C capability flag")]
pub struct Capabilities {
    /// Has a "Custom" dialog.
    pub can_configure: bool,
    /// Has a solver.
    pub can_solve: bool,
    /// Can copy the position as text, at least for some parameters.
    pub can_format_as_text_ever: bool,
    /// Shows a status bar.
    pub wants_statusbar: bool,
    /// Shows a timer.
    pub is_timed: bool,
    /// C's flags word.
    pub flags: GameFlags,
}

/// A mid-end of any game; see [`Midend`] for what each method does.
pub trait Puzzle {
    /// The game's name.
    fn name(&self) -> &'static str;
    /// What the game can do.
    fn capabilities(&self) -> Capabilities;
    /// The game's colours (D7).
    fn palette(&self) -> &'static [PaletteEntry];

    /// [`Midend::size`].
    fn size(
        &mut self,
        max_w: i32,
        max_h: i32,
        user_size: bool,
        device_pixel_ratio: f64,
    ) -> (i32, i32);
    /// [`Midend::tilesize`].
    fn tilesize(&self) -> i32;
    /// [`Midend::reset_tilesize`].
    fn reset_tilesize(&mut self);

    /// [`Midend::new_game`].
    fn new_game(&mut self);
    /// [`Midend::restart_game`].
    fn restart_game(&mut self);
    /// [`Midend::process_key`]; undo, redo, new game, solve and quit are
    /// keys too (`UiAction`).
    fn process_key(&mut self, x: i32, y: i32, button: Button) -> KeyResult;
    /// [`Midend::timer`].
    fn timer(&mut self, tplus: f32);
    /// [`Midend::stop_anim`].
    fn stop_anim(&mut self);
    /// [`Midend::wants_timer`].
    fn wants_timer(&self) -> bool;
    /// [`Midend::is_dirty`].
    fn is_dirty(&self) -> bool;
    /// [`Midend::take_id_changed`].
    fn take_id_changed(&mut self) -> bool;
    /// [`Midend::redraw`].
    fn redraw(&mut self, api: &mut dyn DrawingApi);
    /// [`Midend::force_redraw`].
    fn force_redraw(&mut self);
    /// [`Midend::status_text`].
    fn status_text(&self) -> String;

    /// [`Midend::solve`].
    ///
    /// # Errors
    /// C's message.
    fn solve(&mut self) -> Result<(), GameError>;
    /// [`Midend::status`].
    fn status(&self) -> GameStatus;
    /// [`Midend::can_undo`].
    fn can_undo(&self) -> bool;
    /// [`Midend::can_redo`].
    fn can_redo(&self) -> bool;
    /// [`Midend::can_format_as_text_now`].
    fn can_format_as_text_now(&self) -> bool;
    /// [`Midend::text_format`].
    fn text_format(&self) -> Option<String>;

    /// [`Midend::game_id`].
    ///
    /// # Errors
    /// The game's message.
    fn game_id(&mut self, id: &str) -> Result<(), GameError>;
    /// [`Midend::get_game_id`].
    fn get_game_id(&self) -> Option<String>;
    /// [`Midend::get_random_seed`].
    fn get_random_seed(&self) -> Option<String>;
    /// [`Midend::get_config`].
    fn get_config(&self, kind: ConfigKind) -> Option<Config>;
    /// [`Midend::set_config`].
    ///
    /// # Errors
    /// The game's message.
    fn set_config(&mut self, kind: ConfigKind, items: &[ConfigItem]) -> Result<(), GameError>;

    /// [`Midend::presets`].
    fn presets(&mut self) -> &PresetMenu<usize>;
    /// Chooses a preset for the next New Game (C's
    /// `midend_set_params` with the preset's parameters); false for an
    /// unknown id.
    fn set_preset(&mut self, id: usize) -> bool;
    /// [`Midend::which_preset`].
    fn which_preset(&self) -> Option<usize>;

    /// [`Midend::request_keys`].
    fn request_keys(&self) -> Vec<KeyLabel>;
    /// [`Midend::current_key_label`].
    fn current_key_label(&self, button: Button) -> Option<&'static str>;
    /// [`Midend::cursor_location`].
    fn cursor_location(&self) -> Option<Rect>;

    /// [`Midend::serialise`].
    ///
    /// # Errors
    /// Writing errors.
    fn serialise(&self, w: &mut dyn Write) -> std::io::Result<()>;
    /// [`Midend::deserialise`].
    ///
    /// # Errors
    /// C's message, or reading errors.
    fn deserialise(&mut self, r: &mut dyn Read) -> Result<(), DeserialiseError>;
    /// [`Midend::save_prefs`].
    ///
    /// # Errors
    /// Writing errors.
    fn save_prefs(&self, w: &mut dyn Write) -> std::io::Result<()>;
    /// [`Midend::load_prefs`].
    ///
    /// # Errors
    /// C's message, or reading errors.
    fn load_prefs(&mut self, r: &mut dyn Read) -> Result<(), DeserialiseError>;
}

impl<G: Game> Puzzle for Midend<G> {
    fn name(&self) -> &'static str {
        G::NAME
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            can_configure: G::CAN_CONFIGURE,
            can_solve: G::CAN_SOLVE,
            can_format_as_text_ever: G::CAN_FORMAT_AS_TEXT_EVER,
            wants_statusbar: G::WANTS_STATUSBAR,
            is_timed: G::IS_TIMED,
            flags: G::FLAGS,
        }
    }

    fn palette(&self) -> &'static [PaletteEntry] {
        G::PALETTE
    }

    fn size(
        &mut self,
        max_w: i32,
        max_h: i32,
        user_size: bool,
        device_pixel_ratio: f64,
    ) -> (i32, i32) {
        Midend::size(self, max_w, max_h, user_size, device_pixel_ratio)
    }

    fn tilesize(&self) -> i32 {
        Midend::tilesize(self)
    }

    fn reset_tilesize(&mut self) {
        Midend::reset_tilesize(self);
    }

    fn new_game(&mut self) {
        Midend::new_game(self);
    }

    fn restart_game(&mut self) {
        Midend::restart_game(self);
    }

    fn process_key(&mut self, x: i32, y: i32, button: Button) -> KeyResult {
        Midend::process_key(self, x, y, button)
    }

    fn timer(&mut self, tplus: f32) {
        Midend::timer(self, tplus);
    }

    fn stop_anim(&mut self) {
        Midend::stop_anim(self);
    }

    fn wants_timer(&self) -> bool {
        Midend::wants_timer(self)
    }

    fn is_dirty(&self) -> bool {
        Midend::is_dirty(self)
    }

    fn take_id_changed(&mut self) -> bool {
        Midend::take_id_changed(self)
    }

    fn redraw(&mut self, api: &mut dyn DrawingApi) {
        Midend::redraw(self, api);
    }

    fn force_redraw(&mut self) {
        Midend::force_redraw(self);
    }

    fn status_text(&self) -> String {
        Midend::status_text(self)
    }

    fn solve(&mut self) -> Result<(), GameError> {
        Midend::solve(self)
    }

    fn status(&self) -> GameStatus {
        Midend::status(self)
    }

    fn can_undo(&self) -> bool {
        Midend::can_undo(self)
    }

    fn can_redo(&self) -> bool {
        Midend::can_redo(self)
    }

    fn can_format_as_text_now(&self) -> bool {
        Midend::can_format_as_text_now(self)
    }

    fn text_format(&self) -> Option<String> {
        Midend::text_format(self)
    }

    fn game_id(&mut self, id: &str) -> Result<(), GameError> {
        Midend::game_id(self, id)
    }

    fn get_game_id(&self) -> Option<String> {
        Midend::get_game_id(self)
    }

    fn get_random_seed(&self) -> Option<String> {
        Midend::get_random_seed(self)
    }

    fn get_config(&self, kind: ConfigKind) -> Option<Config> {
        Midend::get_config(self, kind)
    }

    fn set_config(&mut self, kind: ConfigKind, items: &[ConfigItem]) -> Result<(), GameError> {
        Midend::set_config(self, kind, items)
    }

    fn presets(&mut self) -> &PresetMenu<usize> {
        Midend::presets(self)
    }

    fn set_preset(&mut self, id: usize) -> bool {
        Midend::presets(self);
        match self.preset_params(id).cloned() {
            Some(params) => {
                self.set_params(&params);
                true
            }
            None => false,
        }
    }

    fn which_preset(&self) -> Option<usize> {
        Midend::which_preset(self)
    }

    fn request_keys(&self) -> Vec<KeyLabel> {
        Midend::request_keys(self)
    }

    fn current_key_label(&self, button: Button) -> Option<&'static str> {
        Midend::current_key_label(self, button)
    }

    fn cursor_location(&self) -> Option<Rect> {
        Midend::cursor_location(self)
    }

    fn serialise(&self, mut w: &mut dyn Write) -> std::io::Result<()> {
        Midend::serialise(self, &mut w)
    }

    fn deserialise(&mut self, mut r: &mut dyn Read) -> Result<(), DeserialiseError> {
        Midend::deserialise(self, &mut r)
    }

    fn save_prefs(&self, mut w: &mut dyn Write) -> std::io::Result<()> {
        Midend::save_prefs(self, &mut w)
    }

    fn load_prefs(&mut self, mut r: &mut dyn Read) -> Result<(), DeserialiseError> {
        Midend::load_prefs(self, &mut r)
    }
}
