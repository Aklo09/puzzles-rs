//! Port of third_party/puzzles-c/mosaic.c (upstream 616da16).
//!
//! Mosaic (also known as Fill-a-Pix): each clue counts the black squares
//! in the 3 × 3 block around and including it. A click cycles a square
//! through black, white and undecided (a right click goes the other
//! way); dragging along a row or column paints the undecided squares on
//! the way like the first one. A clue turns grey once its block is
//! complete and right, and red once it can no longer be.
//!
//! The generator draws a random picture, shows every clue, checks that
//! simple deduction (one clue at a time) solves it, then hides the clues
//! the solution didn't need; with "aggressive" generation it also tries
//! hiding each needed clue in random order, keeping it hidden if the
//! puzzle is still solved without it.
//!
//! Known C behaviour kept as is:
//! * the generator's check for a clue to start from (0, or a full block)
//!   looks only at the first (h - 1) × (w - 1) squares in reading order,
//!   not at the whole grid;
//! * squares are C `char`s, signed on every platform CI covers. A drag
//!   move read from a file can store any value in a square (its last
//!   field is the state to paint, unchecked), and such values then show
//!   through C's arithmetic and drawing flags exactly as in C;
//! * a move string starting with an unknown letter (or empty) is accepted
//!   and only recounts the clues left. The port reads it as
//!   [`Move::Recount`] and writes it back as the empty move, which C
//!   reads the same way.
//!
//! Differences (UI only): the Custom dialog lists Width before Height
//! (C lists Height first; custom dialog field order decision).
//!
//! Numbers in move strings are read with C's `atoi`, so a field of more
//! than nine digits wraps as with a 64-bit `long` (Linux, macOS; see
//! `puzzles_core::parse`). Fields C requires to be bare digits are
//! written back as their 32-bit unsigned value, which reads back to the
//! same number, so even such a move survives a save.

use std::fmt::Write as _;
use std::rc::Rc;

use puzzles_core::button::{Button, ButtonKind, CursorKey, MouseAction, MouseButton};
use puzzles_core::config::ConfigItem;
use puzzles_core::drawing::{Align, Drawing, FontType};
use puzzles_core::game::{
    Game, GameCtx, GameError, GameStatus, MoveParseError, MoveResult, NewDesc, Rect,
};
use puzzles_core::misc::{env_bool, move_cursor};
use puzzles_core::palette::PaletteEntry;
use puzzles_core::parse::{atoi, skip_digits};
use puzzles_core::preset::PresetMenu;
use puzzles_core::random::RandomState;

const DEFAULT_SIZE: i32 = 10;
const DEFAULT_AGGRESSIVE: bool = true;
const MAX_TILES: i32 = 10_000;
const PREFERRED_TILE_SIZE: i32 = 32;
/// Seconds of the completion flash.
const FLASH_TIME: f32 = 0.5;

// A square's state (C's `enum cell_state`): the low two bits are the
// player's mark, the others what the clue there says about its block.
/// Black.
const MARKED: i8 = 1;
/// White.
const BLANK: i8 = 2;
/// The clue here is satisfied.
const SOLVED: i8 = 4;
/// The clue here can't be satisfied any more.
const ERROR: i8 = 8;
/// Both mark bits (C's `STATE_OK_NUM`, also the number of marks).
const MARKS: i8 = BLANK | MARKED;

// Extra bits of a drawn tile, beyond the square's state.
const DRAW_CURSOR: i32 = 0x100;
const DRAW_CURSOR_U: i32 = 0x200;
const DRAW_CURSOR_L: i32 = 0x400;
const DRAW_CURSOR_UL: i32 = 0x800;
/// The tile is right of the grid: only its left edge is drawn.
const DRAW_MARGIN_R: i32 = 0x1000;
/// The tile is below the grid: only its top edge is drawn.
const DRAW_MARGIN_D: i32 = 0x2000;

puzzles_core::palette! {
    /// Mosaic's colours.
    pub enum Col {
        Background => Role(Background),
        /// An undecided square.
        Unmarked => Specific("unmarked"),
        Grid => Role(Grid),
        /// A black square, and clue text on white and undecided ones.
        Marked => Role(Filled),
        /// A white square, and clue text on black ones.
        Blank => Specific("blank"),
        /// A satisfied clue.
        TextSolved => Role(Done),
        Error => Role(Error),
        Cursor => Role(Cursor),
    }
}

/// Clue text on light squares (C's `COL_TEXT_DARK`).
const TEXT_DARK: Col = Col::Marked;
/// Clue text on black squares (C's `COL_TEXT_LIGHT`).
const TEXT_LIGHT: Col = Col::Blank;

/// Mosaic's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Width in squares.
    pub w: i32,
    /// Height in squares.
    pub h: i32,
    /// Also try hiding the clues the first solution needed (slower,
    /// fewer clues).
    pub aggressive: bool,
}

/// The size of a grid, and its squares' indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Grid {
    w: i32,
    h: i32,
}

impl Grid {
    fn of(params: &Params) -> Self {
        Self {
            w: params.w,
            h: params.h,
        }
    }

    fn len(self) -> usize {
        (self.w * self.h) as usize
    }

    /// The square at `(x, y)`, if it is on the grid (C's `get_coords`).
    fn index(self, x: i32, y: i32) -> Option<usize> {
        ((0..self.w).contains(&x) && (0..self.h).contains(&y)).then(|| (y * self.w + x) as usize)
    }

    /// Every square in reading order, with its coordinates.
    fn squares(self) -> impl Iterator<Item = (usize, i32, i32)> {
        (0..self.h).flat_map(move |y| (0..self.w).map(move |x| ((y * self.w + x) as usize, x, y)))
    }

    /// The squares of the 3 × 3 block centred on `(x, y)` that are on the
    /// grid, column by column as C visits them.
    fn block(self, x: i32, y: i32) -> impl Iterator<Item = usize> {
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
            .filter_map(move |(x, y)| self.index(x, y))
    }

    /// How a block is marked (C's `count_around`), given each square's
    /// state.
    fn count(self, x: i32, y: i32, state: impl Fn(usize) -> i8) -> Counts {
        let mut counts = Counts::default();
        for i in self.block(x, y) {
            counts.total += 1;
            let s = state(i);
            if s & BLANK != 0 {
                counts.blank += 1;
            } else if s & MARKED != 0 {
                counts.marked += 1;
            }
        }
        counts
    }
}

/// The marks in a block.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    marked: i32,
    blank: i32,
    /// Squares in the block that are on the grid.
    total: i32,
}

