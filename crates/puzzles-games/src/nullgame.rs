//! Port of third_party/puzzles-c/nullgame.c (upstream 616da16).
//!
//! The null game: C's template for new games, in which no move is ever
//! possible and nothing is drawn. It is the smallest complete [`Game`],
//! for tests of the mid-end and of the trait itself. Its "FIXME" texts
//! are C's.

use std::convert::Infallible;

use puzzles_core::button::Button;
use puzzles_core::drawing::Drawing;
use puzzles_core::game::{
    Game, GameCtx, GameError, GameStatus, MoveParseError, MoveResult, NewDesc,
};
use puzzles_core::random::RandomState;

puzzles_core::palette! {
    /// The null game's only colour.
    pub enum Col {
        Background => Role(Background),
    }
}

/// The null game.
#[derive(Clone, Copy, Debug)]
pub struct NullGame;

/// The draw state: just the tile size.
#[derive(Clone, Debug, Default)]
pub struct DrawState {
    tilesize: i32,
}

impl Game for NullGame {
    type Params = ();
    type State = ();
    type Ui = ();
    type DrawState = DrawState;
    type Move = Infallible;

    const NAME: &'static str = "Null Game";
    const PREFERRED_TILESIZE: i32 = 20;
    const PALETTE: &'static [puzzles_core::palette::PaletteEntry] = Col::PALETTE;

    fn default_params() {}

    fn decode_params((): &mut (), _s: &str) {}

    fn encode_params((): &(), _full: bool) -> String {
        "FIXME".to_owned()
    }

    fn validate_params((): &(), _full: bool) -> Result<(), GameError> {
        Ok(())
    }

    fn new_desc((): &(), _rng: &mut RandomState, _interactive: bool) -> NewDesc {
        NewDesc::new("FIXME")
    }

    fn validate_desc((): &(), _desc: &str) -> Result<(), GameError> {
        Ok(())
    }

    fn new_game(_ctx: &mut GameCtx, (): &(), _desc: &str) {}

    fn status((): &()) -> GameStatus {
        GameStatus::Ongoing
    }

    fn new_ui(_state: Option<&()>) {}

    fn interpret_move(
        (): &(),
        (): &mut (),
        _ds: &DrawState,
        _x: i32,
        _y: i32,
        _button: Button,
    ) -> MoveResult<Infallible> {
        MoveResult::Unused
    }

    fn execute_move(_ctx: &mut GameCtx, (): &(), mv: &Infallible) -> Option<()> {
        match *mv {}
    }

    fn encode_move(mv: &Infallible, _out: &mut String) {
        match *mv {}
    }

    fn decode_move((): &(), _text: &str) -> Result<Infallible, MoveParseError> {
        Err(MoveParseError)
    }

    fn compute_size((): &(), tilesize: i32, (): &()) -> (i32, i32) {
        (10 * tilesize, 10 * tilesize)
    }

    fn new_drawstate((): &()) -> DrawState {
        DrawState::default()
    }

    fn set_size(ds: &mut DrawState, (): &(), tilesize: i32) {
        ds.tilesize = tilesize;
    }

    fn redraw(
        _dr: &mut Drawing<'_>,
        _ds: &mut DrawState,
        _old: Option<&()>,
        (): &(),
        _dir: i32,
        (): &(),
        _anim_time: f32,
        _flash_time: f32,
    ) {
    }
}