/// A position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    grid: Grid,
    /// The solver was used.
    cheating: bool,
    /// Clues not yet satisfied; 0 when the game is over.
    not_completed_clues: i32,
    /// Each square's state (C's `char`): a mark and clue bits.
    cells: Vec<i8>,
    /// Each square's clue (0 to 9), if shown. Shared by every state of a
    /// game.
    clues: Rc<[Option<u8>]>,
}

impl State {
    /// C's `update_board_state_around`: rechecks the clues in the block
    /// around `(x, y)`.
    fn update_clues_around(&mut self, x: i32, y: i32) {
        let grid = self.grid;
        for (cx, cy) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy))) {
            let Some(i) = grid.index(cx, cy) else {
                continue;
            };
            let Some(clue) = self.clues[i] else {
                continue;
            };
            let clue = i32::from(clue);
            let c = grid.count(cx, cy, |j| self.cells[j]);
            let mark = self.cells[i] & MARKS;
            self.cells[i] = if clue == c.marked && c.total - c.marked - c.blank == 0 {
                mark | SOLVED
            } else if clue < c.marked || clue > c.total - c.blank {
                mark | ERROR
            } else {
                mark
            };
        }
    }

    /// Counts the clues not yet satisfied.
    fn recount(&mut self) {
        let left = self
            .clues
            .iter()
            .zip(&self.cells)
            .filter(|&(clue, &cell)| clue.is_some() && cell & SOLVED == 0)
            .count();
        self.not_completed_clues = left as i32;
    }
}

/// A move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Move {
    /// Cycle a square once (`t<x>,<y>`: undecided, black, white) or twice
    /// (`T<x>,<y>`, the other way round). With `x` or `y` at -1 it does
    /// nothing, as in C.
    Toggle {
        /// Column.
        x: i32,
        /// Row.
        y: i32,
        /// Cycle twice (a right click).
        twice: bool,
    },
    /// Paint the undecided squares from `(x, y)` towards `(from_x,
    /// from_y)`, that one excluded, with `state`: `d<x>,<y>,<from_x>,
    /// <from_y>,<state>` while dragging, `e…` on release (the same move).
    /// The squares lie along the row, or the column if `from_x == x`.
    Drag {
        /// Column the drag reached.
        x: i32,
        /// Row the drag reached.
        y: i32,
        /// Column the drag came from.
        from_x: i32,
        /// Row the drag came from.
        from_y: i32,
        /// What to paint: a square's state, unchecked (C stores it as a
        /// `char`).
        state: i32,
        /// The button was released (`e` rather than `d`).
        release: bool,
    },
    /// The solution: `s` and two hex digits per eight squares, black
    /// squares as 1 bits, high bit first. A square beyond the given bits
    /// keeps its state.
    Solve(Vec<bool>),
    /// Only recount the clues left: any move string C doesn't know.
    /// Written as the empty move.
    Recount,
}

/// The cursor, and the square a drag came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ui {
    /// The last square clicked or dragged over, (-1, -1) if none.
    last_x: i32,
    last_y: i32,
    /// What a drag paints: the state the click gave its first square.
    last_state: i32,
    cur_x: i32,
    cur_y: i32,
    cur_visible: bool,
}

/// What was last drawn.
#[derive(Clone, Debug)]
pub struct DrawState {
    tilesize: i32,
    /// Columns of `tiles`: the grid's width plus the right margin.
    stride: i32,
    /// Per tile, including the right and bottom margins, the bits last
    /// drawn. -1 at first, as in C: it is also a value a tile can have
    /// (from a square holding -1), and such a tile is then never drawn,
    /// in C as here.
    tiles: Vec<i32>,
}

impl DrawState {
    /// C's `COORD_FROM_CELL`.
    fn coord(&self, d: i32) -> i32 {
        d * self.tilesize + self.tilesize / 2 - 1
    }
}

/// The Mosaic puzzle.
#[derive(Clone, Copy, Debug)]
pub struct Mosaic;

impl Game for Mosaic {
    type Params = Params;
    type State = State;
    type Ui = Ui;
    type DrawState = DrawState;
    type Move = Move;

    const NAME: &'static str = "Mosaic";
    const PREFERRED_TILESIZE: i32 = PREFERRED_TILE_SIZE;
    const PALETTE: &'static [PaletteEntry] = Col::PALETTE;
    const CAN_CONFIGURE: bool = true;
    const CAN_SOLVE: bool = true;
    const CAN_FORMAT_AS_TEXT_EVER: bool = true;
    const WANTS_STATUSBAR: bool = true;

    fn default_params() -> Params {
        Params {
            w: DEFAULT_SIZE,
            h: DEFAULT_SIZE,
            aggressive: DEFAULT_AGGRESSIVE,
        }
    }

    fn presets() -> PresetMenu<Params> {
        let mut menu = PresetMenu::new();
        for (size, aggressive) in [
            (3, true),
            (5, true),
            (10, true),
            (15, true),
            (25, true),
            (50, false),
        ] {
            menu.add_preset(
                format!("Size: {size}x{size}"),
                Params {
                    w: size,
                    h: size,
                    aggressive,
                },
            );
        }
        menu
    }

    fn decode_params(params: &mut Params, s: &str) {
        let mut s = s.as_bytes();
        params.w = atoi(s);
        params.h = params.w;
        s = skip_digits(s);
        if let [b'x', rest @ ..] = s {
            params.h = atoi(rest);
            s = skip_digits(rest);
        }
        if let [b'h', rest @ ..] = s {
            params.aggressive = atoi(rest) != 0;
        }
    }

    fn encode_params(params: &Params, full: bool) -> String {
        let mut s = format!("{}x{}", params.w, params.h);
        if full && params.aggressive != DEFAULT_AGGRESSIVE {
            let _ = write!(s, "h{}", u8::from(params.aggressive));
        }
        s
    }

    fn configure(params: &Params) -> Option<Vec<ConfigItem>> {
        // C lists Height first; every port lists Width first.
        Some(vec![
            ConfigItem::string("Width", params.w.to_string()),
            ConfigItem::string("Height", params.h.to_string()),
            ConfigItem::boolean("Aggressive generation (longer)", params.aggressive),
        ])
    }

    fn custom_params(config: &[ConfigItem]) -> Result<Params, GameError> {
        let wrong = || GameError::new("Unexpected configuration fields");
        let [w, h, aggressive] = config else {
            return Err(wrong());
        };
        // C's atol, truncated to an int.
        let number =
            |item: &ConfigItem| item.as_str().map(|s| atoi(s.as_bytes())).ok_or_else(wrong);
        Ok(Params {
            w: number(w)?,
            h: number(h)?,
            aggressive: aggressive.as_bool().ok_or_else(wrong)?,
        })
    }

    fn validate_params(params: &Params, _full: bool) -> Result<(), GameError> {
        if params.h < 3 || params.w < 3 {
            return Err(GameError::new("Minimal size is 3x3"));
        }
        if params.h > MAX_TILES / params.w {
            return Err(GameError::new("Maximum size is 10000 tiles"));
        }
        Ok(())
    }

    // C: mosaic.c, new_game_desc()
    fn new_desc(params: &Params, rng: &mut RandomState, _interactive: bool) -> NewDesc {
        let grid = Grid::of(params);
        let mut image = vec![false; grid.len()];
        let mut clues = vec![Clue::default(); grid.len()];
        loop {
            for pixel in &mut image {
                *pixel = rng.bits(1) != 0;
            }
            for (i, x, y) in grid.squares() {
                clues[i] = Clue::from_image(grid, &image, x, y);
            }
            // C looks for a clue to start from in part of the grid only.
            let start_area = ((grid.h - 1) * (grid.w - 1)) as usize;
            if !clues[..start_area].iter().any(|c| c.empty || c.full) {
                continue;
            }
            if solve_with_clues(grid, &clues, Some(rng)).0 {
                hide_clues(grid, params.aggressive, &mut clues, rng);
                break;
            }
        }
        NewDesc::new(encode_desc(&clues))
    }

    fn validate_desc(params: &Params, desc: &str) -> Result<(), GameError> {
        let mut len = 0_usize;
        for &b in desc.as_bytes() {
            len += match b {
                b'a'..=b'z' => usize::from(b - b'a') + 1,
                b'0'..=b'9' => 1,
                _ => return Err(GameError::new("Invalid character in game description")),
            };
        }
        if len != Grid::of(params).len() {
            return Err(GameError::new("Desc size mismatch"));
        }
        Ok(())
    }

    fn new_game(_ctx: &mut GameCtx, params: &Params, desc: &str) -> State {
        let grid = Grid::of(params);
        let mut clues = Vec::with_capacity(grid.len());
        for &b in desc.as_bytes() {
            if b.is_ascii_digit() {
                clues.push(Some(b - b'0'));
            } else {
                // A letter: a run of hidden clues ('a' is one).
                let run = usize::from(b.wrapping_sub(b'a')) + 1;
                clues.extend(std::iter::repeat_n(None, run));
            }
        }
        clues.resize(grid.len(), None);
        let shown = clues.iter().filter(|c| c.is_some()).count();
        State {
            grid,
            cheating: false,
            not_completed_clues: shown as i32,
            cells: vec![0; grid.len()],
            clues: clues.into(),
        }
    }

    // C: mosaic.c, solve_game()
    fn solve(original: &State, _current: &State, _aux: Option<&str>) -> Result<Move, GameError> {
        let clues: Vec<Clue> = original.clues.iter().map(|&c| Clue::shown(c)).collect();
        let (solved, sol) = solve_by_rows(original.grid, &clues);
        if !solved {
            return Err(GameError::new("Could not solve this board"));
        }
        Ok(Move::Solve(sol.iter().map(|s| s.mark == MARKED).collect()))
    }

    fn text_format(state: &State) -> Option<String> {
        let w = state.grid.w as usize;
        let mut s = String::with_capacity(state.cells.len() * 3 + state.grid.h as usize);
        for row in state.clues.chunks_exact(w) {
            for clue in row {
                match clue {
                    Some(c) => {
                        let _ = write!(s, "|{c}|");
                    }
                    None => s.push_str("| |"),
                }
            }
            s.push('\n');
        }
        Some(s)
    }

    fn status(state: &State) -> GameStatus {
        if state.not_completed_clues == 0 {
            GameStatus::Won
        } else {
            GameStatus::Ongoing
        }
    }

    fn new_ui(_state: Option<&State>) -> Ui {
        Ui {
            last_x: -1,
            last_y: -1,
            last_state: 0,
            cur_x: 0,
            cur_y: 0,
            cur_visible: env_bool("PUZZLES_SHOW_CURSOR").unwrap_or(false),
        }
    }

    fn current_key_label(ui: &Ui, state: &State, button: Button) -> Option<&'static str> {
        let select = cursor_select(button)?;
        if !ui.cur_visible || state.not_completed_clues == 0 {
            return None;
        }
        let i = state.grid.index(ui.cur_x, ui.cur_y)?;
        let twice = select == CursorKey::Select2;
        match state.cells[i] & MARKS {
            0 => Some(if twice { "White" } else { "Black" }),
            MARKED => Some(if twice { "Empty" } else { "White" }),
            BLANK => Some(if twice { "Black" } else { "Empty" }),
            _ => None,
        }
    }

    // C: mosaic.c, interpret_move()
    fn interpret_move(
        state: &State,
        ui: &mut Ui,
        ds: &DrawState,
        x: i32,
        y: i32,
        button: Button,
    ) -> MoveResult<Move> {
        // C compares whole button codes, so a modified key is none of
        // these.
        let plain = button.mods.is_empty();
        let arrow = plain && button.is_cursor_move();
        if state.not_completed_clues == 0 && !arrow {
            return MoveResult::Unused;
        }
        let grid = state.grid;
        let ts = ds.tilesize;
        let (offset_x, offset_y) = (x - ts / 2, y - ts / 2);
        let (gx, gy) = (offset_x / ts, offset_y / ts);
        let mouse = match button.kind {
            ButtonKind::Mouse(b, action) if plain => Some((b, action)),
            _ => None,
        };
        if mouse.is_some() && (offset_x < 0 || offset_y < 0) {
            return MoveResult::Unused;
        }
        let (src_x, src_y) = (ui.last_x, ui.last_y);

        match mouse {
            Some((b @ (MouseButton::Left | MouseButton::Right), MouseAction::Press)) => {
                let twice = b == MouseButton::Right;
                let result = if let Some(i) = grid.index(gx, gy) {
                    let mark = i32::from(state.cells[i] & MARKS);
                    ui.last_state = (mark + if twice { 2 } else { 1 }) % i32::from(MARKS);
                    (ui.last_x, ui.last_y) = (gx, gy);
                    MoveResult::Move(Move::Toggle {
                        x: gx,
                        y: gy,
                        twice,
                    })
                } else {
                    (ui.last_x, ui.last_y) = (-1, -1);
                    MoveResult::Unused
                };
                ui.cur_visible = false;
                result
            }
            Some((
                MouseButton::Left | MouseButton::Right,
                action @ (MouseAction::Drag | MouseAction::Release),
            )) => {
                let release = action == MouseAction::Release;
                let mut result = MoveResult::Unused;
                // Only drags along a row or column.
                if grid.index(gx, gy).is_some()
                    && ui.last_x >= 0
                    && ui.last_y >= 0
                    && (gy == ui.last_y || gx == ui.last_x)
                {
                    let mv = Move::Drag {
                        x: gx,
                        y: gy,
                        from_x: ui.last_x,
                        from_y: ui.last_y,
                        state: ui.last_state,
                        release,
                    };
                    let changed = ui.last_state > 0
                        && line(gx, gy, src_x, src_y).any(|(cx, cy)| {
                            grid.index(cx, cy)
                                .is_some_and(|i| state.cells[i] & MARKS == 0)
                        });
                    // C keeps the last square on release.
                    if !release {
                        (ui.last_x, ui.last_y) = (gx, gy);
                    }
                    if changed {
                        result = MoveResult::Move(mv);
                    }
                } else {
                    (ui.last_x, ui.last_y) = (-1, -1);
                }
                ui.cur_visible = false;
                result
            }
            Some(_) => MoveResult::Unused,
            None if arrow => move_cursor(
                button,
                &mut ui.cur_x,
                &mut ui.cur_y,
                grid.w,
                grid.h,
                false,
                Some(&mut ui.cur_visible),
            ),
            None => match cursor_select(button) {
                Some(_) if !ui.cur_visible => {
                    (ui.cur_x, ui.cur_y) = (0, 0);
                    ui.cur_visible = true;
                    MoveResult::UiUpdate
                }
                Some(key) => MoveResult::Move(Move::Toggle {
                    x: ui.cur_x,
                    y: ui.cur_y,
                    twice: key == CursorKey::Select2,
                }),
                None => MoveResult::Unused,
            },
        }
    }

    // C: mosaic.c, execute_move()
    fn execute_move(_ctx: &mut GameCtx, from: &State, mv: &Move) -> Option<State> {
        let grid = from.grid;
        let mut state = from.clone();
        match *mv {
            Move::Toggle { x, y, twice } => {
                if x == -1 || y == -1 {
                    // Not even a recount.
                    return Some(state);
                }
                let i = grid.index(x, y)?;
                let mut cell = state.cells[i];
                if cell >= MARKS {
                    cell &= MARKS;
                }
                // C's int arithmetic; a negative square stays negative.
                let steps = if twice { 2 } else { 1 };
                state.cells[i] = ((i32::from(cell) + steps) % i32::from(MARKS)) as i8;
                state.update_clues_around(x, y);
            }
            Move::Drag {
                x,
                y,
                from_x,
                from_y,
                state: paint,
                ..
            } => {
                for (cx, cy) in line(x, y, from_x, from_y) {
                    let i = grid.index(cx, cy)?;
                    if state.cells[i] & MARKS == 0 {
                        // C stores the int in a char.
                        state.cells[i] = paint as i8;
                        state.update_clues_around(cx, cy);
                    }
                }
            }
            Move::Solve(ref marked) => {
                state.not_completed_clues = 0;
                state.cheating = true;
                for (cell, &m) in state.cells.iter_mut().zip(marked) {
                    *cell = if m { MARKED | SOLVED } else { BLANK | SOLVED };
                }
                return Some(state);
            }
            Move::Recount => {}
        }
        state.recount();
        Some(state)
    }

    fn encode_move(mv: &Move, out: &mut String) {
        // Fields C reads as bare digits are written unsigned (see the
        // module notes); C's own moves only have small positive ones.
        let _ = match *mv {
            Move::Toggle { x, y, twice } => {
                write!(out, "{}{},{y}", if twice { 'T' } else { 't' }, x as u32)
            }
            Move::Drag {
                x,
                y,
                from_x,
                from_y,
                state,
                release,
            } => write!(
                out,
                "{}{},{},{},{},{state}",
                if release { 'e' } else { 'd' },
                x as u32,
                y as u32,
                from_x as u32,
                from_y as u32
            ),
            Move::Solve(ref marked) => {
                out.push('s');
                for chunk in marked.chunks(8) {
                    let byte = chunk
                        .iter()
                        .enumerate()
                        .filter(|&(_, &m)| m)
                        .fold(0_u8, |b, (k, _)| b | (0x80 >> k));
                    let _ = write!(out, "{byte:02x}");
                }
                Ok(())
            }
            Move::Recount => Ok(()),
        };
    }

    fn decode_move(state: &State, text: &str) -> Result<Move, MoveParseError> {
        // C reads up to the first NUL.
        let bytes = text.as_bytes();
        let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
        let Some((&kind, rest)) = bytes.split_first() else {
            return Ok(Move::Recount);
        };
        Ok(match kind {
            b't' | b'T' => {
                let [x, y] = read_fields(rest)?;
                Move::Toggle {
                    x,
                    y,
                    twice: kind == b'T',
                }
            }
            b'd' | b'e' => {
                let [x, y, from_x, from_y, state] = read_fields(rest)?;
                Move::Drag {
                    x,
                    y,
                    from_x,
                    from_y,
                    state,
                    release: kind == b'e',
                }
            }
            b's' => Move::Solve(read_solution(rest, state.cells.len())),
            _ => Move::Recount,
        })
    }

    fn compute_size(params: &Params, tilesize: i32, _ui: &Ui) -> (i32, i32) {
        ((params.w + 1) * tilesize, (params.h + 1) * tilesize)
    }

    fn new_drawstate(state: &State) -> DrawState {
        let (w, h) = (state.grid.w, state.grid.h);
        DrawState {
            tilesize: 0,
            stride: w + 1,
            tiles: vec![-1; ((w + 1) * (h + 1)) as usize],
        }
    }

    fn set_size(ds: &mut DrawState, _params: &Params, tilesize: i32) {
        ds.tilesize = tilesize;
    }

    // C: mosaic.c, game_redraw()
    fn redraw(
        dr: &mut Drawing<'_>,
        ds: &mut DrawState,
        _old: Option<&State>,
        state: &State,
        _dir: i32,
        ui: &Ui,
        _anim_time: f32,
        flash_time: f32,
    ) {
        let grid = state.grid;
        // Black and white swap in the first and last thirds.
        let flashing = flash_time > 0.0
            && (flash_time <= FLASH_TIME / 3.0 || flash_time > 2.0 * FLASH_TIME / 3.0);
        for y in 0..=grid.h {
            for x in 0..=grid.w {
                let square = grid.index(x, y);
                let mut tile = square.map_or(0, |i| i32::from(state.cells[i]));
                if x == grid.w {
                    tile |= DRAW_MARGIN_R;
                }
                if y == grid.h {
                    tile |= DRAW_MARGIN_D;
                }
                if flashing {
                    tile ^= i32::from(MARKS);
                }
                if ui.cur_visible {
                    for (cx, cy, flag) in [
                        (x, y, DRAW_CURSOR),
                        (x - 1, y, DRAW_CURSOR_L),
                        (x, y - 1, DRAW_CURSOR_U),
                        (x - 1, y - 1, DRAW_CURSOR_UL),
                    ] {
                        if (ui.cur_x, ui.cur_y) == (cx, cy) {
                            tile |= flag;
                        }
                    }
                }
                let clue = square.and_then(|i| state.clues[i]);
                let t = (y * ds.stride + x) as usize;
                if ds.tiles[t] != tile {
                    draw_tile(dr, tile, ds.tilesize, clue, x, y);
                    ds.tiles[t] = tile;
                }
            }
        }
        let status = match (state.not_completed_clues, state.cheating) {
            (0, false) => "COMPLETED!".to_owned(),
            (0, true) => "Auto solved".to_owned(),
            (left, _) => format!("Clues left: {left}"),
        };
        dr.status_bar(&status);
    }

    fn flash_length(old: &State, new: &State, _dir: i32, _ui: &mut Ui) -> f32 {
        if !old.cheating && old.not_completed_clues > 0 && new.not_completed_clues == 0 {
            FLASH_TIME
        } else {
            0.0
        }
    }

    fn cursor_location(ui: &Ui, ds: &DrawState, _state: &State, _params: &Params) -> Option<Rect> {
        ui.cur_visible.then(|| Rect {
            x: ds.coord(ui.cur_x),
            y: ds.coord(ui.cur_y),
            w: ds.tilesize,
            h: ds.tilesize,
        })
    }
}

/// The select key, if `button` is exactly one (C's `IS_CURSOR_SELECT`).
fn cursor_select(button: Button) -> Option<CursorKey> {
    match button.kind {
        ButtonKind::Cursor(key @ (CursorKey::Select | CursorKey::Select2))
            if button.mods.is_empty() =>
        {
            Some(key)
        }
        _ => None,
    }
}

/// The squares a drag from `(from_x, from_y)` to `(x, y)` paints, from
/// `(x, y)` on, `(from_x, from_y)` excluded: along the column if
/// `from_x == x`, else along the row (whatever `from_y` is). C's int
/// arithmetic wraps, so a far-off source paints nothing.
fn line(x: i32, y: i32, from_x: i32, from_y: i32) -> impl Iterator<Item = (i32, i32)> {
    let (dx, dy, len) = if from_x == x && from_y != y {
        let diff = from_y.wrapping_sub(y);
        (0, if diff < 0 { -1 } else { 1 }, diff.wrapping_abs())
    } else {
        let diff = from_x.wrapping_sub(x);
        (if diff < 0 { -1 } else { 1 }, 0, diff.wrapping_abs())
    };
    (0..len).map(move |i| (x.wrapping_add(dx * i), y.wrapping_add(dy * i)))
}

/// C's `execute_move` reading its `N` fields: each with `atoi`, then past
/// its digits, with a comma between fields. So every field but the last
/// is bare digits (none reads as 0), and the last is anything `atoi`
/// takes, followed by anything.
fn read_fields<const N: usize>(mut s: &[u8]) -> Result<[i32; N], MoveParseError> {
    let mut fields = [0; N];
    for (i, field) in fields.iter_mut().enumerate() {
        *field = atoi(s);
        s = skip_digits(s);
        if i + 1 < N {
            s = s.strip_prefix(b",").ok_or(MoveParseError)?;
        }
    }
    Ok(fields)
}

/// C's reading of a solve move's hex digits (after the `s`), as far as
/// the `size` squares go. C takes the digits in pairs, the second of an
/// odd last pair being the string's NUL, and reads anything but `0`–`9`
/// as a letter from `a` (so other characters give out-of-range values
/// whose low bits still count).
fn read_solution(hex: &[u8], size: usize) -> Vec<bool> {
    // As C's `char`, signed.
    let value = |c: u8| -> u32 {
        let c = i32::from(c as i8);
        let v = if (i32::from(b'0')..=i32::from(b'9')).contains(&c) {
            c - i32::from(b'0')
        } else {
            c - i32::from(b'a') + 10
        };
        v as u32
    };
    let mut marked = Vec::with_capacity(hex.len() * 4);
    for pair in hex.chunks(2) {
        let low = pair.get(1).map_or(value(0), |&c| value(c));
        let byte = (value(pair[0]) << 4 | low) as u8;
        marked.extend((0..8).map(|k| byte & (0x80 >> k) != 0));
    }
    marked.truncate(size);
    marked
}

/// A square's clue as the solver sees it (C's `desc_cell`, or a
/// `board_cell` with `full` and `empty` false).
#[derive(Clone, Copy, Debug, Default)]
struct Clue {
    /// How many of the block's squares are black; -1 if unknown.
    value: i8,
    shown: bool,
    /// Every square of the block is black (generation only).
    full: bool,
    /// No square of the block is black (generation only).
    empty: bool,
}

impl Clue {
    /// A clue of a game in play.
    fn shown(clue: Option<u8>) -> Self {
        Self {
            value: clue.map_or(-1, |c| c as i8),
            shown: clue.is_some(),
            full: false,
            empty: false,
        }
    }

    /// The clue at `(x, y)` of a picture (C's `populate_cell`).
    fn from_image(grid: Grid, image: &[bool], x: i32, y: i32) -> Self {
        let value = grid.block(x, y).filter(|&i| image[i]).count() as i8;
        // A full block on an edge or in a corner has fewer squares.
        let edge = x * y == 0 || y == grid.h - 1 || x == grid.w - 1;
        let x_edge = x == 0 || x == grid.w - 1;
        let y_edge = y == 0 || y == grid.h - 1;
        let full = value == 9
            || (edge && ((x_edge && y_edge && value == 4) || ((x_edge || y_edge) && value == 6)));
        Self {
            value,
            shown: true,
            full: value != 0 && full,
            empty: value == 0,
        }
    }
}

/// The solver's view of a square (C's `solution_cell`).
#[derive(Clone, Copy, Debug, Default)]
struct SolutionCell {
    /// 0 (unknown), [`MARKED`] or [`BLANK`].
    mark: i8,
    /// Done with: its clue is used up, or its block is all known.
    solved: bool,
    /// Its clue decided some square when it was used.
    needed: bool,
}

/// What looking at one square did (C's `solve_cell` result).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Progress {
    /// Nothing (0).
    Stuck,
    /// The square is done with (1).
    Solved,
    /// Its clue contradicts its complete block (-1).
    Contradiction,
}

/// C's `solve_cell`: uses the clue at `(x, y)` once it decides its
/// block.
fn solve_cell(grid: Grid, clues: &[Clue], sol: &mut [SolutionCell], x: i32, y: i32) -> Progress {
    let i = (y * grid.w + x) as usize;
    let clue = clues[i];
    if sol[i].solved {
        return Progress::Stuck;
    }
    let c = grid.count(x, y, |j| sol[j].mark);
    let known = c.marked + c.blank;
    let mut settle = |mark: i8| {
        sol[i].solved = true;
        if known < c.total {
            sol[i].needed = true;
        }
        for j in grid.block(x, y) {
            if sol[j].mark == 0 {
                sol[j].mark = mark;
            }
        }
        Progress::Solved
    };
    if clue.shown {
        let value = i32::from(clue.value);
        if clue.full {
            settle(MARKED)
        } else if clue.empty || c.marked == value {
            settle(BLANK)
        } else if value == c.total - c.blank {
            settle(MARKED)
        } else if c.total == known {
            Progress::Contradiction
        } else {
            Progress::Stuck
        }
    } else if c.total == known {
        sol[i].solved = true;
        Progress::Solved
    } else {
        Progress::Stuck
    }
}

/// C's `solve_check`: uses the shown clues over and over (in a random
/// order if `rng` is given) until all are used or none helps. Returns
/// whether every square got known, and the solver's grid.
fn solve_with_clues(
    grid: Grid,
    clues: &[Clue],
    rng: Option<&mut RandomState>,
) -> (bool, Vec<SolutionCell>) {
    let mut sol = vec![SolutionCell::default(); grid.len()];
    // C lists the clues last first (prepending to a linked list).
    let mut order: Vec<(i32, i32)> = grid
        .squares()
        .filter(|&(i, _, _)| clues[i].shown)
        .map(|(_, x, y)| (x, y))
        .collect();
    order.reverse();
    if let Some(rng) = rng {
        rng.shuffle(&mut order);
    }

    let mut solved = 0;
    let mut progress = true;
    let mut error = false;
    while solved < order.len() && progress && !error {
        progress = false;
        for &(x, y) in &order {
            match solve_cell(grid, clues, &mut sol, x, y) {
                Progress::Contradiction => {
                    error = true;
                    break;
                }
                Progress::Solved => {
                    solved += 1;
                    progress = true;
                }
                Progress::Stuck => {}
            }
        }
    }
    // C checks the grid only if the last pass helped.
    let complete = progress && sol.iter().all(|s| s.mark & (MARKED | BLANK) != 0);
    (complete, sol)
}

/// C's `solve_game_actual`: looks at every square in reading order, over
/// and over, until all are done with or none changes. Returns whether
/// all were done with, and the solver's grid.
fn solve_by_rows(grid: Grid, clues: &[Clue]) -> (bool, Vec<SolutionCell>) {
    let size = grid.len();
    let mut sol = vec![SolutionCell::default(); size];
    let mut solved = 0;
    let mut progress = true;
    let mut error = false;
    while solved < size && progress && !error {
        progress = false;
        for y in 0..grid.h {
            for x in 0..grid.w {
                match solve_cell(grid, clues, &mut sol, x, y) {
                    Progress::Contradiction => {
                        // C's break leaves this row only.
                        error = true;
                        break;
                    }
                    Progress::Solved => {
                        progress = true;
                        solved += 1;
                    }
                    Progress::Stuck => {}
                }
            }
        }
    }
    (solved == size, sol)
}

/// C's `hide_clues`: hides the clues a shuffled solve didn't need, then
/// (if aggressive) tries hiding each needed one in random order, keeping
/// those the puzzle can do without.
fn hide_clues(grid: Grid, aggressive: bool, clues: &mut [Clue], rng: &mut RandomState) {
    let (_, sol) = solve_with_clues(grid, clues, Some(rng));
    let mut needed = Vec::new();
    for (i, _, _) in grid.squares() {
        if !sol[i].needed {
            clues[i].shown = false;
        } else if aggressive {
            needed.push(i);
        }
    }
    if aggressive {
        // C's linked list again: last first.
        needed.reverse();
        rng.shuffle(&mut needed);
        for i in needed {
            clues[i].shown = false;
            if !solve_with_clues(grid, clues, None).0 {
                clues[i].shown = true;
            }
        }
    }
}

/// The description: a digit per shown clue, and a letter per run of up to
/// 26 hidden ones (`a` for one).
fn encode_desc(clues: &[Clue]) -> String {
    let mut desc = String::new();
    let mut run = 0_u8;
    for clue in clues {
        if clue.shown {
            if run > 0 {
                desc.push(char::from(b'a' + run - 1));
                run = 0;
            }
            // Clues are 0 to 9.
            desc.push(char::from(b'0' + clue.value as u8));
        } else {
            if run == 26 {
                desc.push('z');
                run = 0;
            }
            run += 1;
        }
    }
    if run > 0 {
        desc.push(char::from(b'a' + run - 1));
    }
    desc
}

/// C's `draw_cell`: a square with its top and left grid lines, or just
/// the line of a margin tile.
fn draw_tile(dr: &mut Drawing<'_>, tile: i32, ts: i32, clue: Option<u8>, x: i32, y: i32) {
    let has = |bits: i8| tile & i32::from(bits) != 0;
    let (sx, sy) = (x * ts + ts / 2 - 1, y * ts + ts / 2 - 1);
    let line_colour = |flags: i32| {
        if tile & flags != 0 {
            Col::Cursor
        } else {
            Col::Grid
        }
    };

    dr.clip(sx - 1, sy - 1, ts, ts);
    if tile & DRAW_MARGIN_R == 0 {
        dr.rect(
            sx - 1,
            sy - 1,
            ts,
            1,
            line_colour(DRAW_CURSOR | DRAW_CURSOR_U),
        );
    }
    if tile & DRAW_MARGIN_D == 0 {
        dr.rect(
            sx - 1,
            sy - 1,
            1,
            ts,
            line_colour(DRAW_CURSOR | DRAW_CURSOR_L),
        );
    }
    if tile & DRAW_CURSOR_UL != 0 {
        dr.rect(sx - 1, sy - 1, 1, 1, Col::Cursor);
    }

    if tile & (DRAW_MARGIN_R | DRAW_MARGIN_D) == 0 {
        let (fill, mut text) = if has(MARKED) {
            (Col::Marked, TEXT_LIGHT)
        } else if has(BLANK) {
            (Col::Blank, TEXT_DARK)
        } else {
            (Col::Unmarked, TEXT_DARK)
        };
        if has(ERROR) {
            text = Col::Error;
        } else if has(SOLVED) {
            text = Col::TextSolved;
        }
        dr.rect(sx, sy, ts - 1, ts - 1, fill);
        if let Some(clue) = clue {
            dr.text(
                sx + ts / 2,
                sy + ts / 2,
                FontType::Variable,
                ts * 3 / 5,
                Align::CENTRE,
                text,
                &clue.to_string(),
            );
        }
    }

    dr.unclip();
    dr.update(sx - 1, sy - 1, ts, ts);
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn params(id: &str) -> Params {
        let mut p = Mosaic::default_params();
        Mosaic::decode_params(&mut p, id);
        p
    }

    fn start(id: &str, seed: &[u8]) -> State {
        let p = params(id);
        let desc = Mosaic::new_desc(&p, &mut RandomState::new(seed), true).desc;
        Mosaic::new_game(&mut GameCtx::new(), &p, &desc)
    }

    fn encode(mv: &Move) -> String {
        let mut s = String::new();
        Mosaic::encode_move(mv, &mut s);
        s
    }

    fn play(state: &State, mv: &Move) -> State {
        Mosaic::execute_move(&mut GameCtx::new(), state, mv).expect("a legal move")
    }

    #[test]
    fn params_parse_like_c() {
        assert_eq!(
            params("4x6h0"),
            Params {
                w: 4,
                h: 6,
                aggressive: false
            }
        );
        assert_eq!(
            params("7"),
            Params {
                w: 7,
                h: 7,
                ..Mosaic::default_params()
            }
        );
        // Upstream's icon: `a0` stops the parse, so `h1` is never read.
        assert_eq!(params("8x8a0h0"), params("8x8"));
        assert_eq!(Mosaic::encode_params(&params("5x5h0"), true), "5x5h0");
        assert_eq!(Mosaic::encode_params(&params("5x5h0"), false), "5x5");
        assert_eq!(Mosaic::encode_params(&params("5x5h7"), true), "5x5");
        assert!(Mosaic::validate_params(&params("2x5"), true).is_err());
        assert!(Mosaic::validate_params(&params("100x100"), true).is_ok());
        assert!(Mosaic::validate_params(&params("100x101"), true).is_err());
        assert!(Mosaic::validate_params(&params("-3x5"), true).is_err());
    }

    #[test]
    fn descriptions_encode_runs_of_hidden_clues() {
        let shown = |v: i8| Clue {
            value: v,
            shown: true,
            ..Clue::default()
        };
        let mut clues = vec![Clue::default(); 30];
        clues[1] = shown(3);
        clues.push(shown(0));
        assert_eq!(encode_desc(&clues), "a3zb0");
        let p = Params {
            w: 31,
            h: 1,
            aggressive: true,
        };
        // Not a valid size, but the codec doesn't mind.
        let state = Mosaic::new_game(&mut GameCtx::new(), &p, "a3zb0");
        assert_eq!(state.clues[1], Some(3));
        assert_eq!(state.clues[30], Some(0));
        assert_eq!(state.not_completed_clues, 2);
    }

    #[test]
    fn descriptions_validate_like_c() {
        let p = params("3x3");
        assert_eq!(Mosaic::validate_desc(&p, "1a2b3c"), Ok(()));
        assert_eq!(Mosaic::validate_desc(&p, "i"), Ok(()));
        assert_eq!(
            Mosaic::validate_desc(&p, "h"),
            Err(GameError::new("Desc size mismatch"))
        );
        assert_eq!(
            Mosaic::validate_desc(&p, "1 2"),
            Err(GameError::new("Invalid character in game description"))
        );
    }

    #[test]
    fn solving_generated_puzzles_satisfies_every_clue() {
        for id in ["3x3", "5x5", "10x10", "7x4h0"] {
            for seed in 0..5_u8 {
                let state = start(id, &[seed]);
                let Ok(Move::Solve(marked)) = Mosaic::solve(&state, &state, None) else {
                    panic!("{id}: no solution");
                };
                // Mark the solution square by square, as a player would.
                let mut s = state.clone();
                for (i, &m) in marked.iter().enumerate() {
                    let (x, y) = (i as i32 % s.grid.w, i as i32 / s.grid.w);
                    s = play(&s, &Move::Toggle { x, y, twice: !m });
                }
                assert_eq!(Mosaic::status(&s), GameStatus::Won, "{id} {seed}");
                let mut ui = Mosaic::new_ui(None);
                assert_eq!(Mosaic::flash_length(&state, &s, 1, &mut ui), FLASH_TIME);
                let solved = play(&state, &Move::Solve(marked));
                assert!(solved.cheating);
                assert_eq!(Mosaic::status(&solved), GameStatus::Won);
            }
        }
    }

    #[test]
    fn toggles_cycle_and_check_clues() {
        // 3x3, a single 9 in the middle.
        let p = params("3x3");
        let state = Mosaic::new_game(&mut GameCtx::new(), &p, "d9d");
        let mut s = state.clone();
        for i in 0..9 {
            s = play(
                &s,
                &Move::Toggle {
                    x: i % 3,
                    y: i / 3,
                    twice: false,
                },
            );
        }
        assert!(s.cells.iter().all(|&c| c & MARKS == MARKED));
        assert_eq!(s.cells[4], MARKED | SOLVED);
        assert_eq!(Mosaic::status(&s), GameStatus::Won);
        // Whitening one square makes the clue impossible.
        let s = play(
            &state,
            &Move::Toggle {
                x: 0,
                y: 0,
                twice: true,
            },
        );
        assert_eq!(s.cells[0], BLANK);
        assert_eq!(s.cells[4], ERROR);
        // -1 is a no-op, out of range is illegal.
        let noop = Move::Toggle {
            x: -1,
            y: 0,
            twice: false,
        };
        assert_eq!(play(&s, &noop), s);
        let off = Move::Toggle {
            x: 3,
            y: 0,
            twice: false,
        };
        assert_eq!(Mosaic::execute_move(&mut GameCtx::new(), &s, &off), None);
    }

    #[test]
    fn drags_paint_undecided_squares_towards_the_start() {
        let p = params("4x3");
        let state = Mosaic::new_game(&mut GameCtx::new(), &p, "l");
        let s = play(
            &state,
            &Move::Toggle {
                x: 3,
                y: 1,
                twice: false,
            },
        );
        let s = play(
            &s,
            &Move::Drag {
                x: 0,
                y: 1,
                from_x: 3,
                from_y: 1,
                state: i32::from(BLANK),
                release: false,
            },
        );
        assert_eq!(&s.cells[4..8], &[BLANK, BLANK, BLANK, MARKED]);
        // Leaving the grid makes it illegal.
        let off = Move::Drag {
            x: 0,
            y: 2,
            from_x: 0,
            from_y: 5,
            state: 1,
            release: true,
        };
        assert_eq!(Mosaic::execute_move(&mut GameCtx::new(), &s, &off), None);
    }

    #[test]
    fn moves_decode_like_c() {
        let state = start("5x5", b"codec");
        let ok = |text: &str| Mosaic::decode_move(&state, text);
        assert_eq!(
            ok("t1,2"),
            Ok(Move::Toggle {
                x: 1,
                y: 2,
                twice: false
            })
        );
        // The first field is bare digits (none is 0); the last is atoi's.
        assert_eq!(
            ok("T,  -7junk"),
            Ok(Move::Toggle {
                x: 0,
                y: -7,
                twice: true
            })
        );
        assert_eq!(
            ok("e1,2,3,4,-9"),
            Ok(Move::Drag {
                x: 1,
                y: 2,
                from_x: 3,
                from_y: 4,
                state: -9,
                release: true
            })
        );
        for bad in [
            "t",
            "t1",
            "t 1,2",
            "t+1,2",
            "t-1,2",
            "d1,2,3,4",
            "d1,2,3 ,4,5",
        ] {
            assert_eq!(ok(bad), Err(MoveParseError), "{bad:?}");
        }
        for other in ["", "x", "S00", "\0t1,2"] {
            assert_eq!(ok(other), Ok(Move::Recount), "{other:?}");
        }
        // An overflowing field wraps as C's atoi does, and is written back
        // so that it reads the same.
        let wrapped = ok("t4294967295,3").expect("parses");
        assert_eq!(
            wrapped,
            Move::Toggle {
                x: -1,
                y: 3,
                twice: false
            }
        );
        assert_eq!(encode(&wrapped), "t4294967295,3");
    }

    #[test]
    fn solve_moves_read_like_c() {
        let state = start("3x3", b"solve");
        let bits = |byte: u8| (0..8).map(move |k| byte & (0x80 >> k) != 0);
        let read = |text: &str| match Mosaic::decode_move(&state, text) {
            Ok(Move::Solve(m)) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(read("sa580"), bits(0xa5).chain([true]).collect::<Vec<_>>());
        // Extra digits are ignored; missing ones leave squares alone.
        assert_eq!(read("sa580ff"), read("sa580"));
        assert_eq!(read("s"), Vec::<bool>::new());
        // An odd digit pairs with the NUL ('\0' - 'a' + 10 = -87, 0xa9).
        assert_eq!(read("s1"), bits(0x10 | 0xa9).collect::<Vec<_>>());
        // Uppercase isn't hex to C: 'A' - 'a' + 10 = -22.
        assert_eq!(read("sA0"), bits((-22_i32 << 4) as u8).collect::<Vec<_>>());
        assert_eq!(encode(&Move::Solve(read("sa580ff"))), "sa580");
    }

    #[test]
    fn text_format_shows_clues() {
        let p = params("3x3");
        let state = Mosaic::new_game(&mut GameCtx::new(), &p, "1c9b0a");
        assert_eq!(
            Mosaic::text_format(&state).as_deref(),
            Some("|1|| || |\n| ||9|| |\n| ||0|| |\n")
        );
    }

    fn any_move() -> impl Strategy<Value = Move> {
        let field = prop_oneof![0..12_i32, any::<i32>()];
        prop_oneof![
            (field.clone(), any::<i32>(), any::<bool>()).prop_map(|(x, y, twice)| Move::Toggle {
                x,
                y,
                twice
            }),
            (
                field.clone(),
                field.clone(),
                field.clone(),
                field,
                any::<i32>(),
                any::<bool>()
            )
                .prop_map(|(x, y, from_x, from_y, state, release)| Move::Drag {
                    x,
                    y,
                    from_x,
                    from_y,
                    state,
                    release
                }),
            prop::collection::vec(any::<bool>(), 0..=25).prop_map(|mut v| {
                // Whole bytes, or the full grid.
                if v.len() != 25 {
                    v.truncate(v.len() / 8 * 8);
                }
                Move::Solve(v)
            }),
            Just(Move::Recount),
        ]
    }

    proptest! {
        #[test]
        fn move_codec_round_trips(mv in any_move()) {
            let state = start("5x5", b"codec");
            prop_assert_eq!(Mosaic::decode_move(&state, &encode(&mv)), Ok(mv));
        }

        /// Untrusted text never panics.
        #[test]
        fn untrusted_text_never_panics(
            id in "\\PC{0,12}",
            desc in "[0-9a-z ]{0,40}",
            mv in "[tTdesx0-9, +-]{0,16}|\\PC{0,16}",
        ) {
            let mut p = Mosaic::default_params();
            Mosaic::decode_params(&mut p, &id);
            let p = if Mosaic::validate_params(&p, true).is_ok() && p.w * p.h <= 64 {
                p
            } else {
                params("3x4")
            };
            if Mosaic::validate_desc(&p, &desc).is_ok() {
                let state = Mosaic::new_game(&mut GameCtx::new(), &p, &desc);
                if let Ok(mv) = Mosaic::decode_move(&state, &mv)
                    && let Some(next) = Mosaic::execute_move(&mut GameCtx::new(), &state, &mv)
                {
                    let mut ds = Mosaic::new_drawstate(&next);
                    Mosaic::set_size(&mut ds, &p, 16);
                    let mut rec = puzzles_core::drawing::NullDrawing;
                    let mut dr = Drawing::new(&mut rec);
                    Mosaic::redraw(&mut dr, &mut ds, None, &next, 1, &Mosaic::new_ui(None), 0.0, 0.1);
                }
                let _ = Mosaic::solve(&state, &state, None);
            }
        }
    }
}
