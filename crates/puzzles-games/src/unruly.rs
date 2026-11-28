//! Port of third_party/puzzles-c/unruly.c (upstream 616da16).
//!
//! Unruly (also known as Binairo, Tohu-Wa-Vohu): fill the grid with
//! black and white squares so that no row or column has three of a
//! colour in a row and each has as many of one as of the other;
//! optionally, no two rows or two columns may be the same.
//!
//! The generator fills a grid at random, letting the solver place
//! whatever each choice forces, then empties squares in random order as
//! long as the solver at the chosen difficulty still finishes, and starts
//! over if the result is solvable one difficulty lower.
//!
//! In C, the solver's account of its reasoning exists only in the
//! `STANDALONE_SOLVER` build (`unrulysolver -v`); here [`solve`] and
//! [`generate`] write it to a [`SolverLog`], which the game itself keeps
//! quiet.
//!
//! Cells are `0`/`1` in move strings and text, where 1 is black.

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
use puzzles_core::parse::{atoi, scan_int, skip_digits};
use puzzles_core::preset::PresetMenu;
use puzzles_core::random::RandomState;
use puzzles_core::solver_log::SolverLog;

const PREFERRED_TILE_SIZE: i32 = 32;
/// Seconds per frame of the completion flash.
const FLASH_FRAME: f32 = 0.12;

// Tile bits, as C's: errors to mark, then what the square shows.
const FE_HOR_ROW_LEFT: i32 = 0x0001;
const FE_HOR_ROW_MID: i32 = 0x0003;
const FE_HOR_ROW_RIGHT: i32 = 0x0002;
const FE_VER_ROW_TOP: i32 = 0x0004;
const FE_VER_ROW_MID: i32 = 0x000C;
const FE_VER_ROW_BOTTOM: i32 = 0x0008;
/// Too many of this colour in the row or column.
const FE_COUNT: i32 = 0x0010;
/// The row is the same as another.
const FE_ROW_MATCH: i32 = 0x0020;
/// The column is the same as another.
const FE_COL_MATCH: i32 = 0x0040;
const FF_ONE: i32 = 0x0080;
const FF_ZERO: i32 = 0x0100;
const FF_CURSOR: i32 = 0x0200;
const FF_FLASH1: i32 = 0x0400;
const FF_FLASH2: i32 = 0x0800;
const FF_IMMUTABLE: i32 = 0x1000;

puzzles_core::palette! {
    /// Unruly's colours.
    pub enum Col {
        Background => Role(Background),
        Grid => Role(Grid),
        /// An empty square.
        Empty => Specific("empty"),
        /// A white square (0).
        Zero => Series(Binary, 0),
        /// A white clue's bevel, and the first frame of the flash.
        ZeroHighlight => Derived(Zero, Highlight),
        /// A white clue's bevel, and the second frame of the flash.
        ZeroLowlight => Derived(Zero, Lowlight),
        /// A black square (1).
        One => Series(Binary, 1),
        /// A black clue's bevel, and the first frame of the flash.
        OneHighlight => Derived(One, Highlight),
        /// A black clue's bevel, and the second frame of the flash.
        OneLowlight => Derived(One, Lowlight),
        Cursor => Role(Cursor),
        Error => Role(Error),
    }
}

impl Col {
    /// The bevel highlight of a square colour (C's `COL_n + 1`).
    fn highlight(self) -> Self {
        if self == Self::One {
            Self::OneHighlight
        } else {
            Self::ZeroHighlight
        }
    }

    /// The bevel lowlight of a square colour (C's `COL_n + 2`).
    fn lowlight(self) -> Self {
        if self == Self::One {
            Self::OneLowlight
        } else {
            Self::ZeroLowlight
        }
    }
}

/// How hard a puzzle is: the solver techniques it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Difficulty {
    /// Three in a row, and a row with one square left.
    Trivial,
    /// A row with all of one colour, and (with unique rows) a row about
    /// to copy another.
    Easy,
    /// A row with one of a colour left that can't go next to a pair.
    Normal,
}

impl Difficulty {
    const ALL: [Self; 3] = [Self::Trivial, Self::Easy, Self::Normal];

    /// The name in menus and the solver's report.
    #[must_use]
    pub fn name(self) -> &'static str {
        ["Trivial", "Easy", "Normal"][self as usize]
    }

    /// The letter in parameter strings.
    fn letter(self) -> char {
        ['t', 'e', 'n'][self as usize]
    }

    /// The next easier one.
    fn easier(self) -> Option<Self> {
        (self as usize).checked_sub(1).map(|i| Self::ALL[i])
    }
}

/// Unruly's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Width in squares (even).
    pub w2: i32,
    /// Height in squares (even).
    pub h2: i32,
    /// No two rows, and no two columns, may be the same.
    pub unique: bool,
    /// `None` for an unknown difficulty letter in a game ID, which
    /// [`Game::validate_params`] rejects (C's `DIFFCOUNT + 1`).
    pub diff: Option<Difficulty>,
}

/// What a square holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Cell {
    Empty,
    /// Black (C's `N_ONE`).
    One,
    /// White (C's `N_ZERO`).
    Zero,
    /// Held out of the way by the solver for a moment (C's `BOGUS`).
    Bogus,
}

impl Cell {
    /// The digit for a placed colour (C prints anything but 1 as 0).
    fn digit(self) -> char {
        if self == Self::One { '1' } else { '0' }
    }

    /// The other colour.
    fn other(self) -> Self {
        if self == Self::One {
            Self::Zero
        } else {
            Self::One
        }
    }
}

/// A position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    w2: i32,
    h2: i32,
    unique: bool,
    grid: Vec<Cell>,
    /// The clues, which can't be changed. Shared by every state of a game.
    immutable: Rc<[bool]>,
    completed: bool,
    cheated: bool,
}

impl State {
    fn blank(w2: i32, h2: i32, unique: bool) -> Self {
        let s = (w2 * h2) as usize;
        Self {
            w2,
            h2,
            unique,
            grid: vec![Cell::Empty; s],
            immutable: vec![false; s].into(),
            completed: false,
            cheated: false,
        }
    }

    fn index(&self, x: i32, y: i32) -> usize {
        (y * self.w2 + x) as usize
    }

    /// The colours' counts per row and column (C's `unruly_scratch`).
    fn counts(&self) -> Counts {
        Counts::of(self)
    }
}

/// How many of each colour each row and column has (C's
/// `unruly_scratch`).
#[derive(Clone, Debug)]
struct Counts {
    ones_rows: Vec<i32>,
    ones_cols: Vec<i32>,
    zeros_rows: Vec<i32>,
    zeros_cols: Vec<i32>,
}

impl Counts {
    /// C's `unruly_new_scratch`.
    fn of(state: &State) -> Self {
        let (w2, h2) = (state.w2 as usize, state.h2 as usize);
        let mut counts = Self {
            ones_rows: vec![0; h2],
            ones_cols: vec![0; w2],
            zeros_rows: vec![0; h2],
            zeros_cols: vec![0; w2],
        };
        for (i, &c) in state.grid.iter().enumerate() {
            counts.add(c, i % w2, i / w2);
        }
        counts
    }

    /// Counts a square of colour `c` at `(x, y)`.
    fn add(&mut self, c: Cell, x: usize, y: usize) {
        match c {
            Cell::One => {
                self.ones_rows[y] += 1;
                self.ones_cols[x] += 1;
            }
            Cell::Zero => {
                self.zeros_rows[y] += 1;
                self.zeros_cols[x] += 1;
            }
            Cell::Empty | Cell::Bogus => {}
        }
    }

    /// Colour `c`'s counts per row (`horizontal`) or column.
    fn lines(&self, c: Cell, horizontal: bool) -> &[i32] {
        match (c, horizontal) {
            (Cell::One, true) => &self.ones_rows,
            (Cell::One, false) => &self.ones_cols,
            (_, true) => &self.zeros_rows,
            (_, false) => &self.zeros_cols,
        }
    }
}

/// Whether the rows and columns have the right number of each colour
/// (C's `unruly_validate_counts` result).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Balance {
    /// Every row and column is full and balanced (0).
    Exact,
    /// Some line is short of a colour, none has too many (1).
    Short,
    /// Some line has too many of a colour (-1).
    Excess,
}

/// A move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Move {
    /// Set a square: `P<c>,<x>,<y>` with `c` `1` (black), `0` (white) or
    /// `-` (empty).
    Place {
        /// `Some(true)` black, `Some(false)` white, `None` empty.
        colour: Option<bool>,
        /// Column.
        x: i32,
        /// Row.
        y: i32,
    },
    /// The solution: `S` and a `0` or `1` per square.
    Solve(Vec<bool>),
}

/// The keyboard cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ui {
    cx: i32,
    cy: i32,
    cursor: bool,
}

/// What was last drawn.
#[derive(Clone, Debug)]
pub struct DrawState {
    tilesize: i32,
    started: bool,
    /// Per square, the tile bits last drawn, or `None` if never drawn.
    tiles: Vec<Option<i32>>,
}

impl DrawState {
    /// C's `COORD`.
    fn coord(&self, x: i32) -> i32 {
        x * self.tilesize + self.tilesize / 2
    }

    /// C's `FROMCOORD`.
    fn tile_at(&self, x: i32) -> i32 {
        (x - self.tilesize / 2) / self.tilesize
    }
}

/// The Unruly puzzle.
#[derive(Clone, Copy, Debug)]
pub struct Unruly;

/// C's `unruly_presets`.
const PRESETS: [(i32, Difficulty); 7] = [
    (8, Difficulty::Trivial),
    (8, Difficulty::Easy),
    (8, Difficulty::Normal),
    (10, Difficulty::Easy),
    (10, Difficulty::Normal),
    (14, Difficulty::Easy),
    (14, Difficulty::Normal),
];

/// The number of different valid rows of length `2n`, for `n` from 0
/// while it fits in an `i32` (OEIS A177790): with unique rows, a puzzle
/// `2n` wide can be at most this tall, and vice versa.
const A177790: [i64; 24] = [
    1,
    2,
    6,
    14,
    34,
    84,
    208,
    518,
    1_296,
    3_254,
    8_196,
    20_700,
    52_404,
    132_942,
    337_878,
    860_142,
    2_192_902,
    5_598_144,
    14_308_378,
    36_610_970,
    93_770_358,
    240_390_602,
    616_787_116,
    1_583_765_724,
];

impl Game for Unruly {
    type Params = Params;
    type State = State;
    type Ui = Ui;
    type DrawState = DrawState;
    type Move = Move;

    const NAME: &'static str = "Unruly";
    const PREFERRED_TILESIZE: i32 = PREFERRED_TILE_SIZE;
    const PALETTE: &'static [PaletteEntry] = Col::PALETTE;
    const CAN_CONFIGURE: bool = true;
    const CAN_SOLVE: bool = true;
    const CAN_FORMAT_AS_TEXT_EVER: bool = true;

    fn default_params() -> Params {
        let (size, diff) = PRESETS[0];
        Params {
            w2: size,
            h2: size,
            unique: false,
            diff: Some(diff),
        }
    }

    fn presets() -> PresetMenu<Params> {
        let mut menu = PresetMenu::new();
        for (size, diff) in PRESETS {
            menu.add_preset(
                format!("{size}x{size} {}", diff.name()),
                Params {
                    w2: size,
                    h2: size,
                    unique: false,
                    diff: Some(diff),
                },
            );
        }
        menu
    }

    fn decode_params(params: &mut Params, s: &str) {
        let mut p = s.as_bytes();
        params.unique = false;
        params.w2 = atoi(p);
        p = skip_digits(p);
        if let [b'x', rest @ ..] = p {
            params.h2 = atoi(rest);
            p = skip_digits(rest);
        } else {
            params.h2 = params.w2;
        }
        if let [b'u', rest @ ..] = p {
            params.unique = true;
            p = rest;
        }
        if let [b'd', rest @ ..] = p {
            params.diff = rest.first().and_then(|&c| {
                Difficulty::ALL
                    .into_iter()
                    .find(|d| d.letter() == char::from(c))
            });
        }
    }

    fn encode_params(params: &Params, full: bool) -> String {
        let mut s = format!("{}x{}", params.w2, params.h2);
        if params.unique {
            s.push('u');
        }
        if full {
            // C reads past its letters for an unknown difficulty, which
            // only invalid parameters have; the port writes none.
            s.push('d');
            if let Some(diff) = params.diff {
                s.push(diff.letter());
            }
        }
        s
    }

    fn configure(params: &Params) -> Option<Vec<ConfigItem>> {
        const NAMES: &[&str] = &["Trivial", "Easy", "Normal"];
        Some(vec![
            ConfigItem::string("Width", params.w2.to_string()),
            ConfigItem::string("Height", params.h2.to_string()),
            ConfigItem::boolean("Unique rows and columns", params.unique),
            ConfigItem::choices(
                "Difficulty",
                NAMES,
                params.diff.map_or(NAMES.len(), |d| d as usize),
            ),
        ])
    }

    fn custom_params(config: &[ConfigItem]) -> Result<Params, GameError> {
        let wrong = || GameError::new("Unexpected configuration fields");
        let [w, h, unique, diff] = config else {
            return Err(wrong());
        };
        let number =
            |item: &ConfigItem| item.as_str().map(|s| atoi(s.as_bytes())).ok_or_else(wrong);
        Ok(Params {
            w2: number(w)?,
            h2: number(h)?,
            unique: unique.as_bool().ok_or_else(wrong)?,
            diff: Difficulty::ALL
                .get(diff.selected().ok_or_else(wrong)?)
                .copied(),
        })
    }

    fn validate_params(params: &Params, _full: bool) -> Result<(), GameError> {
        let (w2, h2) = (params.w2, params.h2);
        if (w2 & 1) != 0 || (h2 & 1) != 0 {
            return Err(GameError::new("Width and height must both be even"));
        }
        if w2 < 6 || h2 < 6 {
            return Err(GameError::new("Width and height must be at least 6"));
        }
        if w2 > i32::MAX / h2 {
            return Err(GameError::new(
                "Width times height must not be unreasonably large",
            ));
        }
        if params.unique {
            let limit = |n: i32| A177790.get((n / 2) as usize).copied();
            if limit(w2).is_some_and(|max| i64::from(h2) > max) {
                return Err(GameError::new("Puzzle is too tall for unique-rows mode"));
            }
            if limit(h2).is_some_and(|max| i64::from(w2) > max) {
                return Err(GameError::new("Puzzle is too long for unique-rows mode"));
            }
        }
        if params.diff.is_none() {
            return Err(GameError::new("Unknown difficulty rating"));
        }
        Ok(())
    }

    fn new_desc(params: &Params, rng: &mut RandomState, _interactive: bool) -> NewDesc {
        NewDesc::new(generate(params, rng, &mut SolverLog::quiet()))
    }

    fn validate_desc(params: &Params, desc: &str) -> Result<(), GameError> {
        let s = i64::from(params.w2) * i64::from(params.h2);
        let mut pos = 0_i64;
        for &c in desc.as_bytes() {
            pos += match c {
                b'a'..=b'y' => 1 + i64::from(c - b'a'),
                b'A'..=b'Y' => 1 + i64::from(c - b'A'),
                b'z' | b'Z' => 25,
                _ => return Err(GameError::new("Description contains invalid characters")),
            };
        }
        if pos < s + 1 {
            return Err(GameError::new("Description too short"));
        }
        if pos > s + 1 {
            return Err(GameError::new("Description too long"));
        }
        Ok(())
    }

    fn new_game(_ctx: &mut GameCtx, params: &Params, desc: &str) -> State {
        let mut state = State::blank(params.w2, params.h2, params.unique);
        let mut immutable = vec![false; state.grid.len()];
        let mut pos = 0_usize;
        for &c in desc.as_bytes() {
            let (run, cell) = match c {
                b'a'..=b'y' => (c - b'a', Cell::Zero),
                b'A'..=b'Y' => (c - b'A', Cell::One),
                // 'z', 'Z': 25 empty squares.
                _ => {
                    pos += 25;
                    continue;
                }
            };
            pos += usize::from(run);
            if pos < state.grid.len() {
                state.grid[pos] = cell;
                immutable[pos] = true;
            }
            pos += 1;
        }
        state.immutable = immutable.into();
        state
    }

    fn solve(original: &State, _current: &State, _aux: Option<&str>) -> Result<Move, GameError> {
        let solved = solve(original, &mut SolverLog::quiet());
        match solved.verdict {
            Verdict::Solved => Ok(Move::Solve(
                solved.state.grid.iter().map(|&c| c == Cell::One).collect(),
            )),
            Verdict::NoSolution => Err(GameError::new("No solution found.")),
            Verdict::Invalid => Err(GameError::new("Puzzle is invalid.")),
        }
    }

    fn text_format(state: &State) -> Option<String> {
        let mut s = String::with_capacity(state.grid.len() * 2 + state.h2 as usize);
        for row in state.grid.chunks_exact(state.w2 as usize) {
            for &c in row {
                s.push(match c {
                    Cell::One => '1',
                    Cell::Zero => '0',
                    Cell::Empty | Cell::Bogus => '.',
                });
                s.push(' ');
            }
            s.push('\n');
        }
        Some(s)
    }

    fn status(state: &State) -> GameStatus {
        if state.completed {
            GameStatus::Won
        } else {
            GameStatus::Ongoing
        }
    }

    fn new_ui(_state: Option<&State>) -> Ui {
        Ui {
            cx: 0,
            cy: 0,
            cursor: env_bool("PUZZLES_SHOW_CURSOR").unwrap_or(false),
        }
    }

    fn current_key_label(ui: &Ui, state: &State, button: Button) -> Option<&'static str> {
        // C compares the whole code, so a modified key is no select.
        let ButtonKind::Cursor(key @ (CursorKey::Select | CursorKey::Select2)) = button.kind else {
            return None;
        };
        if !ui.cursor || !button.mods.is_empty() {
            return None;
        }
        let i = state.index(ui.cx, ui.cy);
        if state.immutable[i] {
            return None;
        }
        let select = key == CursorKey::Select;
        match state.grid[i] {
            Cell::Empty => Some(if select { "Black" } else { "White" }),
            Cell::One => Some(if select { "White" } else { "Empty" }),
            Cell::Zero => Some(if select { "Empty" } else { "Black" }),
            Cell::Bogus => None,
        }
    }

    // C: unruly.c, interpret_move()
    fn interpret_move(
        state: &State,
        ui: &mut Ui,
        ds: &DrawState,
        ox: i32,
        oy: i32,
        button: Button,
    ) -> MoveResult<Move> {
        let (mut hx, mut hy) = (ui.cx, ui.cy);
        let (gx, gy) = (ds.tile_at(ox), ds.tile_at(oy));
        let mut nothing = MoveResult::NoEffect;
        // C strips every modifier.
        let kind = button.kind;

        let click = match kind {
            ButtonKind::Mouse(b, MouseAction::Press) => Some(b),
            _ => None,
        };
        if click.is_some() {
            let ts = ds.tilesize;
            if ox >= ts / 2 && gx < state.w2 && oy >= ts / 2 && gy < state.h2 {
                (hx, hy) = (gx, gy);
                if ui.cursor {
                    ui.cursor = false;
                    nothing = MoveResult::UiUpdate;
                }
            } else {
                return MoveResult::Unused;
            }
        }

        if matches!(
            kind,
            ButtonKind::Cursor(
                CursorKey::Up | CursorKey::Down | CursorKey::Left | CursorKey::Right
            )
        ) {
            return move_cursor(
                Button::new(kind),
                &mut ui.cx,
                &mut ui.cy,
                state.w2,
                state.h2,
                false,
                Some(&mut ui.cursor),
            );
        }

        let key = match kind {
            ButtonKind::Cursor(k @ (CursorKey::Select | CursorKey::Select2)) => {
                Some(Key::Select(k))
            }
            ButtonKind::Key(c @ ('\u{8}' | '0' | '1' | '2')) => Some(Key::Char(c)),
            _ => None,
        };
        if !(click.is_some() || (ui.cursor && key.is_some())) {
            return MoveResult::Unused;
        }

        let i = state.index(hx, hy);
        if state.immutable[i] {
            return nothing;
        }
        let current = state.grid[i];
        // C's character: '1', '0' or '-'.
        let colour = match (click, key) {
            (_, Some(Key::Char('0' | '2'))) => Some(false),
            (_, Some(Key::Char('1'))) => Some(true),
            (Some(MouseButton::Middle), _) => None,
            (Some(MouseButton::Right), _) | (_, Some(Key::Select(CursorKey::Select2))) => {
                match current {
                    Cell::Empty => Some(false),
                    Cell::Zero => Some(true),
                    _ => None,
                }
            }
            (Some(MouseButton::Left), _) | (_, Some(Key::Select(_))) => match current {
                Cell::Empty => Some(true),
                Cell::One => Some(false),
                _ => None,
            },
            // Backspace.
            _ => None,
        };
        if current == cell_of(colour) {
            // Don't put no-ops on the undo chain.
            return nothing;
        }
        MoveResult::Move(Move::Place {
            colour,
            x: hx,
            y: hy,
        })
    }

    // C: unruly.c, execute_move()
    fn execute_move(_ctx: &mut GameCtx, from: &State, mv: &Move) -> Option<State> {
        match *mv {
            Move::Solve(ref ones) => {
                if ones.len() != from.grid.len() {
                    return None;
                }
                let mut state = from.clone();
                for (c, &one) in state.grid.iter_mut().zip(ones) {
                    *c = if one { Cell::One } else { Cell::Zero };
                }
                state.completed = true;
                state.cheated = true;
                Some(state)
            }
            Move::Place { colour, x, y } => {
                if !(0..from.w2).contains(&x) || !(0..from.h2).contains(&y) {
                    return None;
                }
                let i = from.index(x, y);
                if from.immutable[i] {
                    return None;
                }
                let mut state = from.clone();
                state.grid[i] = cell_of(colour);
                if !state.completed
                    && balance(&state, &state.counts(), None) == Balance::Exact
                    && !has_row_errors(&state, None)
                {
                    state.completed = true;
                }
                Some(state)
            }
        }
    }

    fn encode_move(mv: &Move, out: &mut String) {
        match mv {
            Move::Place { colour, x, y } => {
                let c = match colour {
                    Some(true) => '1',
                    Some(false) => '0',
                    None => '-',
                };
                let _ = write!(out, "P{c},{x},{y}");
            }
            Move::Solve(ones) => {
                out.push('S');
                out.extend(ones.iter().map(|&one| if one { '1' } else { '0' }));
            }
        }
    }

    fn decode_move(state: &State, text: &str) -> Result<Move, MoveParseError> {
        // C reads up to the first NUL.
        let bytes = text.as_bytes();
        let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
        match bytes.split_first() {
            // `S` and a 0 or 1 per square; anything may follow.
            Some((b'S', rest)) => {
                let digits = rest.get(..state.grid.len()).ok_or(MoveParseError)?;
                digits
                    .iter()
                    .map(|&b| match b {
                        b'1' => Ok(true),
                        b'0' => Ok(false),
                        _ => Err(MoveParseError),
                    })
                    .collect::<Result<_, _>>()
                    .map(Move::Solve)
            }
            // sscanf("%c,%d,%d"): any character, then the numbers.
            Some((b'P', rest)) => {
                let (&c, rest) = rest.split_first().ok_or(MoveParseError)?;
                let rest = rest.strip_prefix(b",").ok_or(MoveParseError)?;
                let (x, rest) = scan_int(rest).ok_or(MoveParseError)?;
                let rest = rest.strip_prefix(b",").ok_or(MoveParseError)?;
                let (y, _) = scan_int(rest).ok_or(MoveParseError)?;
                let colour = match c {
                    b'1' => Some(true),
                    b'0' => Some(false),
                    b'-' => None,
                    _ => return Err(MoveParseError),
                };
                if !(0..state.w2).contains(&x) || !(0..state.h2).contains(&y) {
                    return Err(MoveParseError);
                }
                Ok(Move::Place { colour, x, y })
            }
            _ => Err(MoveParseError),
        }
    }

    fn compute_size(params: &Params, tilesize: i32, _ui: &Ui) -> (i32, i32) {
        (tilesize * (params.w2 + 1), tilesize * (params.h2 + 1))
    }

    fn new_drawstate(state: &State) -> DrawState {
        DrawState {
            tilesize: 0,
            started: false,
            tiles: vec![None; state.grid.len()],
        }
    }

    fn set_size(ds: &mut DrawState, _params: &Params, tilesize: i32) {
        ds.tilesize = tilesize;
    }

    // C: unruly.c, game_redraw()
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
        let (w2, h2) = (state.w2, state.h2);
        let ts = ds.tilesize;
        if !ds.started {
            // The grid's outer edge.
            dr.rect(
                ds.coord(0) - ts / 10,
                ds.coord(0) - ts / 10,
                ts * w2 + 2 * (ts / 10) - 1,
                ts * h2 + 2 * (ts / 10) - 1,
                Col::Grid,
            );
            dr.update(0, 0, ts * (w2 + 1), ts * (h2 + 1));
            ds.started = true;
        }

        let flash = if flash_time > 0.0 {
            if (flash_time / FLASH_FRAME) as i32 == 1 {
                FF_FLASH2
            } else {
                FF_FLASH1
            }
        } else {
            0
        };

        let mut errors = vec![0; state.grid.len()];
        has_row_errors(state, Some(&mut errors));
        let mut excess = vec![false; 2 * (w2 + h2) as usize];
        balance(state, &state.counts(), Some(&mut excess));
        let (w2u, h2u) = (w2 as usize, h2 as usize);

        for y in 0..h2 {
            for x in 0..w2 {
                let i = state.index(x, y);
                let (xu, yu) = (x as usize, y as usize);
                let mut tile = errors[i];
                match state.grid[i] {
                    Cell::One => {
                        tile |= FF_ONE;
                        if excess[yu] || excess[2 * h2u + xu] {
                            tile |= FE_COUNT;
                        }
                    }
                    Cell::Zero => {
                        tile |= FF_ZERO;
                        if excess[h2u + yu] || excess[2 * h2u + w2u + xu] {
                            tile |= FE_COUNT;
                        }
                    }
                    Cell::Empty | Cell::Bogus => {}
                }
                tile |= flash;
                if state.immutable[i] {
                    tile |= FF_IMMUTABLE;
                }
                if ui.cursor && (ui.cx, ui.cy) == (x, y) {
                    tile |= FF_CURSOR;
                }
                if ds.tiles[i] != Some(tile) {
                    ds.tiles[i] = Some(tile);
                    draw_tile(dr, ds.coord(x), ds.coord(y), ts, tile);
                }
            }
        }
    }

    fn flash_length(old: &State, new: &State, _dir: i32, _ui: &mut Ui) -> f32 {
        if !old.completed && new.completed && !old.cheated && !new.cheated {
            FLASH_FRAME * 3.0
        } else {
            0.0
        }
    }

    fn cursor_location(ui: &Ui, ds: &DrawState, _state: &State, _params: &Params) -> Option<Rect> {
        ui.cursor.then(|| Rect {
            x: ds.coord(ui.cx),
            y: ds.coord(ui.cy),
            w: ds.tilesize,
            h: ds.tilesize,
        })
    }
}

/// A key that places or clears a square.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Select(CursorKey),
    Char(char),
}

/// The square a move's colour gives.
fn cell_of(colour: Option<bool>) -> Cell {
    match colour {
        Some(true) => Cell::One,
        Some(false) => Cell::Zero,
        None => Cell::Empty,
    }
}

// ---------------------------------------------------------------------
// Checking

/// C's `unruly_validate_rows`: counts the runs of three `check`s along
/// rows (`horizontal`) or columns, marking them in `errors`.
fn three_in_a_row(
    state: &State,
    horizontal: bool,
    check: Cell,
    mut errors: Option<&mut [i32]>,
) -> i32 {
    let (dx, dy) = if horizontal { (1, 0) } else { (0, 1) };
    let (err1, err2, err3) = if horizontal {
        (FE_HOR_ROW_LEFT, FE_HOR_ROW_MID, FE_HOR_ROW_RIGHT)
    } else {
        (FE_VER_ROW_TOP, FE_VER_ROW_MID, FE_VER_ROW_BOTTOM)
    };
    let mut found = 0;
    for y in dy..state.h2 - dy {
        for x in dx..state.w2 - dx {
            let i1 = state.index(x - dx, y - dy);
            let i2 = state.index(x, y);
            let i3 = state.index(x + dx, y + dy);
            if [i1, i2, i3].iter().all(|&i| state.grid[i] == check) {
                found += 1;
                if let Some(errors) = errors.as_deref_mut() {
                    errors[i1] |= err1;
                    errors[i2] |= err2;
                    errors[i3] |= err3;
                }
            }
        }
    }
    found
}

/// C's `unruly_validate_unique`: counts the pairs of full rows
/// (`horizontal`) or columns that are the same, marking them in
/// `errors`.
fn repeated_lines(state: &State, horizontal: bool, mut errors: Option<&mut [i32]>) -> i32 {
    let (w2, h2) = (state.w2 as usize, state.h2 as usize);
    let (rmult, cmult, nr, nc) = if horizontal {
        (w2, 1, h2, w2)
    } else {
        (1, w2, w2, h2)
    };
    let err = if horizontal {
        FE_ROW_MATCH
    } else {
        FE_COL_MATCH
    };
    let at = |r: usize, c: usize| state.grid[r * rmult + c * cmult];
    let mut found = 0;
    for r in 0..nr {
        if (0..nc).any(|c| at(r, c) == Cell::Empty) {
            continue;
        }
        for r2 in r + 1..nr {
            if (0..nc).all(|c| at(r, c) == at(r2, c)) {
                if let Some(errors) = errors.as_deref_mut() {
                    for c in 0..nc {
                        errors[r * rmult + c * cmult] |= err;
                        errors[r2 * rmult + c * cmult] |= err;
                    }
                }
                found += 1;
            }
        }
    }
    found
}

/// C's `unruly_validate_all_rows`: whether there is a run of three or (in
/// unique mode) a repeated line, marking them in `errors`.
fn has_row_errors(state: &State, mut errors: Option<&mut [i32]>) -> bool {
    let mut found = 0;
    for (horizontal, check) in [
        (true, Cell::One),
        (false, Cell::One),
        (true, Cell::Zero),
        (false, Cell::Zero),
    ] {
        found += three_in_a_row(state, horizontal, check, errors.as_deref_mut());
    }
    if state.unique {
        found += repeated_lines(state, true, errors.as_deref_mut());
        found += repeated_lines(state, false, errors);
    }
    found != 0
}

/// C's `unruly_validate_counts`: whether every line has its share of
/// each colour. `excess` gets, per line, whether it has too many: ones
/// by row, zeros by row, ones by column, zeros by column.
fn balance(state: &State, counts: &Counts, mut excess: Option<&mut [bool]>) -> Balance {
    let (w2, h2) = (state.w2 as usize, state.h2 as usize);
    let (w, h) = (state.w2 / 2, state.h2 / 2);
    let mut below = false;
    let mut above = false;
    let mut check = |count: i32, target: i32, flag: usize| {
        below |= count < target;
        above |= count > target;
        if let Some(excess) = excess.as_deref_mut() {
            excess[flag] = count > target;
        }
    };
    for i in 0..w2 {
        check(counts.ones_cols[i], h, 2 * h2 + i);
        check(counts.zeros_cols[i], h, 2 * h2 + w2 + i);
    }
    for i in 0..h2 {
        check(counts.ones_rows[i], w, i);
        check(counts.zeros_rows[i], w, h2 + i);
    }
    if above {
        Balance::Excess
    } else if below {
        Balance::Short
    } else {
        Balance::Exact
    }
}

// ---------------------------------------------------------------------
// Solver

/// Which line-filling technique [`Solver::fill_lines`] applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineRule {
    /// One square left (C's `check_single_gap`).
    SingleGap,
    /// Any squares left (C's `check_complete_nums`).
    Complete,
}

/// What [`solve`] made of a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Solved.
    Solved,
    /// The techniques ran out (C: "No solution found.").
    NoSolution,
    /// The clues contradict each other (C: "Puzzle is invalid.").
    Invalid,
}

/// The result of [`solve`].
#[derive(Clone, Debug)]
pub struct Solved {
    /// The position as far as the solver got.
    pub state: State,
    /// The hardest technique used, `None` if none was needed (C's
    /// `maxdiff` of -1).
    pub hardest: Option<Difficulty>,
    /// Whether it solved it.
    pub verdict: Verdict,
}

/// Solves a position with every technique (C's `solve_game`, and the
/// standalone solver's run on a game ID), writing its reasoning to `log`.
#[must_use]
pub fn solve(state: &State, log: &mut SolverLog) -> Solved {
    let mut solver = Solver::new(state.clone(), log);
    let hardest = solver.run(Difficulty::Normal);
    let mut verdict = match balance(&solver.state, &solver.counts, None) {
        Balance::Exact => Verdict::Solved,
        Balance::Short => Verdict::NoSolution,
        Balance::Excess => Verdict::Invalid,
    };
    if has_row_errors(&solver.state, None) {
        verdict = Verdict::Invalid;
    }
    Solved {
        state: solver.state,
        hardest,
        verdict,
    }
}

/// The solver: a position being filled in, with its counts (C's
/// `game_state` and `unruly_scratch`).
struct Solver<'a> {
    state: State,
    counts: Counts,
    log: &'a mut SolverLog,
}

impl<'a> Solver<'a> {
    fn new(state: State, log: &'a mut SolverLog) -> Self {
        let counts = state.counts();
        Self { state, counts, log }
    }

    /// With the counts given, rather than taken from the grid.
    fn with_counts(state: State, counts: Counts, log: &'a mut SolverLog) -> Self {
        Self { state, counts, log }
    }

    fn w2(&self) -> usize {
        self.state.w2 as usize
    }

    /// Places `c` at square `i`.
    fn place(&mut self, i: usize, c: Cell) {
        let w2 = self.w2();
        self.state.grid[i] = c;
        self.counts.add(c, i % w2, i / w2);
    }

    /// C's `unruly_solve_game`: uses the techniques up to `max` until
    /// none helps. Returns the hardest one used.
    fn run(&mut self, max: Difficulty) -> Option<Difficulty> {
        let mut hardest = None;
        loop {
            let level = if self.all_threes() > 0 || self.all_single_gaps() > 0 {
                Difficulty::Trivial
            } else if max < Difficulty::Easy {
                break;
            } else if self.all_complete() > 0 || (self.state.unique && self.all_uniques() > 0) {
                Difficulty::Easy
            } else if max < Difficulty::Normal {
                break;
            } else if self.all_near_complete() > 0 {
                Difficulty::Normal
            } else {
                break;
            };
            hardest = hardest.max(Some(level));
        }
        hardest
    }

    /// C's `unruly_solver_check_threes`: next to a pair of `check`s, or
    /// between two, a square must be `block`.
    fn threes(&mut self, horizontal: bool, check: Cell, block: Cell) -> i32 {
        let (w2, h2) = (self.state.w2, self.state.h2);
        let (dx, dy) = if horizontal { (1, 0) } else { (0, 1) };
        let mut found = 0;
        for y in dy..h2 - dy {
            for x in dx..w2 - dx {
                let i1 = self.state.index(x - dx, y - dy);
                let i2 = self.state.index(x, y);
                let i3 = self.state.index(x + dx, y + dy);
                // Which square is empty, given the other two.
                for (a, b, empty) in [(i1, i2, i3), (i1, i3, i2), (i2, i3, i1)] {
                    let g = &self.state.grid;
                    if g[a] == check && g[b] == check && g[empty] == Cell::Empty {
                        found += 1;
                        let w = w2 as usize;
                        self.log.line(format_args!(
                            "Solver: {},{} and {},{} confirm {} at {},{}",
                            a % w,
                            a / w,
                            b % w,
                            b / w,
                            block.digit(),
                            empty % w,
                            empty / w
                        ));
                        self.place(empty, block);
                    }
                }
            }
        }
        found
    }

    fn all_threes(&mut self) -> i32 {
        self.threes(true, Cell::One, Cell::Zero)
            + self.threes(true, Cell::Zero, Cell::One)
            + self.threes(false, Cell::One, Cell::Zero)
            + self.threes(false, Cell::Zero, Cell::One)
    }

    /// C's `unruly_solver_check_uniques`: a line one `check` short of
    /// copying a full-of-`check` line must get `block` where they differ.
    fn uniques(&mut self, horizontal: bool, check: Cell, block: Cell) -> i32 {
        let (w2, h2) = (self.w2(), self.state.h2 as usize);
        let (rmult, cmult, nr, nc) = if horizontal {
            (w2, 1, h2, w2)
        } else {
            (1, w2, w2, h2)
        };
        let max = (nc / 2) as i32;
        let mut found = 0;
        for r in 0..nr {
            if self.counts.lines(check, horizontal)[r] != max {
                continue;
            }
            for r2 in 0..nr {
                if self.counts.lines(check, horizontal)[r2] != max - 1 {
                    continue;
                }
                let g = &self.state.grid;
                let mut nmatch = 0;
                let mut nonmatch = None;
                for c in 0..nc {
                    if g[r * rmult + c * cmult] == check {
                        if g[r2 * rmult + c * cmult] == check {
                            nmatch += 1;
                        } else {
                            nonmatch = Some(c);
                        }
                    }
                }
                if nmatch != max - 1 {
                    continue;
                }
                // Line r has max checks and only max - 1 are matched.
                let Some(nonmatch) = nonmatch else {
                    continue;
                };
                let i1 = r2 * rmult + nonmatch * cmult;
                // C asserts the square is empty if not already `block`.
                if g[i1] != Cell::Empty {
                    continue;
                }
                self.log.line(format_args!(
                    "Solver: matching {} {}, {} gives {} at {},{}",
                    if horizontal { "rows" } else { "cols" },
                    r,
                    r2,
                    block.digit(),
                    i1 % w2,
                    i1 / w2
                ));
                self.place(i1, block);
                found += 1;
            }
        }
        found
    }

    fn all_uniques(&mut self) -> i32 {
        self.uniques(true, Cell::One, Cell::Zero)
            + self.uniques(true, Cell::Zero, Cell::One)
            + self.uniques(false, Cell::One, Cell::Zero)
            + self.uniques(false, Cell::Zero, Cell::One)
    }

    /// C's `unruly_solver_fill_row`: `fill` in every empty square of row
    /// (`horizontal`) or column `i`.
    fn fill_line(&mut self, i: usize, horizontal: bool, fill: Cell) -> i32 {
        let (w2, h2) = (self.w2(), self.state.h2 as usize);
        self.log.indent();
        self.log.raw(format_args!(
            "Solver: Filling {} {i} with {}:",
            if horizontal { "Row" } else { "Col" },
            fill.digit()
        ));
        let mut found = 0;
        for j in 0..if horizontal { w2 } else { h2 } {
            let p = if horizontal { i * w2 + j } else { j * w2 + i };
            if self.state.grid[p] == Cell::Empty {
                let (x, y) = if horizontal { (j, i) } else { (i, j) };
                self.log.raw(format_args!(" ({x},{y})"));
                found += 1;
                self.place(p, fill);
            }
        }
        self.log.raw(format_args!("\n"));
        found
    }

    /// C's `unruly_solver_check_single_gap`: a line with all of the other
    /// colour and one square left gets `fill` there.
    fn single_gaps(&mut self, horizontal: bool, fill: Cell) -> i32 {
        self.fill_lines(horizontal, fill, LineRule::SingleGap)
    }

    /// C's `unruly_solver_check_complete_nums`: a line with all of the
    /// other colour gets `fill` in every empty square.
    fn complete(&mut self, horizontal: bool, fill: Cell) -> i32 {
        self.fill_lines(horizontal, fill, LineRule::Complete)
    }

    /// Fills each line with all of the other colour that `rule` applies
    /// to, logging why.
    fn fill_lines(&mut self, horizontal: bool, fill: Cell, rule: LineRule) -> i32 {
        let (count, target) = if horizontal {
            (self.state.h2, self.state.w2 / 2)
        } else {
            (self.state.w2, self.state.h2 / 2)
        };
        let mut found = 0;
        for i in 0..count as usize {
            let complete = self.counts.lines(fill.other(), horizontal)[i];
            let other = self.counts.lines(fill, horizontal)[i];
            let applies = complete == target
                && match rule {
                    LineRule::SingleGap => other == target - 1,
                    LineRule::Complete => other < target,
                };
            if applies {
                if rule == LineRule::SingleGap {
                    self.log.line(format_args!(
                        "Solver: Row {i} has only one square left which must be {}",
                        fill.digit()
                    ));
                } else {
                    self.log.line(format_args!(
                        "Solver: Row {i} satisfied for {}",
                        fill.other().digit()
                    ));
                }
                found += self.fill_line(i, horizontal, fill);
            }
        }
        found
    }

    fn all_single_gaps(&mut self) -> i32 {
        self.single_gaps(true, Cell::Zero)
            + self.single_gaps(false, Cell::Zero)
            + self.single_gaps(true, Cell::One)
            + self.single_gaps(false, Cell::One)
    }

    fn all_complete(&mut self) -> i32 {
        self.complete(true, Cell::Zero)
            + self.complete(false, Cell::Zero)
            + self.complete(true, Cell::One)
            + self.complete(false, Cell::One)
    }

    /// C's `unruly_solver_check_near_complete`: in a line with one of the
    /// other colour left, that one must go among three squares that
    /// would otherwise make three `fill`s in a row (a `fill` and two empty
    /// squares, or three empty squares), so every other empty square of
    /// the line is `fill`.
    fn near_complete(&mut self, horizontal: bool, fill: Cell) -> i32 {
        let (w2, h2) = (self.state.w2, self.state.h2);
        let (w, h) = (w2 / 2, h2 / 2);
        let (dx, dy) = if horizontal { (1, 0) } else { (0, 1) };
        let other = fill.other();
        let mut found = 0;
        for y in dy..h2 - dy {
            // One colour must have one left, the other at least two.
            if horizontal
                && (self.counts.lines(other, true)[y as usize] < w - 1
                    || self.counts.lines(fill, true)[y as usize] > w - 2)
            {
                continue;
            }
            for x in dx..w2 - dx {
                if !horizontal
                    && (self.counts.lines(other, false)[x as usize] < h - 1
                        || self.counts.lines(fill, false)[x as usize] > h - 2)
                {
                    continue;
                }
                let line = if horizontal { y } else { x } as usize;
                let i1 = self.state.index(x - dx, y - dy);
                let i2 = self.state.index(x, y);
                let i3 = self.state.index(x + dx, y + dy);
                let g = &self.state.grid;
                let pattern = [g[i1], g[i2], g[i3]];
                // The squares to hold aside, by which one is `fill`.
                let hold: &[usize] = match pattern {
                    [f, Cell::Empty, Cell::Empty] if f == fill => &[i2, i3],
                    [Cell::Empty, f, Cell::Empty] if f == fill => &[i1, i3],
                    [Cell::Empty, Cell::Empty, f] if f == fill => &[i1, i2],
                    [Cell::Empty, Cell::Empty, Cell::Empty] => &[i1, i2, i3],
                    _ => continue,
                };
                let hold = hold.to_vec();
                for &i in &hold {
                    self.state.grid[i] = Cell::Bogus;
                }
                self.log.line(format_args!(
                    "Solver: Row {line} nearly satisfied for {}",
                    other.digit()
                ));
                found += self.fill_line(line, horizontal, fill);
                for &i in &hold {
                    self.state.grid[i] = Cell::Empty;
                }
            }
        }
        found
    }

    fn all_near_complete(&mut self) -> i32 {
        self.near_complete(true, Cell::Zero)
            + self.near_complete(false, Cell::Zero)
            + self.near_complete(true, Cell::One)
            + self.near_complete(false, Cell::One)
    }
}

// ---------------------------------------------------------------------
// Generator

/// C's `unruly_fill_game`: fills the grid at random, the solver placing
/// what each choice forces. False if the result breaks a rule.
fn fill_grid(solver: &mut Solver<'_>, rng: &mut RandomState) -> bool {
    solver
        .log
        .line(format_args!("Generator: Attempt to fill grid"));
    let mut spaces: Vec<usize> = (0..solver.state.grid.len()).collect();
    rng.shuffle(&mut spaces);
    for i in spaces {
        if solver.state.grid[i] != Cell::Empty {
            continue;
        }
        let c = if rng.upto(2) != 0 {
            Cell::One
        } else {
            Cell::Zero
        };
        solver.place(i, c);
        solver.run(Difficulty::Normal);
    }
    !has_row_errors(&solver.state, None)
        && balance(&solver.state, &solver.counts, None) == Balance::Exact
}

/// Generates a puzzle description (C's `new_game_desc`; with a verbose
/// log, the standalone solver's generation report). `params` are valid.
#[must_use]
pub fn generate(params: &Params, rng: &mut RandomState, log: &mut SolverLog) -> String {
    let diff = params.diff.unwrap_or(Difficulty::Normal);
    let mut quiet = SolverLog::quiet();
    let mut attempts = 0;
    let state = loop {
        let mut state = loop {
            attempts += 1;
            let mut solver = Solver::new(State::blank(params.w2, params.h2, params.unique), log);
            if fill_grid(&mut solver, rng) {
                break solver.state;
            }
        };

        if log.enabled() {
            log.line(format_args!("Puzzle generated in {attempts} attempts"));
            log.raw(format_args!("{}", text(&state)));
        }

        // Empty squares in random order while the solver still finishes.
        let mut spaces: Vec<usize> = (0..state.grid.len()).collect();
        rng.shuffle(&mut spaces);
        for i in spaces {
            let c = state.grid[i];
            state.grid[i] = Cell::Empty;
            let counts = state.counts();
            let mut solver = Solver::with_counts(state.clone(), counts, &mut quiet);
            solver.run(diff);
            if balance(&solver.state, &solver.counts, None) != Balance::Exact {
                state.grid[i] = c;
            }
        }

        if log.enabled() {
            log.line(format_args!("Final puzzle:"));
            log.raw(format_args!("{}", text(&state)));
        }

        // Too easy if one difficulty lower doesn't solve it either.
        let Some(easier) = diff.easier() else {
            break state;
        };
        let mut solver = Solver::new(state.clone(), log);
        solver.run(easier);
        if balance(&solver.state, &solver.counts, None) == Balance::Short {
            break state;
        }
    };
    encode_desc(&state)
}

fn text(state: &State) -> String {
    Unruly::text_format(state).unwrap_or_default()
}

/// The description: per clue, a letter giving the empty squares before it
/// (`a` none) in lower case for 0 and upper case for 1, `z`/`Z` for 25
/// empty squares, and a last letter for the empty squares at the end.
fn encode_desc(state: &State) -> String {
    let mut desc = String::new();
    let mut run = 0_u8;
    for i in 0..=state.grid.len() {
        let cell = state.grid.get(i).copied();
        let base = match cell {
            None | Some(Cell::Zero) => b'a',
            Some(Cell::One) => b'A',
            Some(Cell::Empty | Cell::Bogus) => {
                run += 1;
                continue;
            }
        };
        while run > 24 {
            desc.push(char::from(base + 25));
            run -= 25;
        }
        desc.push(char::from(base + run));
        run = 0;
    }
    desc
}

// ---------------------------------------------------------------------
// Drawing

/// C's `unruly_draw_err_rectangle`.
fn draw_err_rectangle(dr: &mut Drawing<'_>, x: i32, y: i32, w: i32, h: i32, ts: i32) {
    let thick = ts / 10;
    let margin = ts / 20;
    dr.rect(x + margin, y + margin, w - 2 * margin, thick, Col::Error);
    dr.rect(x + margin, y + margin, thick, h - 2 * margin, Col::Error);
    dr.rect(
        x + margin,
        y + h - margin - thick,
        w - 2 * margin,
        thick,
        Col::Error,
    );
    dr.rect(
        x + w - margin - thick,
        y + margin,
        thick,
        h - 2 * margin,
        Col::Error,
    );
}

/// C's `unruly_draw_tile`.
fn draw_tile(dr: &mut Drawing<'_>, x: i32, y: i32, ts: i32, tile: i32) {
    dr.clip(x, y, ts, ts);
    // The grid edge first, so the tile can draw over it.
    dr.rect(x, y, ts, ts, Col::Grid);

    let square = if tile & FF_ZERO != 0 {
        Some(Col::Zero)
    } else if tile & FF_ONE != 0 {
        Some(Col::One)
    } else {
        None
    };
    // The flash shows a square's bevel colours in turn.
    let fill = match square {
        Some(c) if tile & FF_FLASH1 != 0 => c.highlight(),
        Some(c) if tile & FF_FLASH2 != 0 => c.lowlight(),
        Some(c) => c,
        None => Col::Empty,
    };
    dr.rect(x, y, ts - 1, ts - 1, fill);
    if let Some(c) = square
        && fill == c
        && tile & FF_IMMUTABLE != 0
    {
        // Clues are sunk: shadow top and left, light bottom and right.
        let inset = ts / 6;
        let len = ts - 2 * inset - 2;
        dr.rect(x + inset, y + inset, len, 1, c.lowlight());
        dr.rect(x + inset, y + inset, 1, len, c.lowlight());
        dr.rect(x + inset + 1, y + ts - inset - 2, len, 1, c.highlight());
        dr.rect(x + ts - inset - 2, y + inset + 1, 1, len, c.highlight());
    }

    // Three in a row.
    if tile & (FE_HOR_ROW_LEFT | FE_HOR_ROW_RIGHT) != 0 {
        let (mut left, mut right) = (x, x + ts - 1);
        if tile & FE_HOR_ROW_LEFT != 0 {
            right += ts / 2;
        }
        if tile & FE_HOR_ROW_RIGHT != 0 {
            left -= ts / 2;
        }
        draw_err_rectangle(dr, left, y, right - left, ts - 1, ts);
    }
    if tile & (FE_VER_ROW_TOP | FE_VER_ROW_BOTTOM) != 0 {
        let (mut top, mut bottom) = (y, y + ts - 1);
        if tile & FE_VER_ROW_TOP != 0 {
            bottom += ts / 2;
        }
        if tile & FE_VER_ROW_BOTTOM != 0 {
            top -= ts / 2;
        }
        draw_err_rectangle(dr, x, top, ts - 1, bottom - top, ts);
    }

    if tile & FE_COUNT != 0 {
        dr.text(
            x + ts / 2,
            y + ts / 2,
            FontType::Variable,
            ts / 2,
            Align::CENTRE,
            Col::Error,
            "!",
        );
    }

    // Repeated rows and columns.
    if tile & FE_ROW_MATCH != 0 {
        dr.rect(x, y + ts / 2 - ts / 12, ts, 2 * (ts / 12), Col::Error);
    }
    if tile & FE_COL_MATCH != 0 {
        dr.rect(x + ts / 2 - ts / 12, y, 2 * (ts / 12), ts, Col::Error);
    }

    if tile & FF_CURSOR != 0 {
        let t = ts / 12;
        dr.rect(x, y, t, ts - 1, Col::Cursor);
        dr.rect(x, y, ts - 1, t, Col::Cursor);
        dr.rect(x + ts - 1 - t, y, t, ts - 1, Col::Cursor);
        dr.rect(x, y + ts - 1 - t, ts - 1, t, Col::Cursor);
    }

    dr.unclip();
    dr.update(x, y, ts, ts);
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn params(id: &str) -> Params {
        let mut p = Unruly::default_params();
        Unruly::decode_params(&mut p, id);
        p
    }

    fn start(id: &str, seed: &[u8]) -> State {
        let p = params(id);
        let desc = Unruly::new_desc(&p, &mut RandomState::new(seed), true).desc;
        Unruly::new_game(&mut GameCtx::new(), &p, &desc)
    }

    fn encode(mv: &Move) -> String {
        let mut s = String::new();
        Unruly::encode_move(mv, &mut s);
        s
    }

    #[test]
    fn params_parse_like_c() {
        assert_eq!(
            params("10x12udn"),
            Params {
                w2: 10,
                h2: 12,
                unique: true,
                diff: Some(Difficulty::Normal)
            }
        );
        // `u` is reset by every parse; the difficulty is kept.
        let mut p = params("8u");
        assert!(p.unique);
        Unruly::decode_params(&mut p, "6");
        assert_eq!(
            (p.w2, p.h2, p.unique, p.diff),
            (6, 6, false, Some(Difficulty::Trivial))
        );
        assert_eq!(params("8dq").diff, None);
        assert_eq!(params("8d").diff, None);
        assert_eq!(Unruly::encode_params(&params("8x6ude"), false), "8x6u");
        assert_eq!(Unruly::encode_params(&params("8x6ude"), true), "8x6ude");
        let err = |id| Unruly::validate_params(&params(id), true).map_err(|e| e.0);
        assert_eq!(err("7x8"), Err("Width and height must both be even".into()));
        assert_eq!(
            err("4x8"),
            Err("Width and height must be at least 6".into())
        );
        assert_eq!(
            err("6x16u"),
            Err("Puzzle is too tall for unique-rows mode".into())
        );
        assert_eq!(
            err("16x6u"),
            Err("Puzzle is too long for unique-rows mode".into())
        );
        assert_eq!(err("6x14u"), Ok(()));
        assert_eq!(err("8dx"), Err("Unknown difficulty rating".into()));
    }

    #[test]
    fn descriptions_round_trip() {
        for id in ["6x6dt", "8x10de", "14dn", "8udn"] {
            let state = start(id, id.as_bytes());
            let p = params(id);
            let desc = encode_desc(&state);
            assert_eq!(Unruly::validate_desc(&p, &desc), Ok(()), "{id}");
            assert_eq!(Unruly::new_game(&mut GameCtx::new(), &p, &desc), state);
        }
        let p = params("6");
        assert_eq!(Unruly::validate_desc(&p, "zl"), Ok(()));
        assert_eq!(
            Unruly::validate_desc(&p, "zk"),
            Err(GameError::new("Description too short"))
        );
        assert_eq!(
            Unruly::validate_desc(&p, "zZb"),
            Err(GameError::new("Description too long"))
        );
        assert_eq!(
            Unruly::validate_desc(&p, "z1"),
            Err(GameError::new("Description contains invalid characters"))
        );
    }

    #[test]
    fn generated_puzzles_solve() {
        for id in ["6x6dt", "8x8de", "8x8dn", "10x6udn"] {
            for seed in 0..4_u8 {
                let state = start(id, &[seed]);
                let mv = Unruly::solve(&state, &state, None).expect("solvable");
                let solved = Unruly::execute_move(&mut GameCtx::new(), &state, &mv).expect("legal");
                assert!(!has_row_errors(&solved, None), "{id}");
                assert_eq!(balance(&solved, &solved.counts(), None), Balance::Exact);
                assert_eq!(Unruly::status(&solved), GameStatus::Won);
            }
        }
    }

    #[test]
    fn placing_the_last_square_completes() {
        let state = start("6x6dt", b"place");
        let Ok(Move::Solve(ones)) = Unruly::solve(&state, &state, None) else {
            panic!("no solution");
        };
        let mut s = state.clone();
        let mut ui = Unruly::new_ui(None);
        for (i, &one) in ones.iter().enumerate() {
            if s.immutable[i] {
                continue;
            }
            let (x, y) = (i as i32 % 6, i as i32 / 6);
            let next = Unruly::execute_move(
                &mut GameCtx::new(),
                &s,
                &Move::Place {
                    colour: Some(one),
                    x,
                    y,
                },
            )
            .expect("legal");
            if next.completed {
                assert_eq!(
                    Unruly::flash_length(&s, &next, 1, &mut ui),
                    FLASH_FRAME * 3.0
                );
            }
            s = next;
        }
        assert!(s.completed && !s.cheated);
        // Clues can't be changed.
        let clue = (0..36).find(|&i| s.immutable[i]).unwrap() as i32;
        let mv = Move::Place {
            colour: None,
            x: clue % 6,
            y: clue / 6,
        };
        assert_eq!(Unruly::execute_move(&mut GameCtx::new(), &s, &mv), None);
    }

    #[test]
    fn moves_decode_like_c() {
        let state = start("6x6dt", b"codec");
        let ok = |t: &str| Unruly::decode_move(&state, t);
        assert_eq!(
            ok("P1, 2,+3junk"),
            Ok(Move::Place {
                colour: Some(true),
                x: 2,
                y: 3
            })
        );
        assert_eq!(
            ok("P-,0,5"),
            Ok(Move::Place {
                colour: None,
                x: 0,
                y: 5
            })
        );
        for bad in [
            "P", "P1", "P1,", "P1,2", "Px,1,1", "P1,6,0", "P1 ,1,1", "S0101", "Q",
        ] {
            assert_eq!(ok(bad), Err(MoveParseError), "{bad:?}");
        }
        let solve = format!("S{}trailing", "01".repeat(18));
        let Ok(Move::Solve(ones)) = ok(&solve) else {
            panic!()
        };
        assert_eq!(encode(&Move::Solve(ones)), format!("S{}", "01".repeat(18)));
        assert_eq!(ok(&format!("S{}2", "0".repeat(35))), Err(MoveParseError));
    }

    proptest! {
        #[test]
        fn move_codec_round_trips(
            colour in prop::option::of(any::<bool>()),
            x in 0..6_i32,
            y in 0..6_i32,
            ones in prop::collection::vec(any::<bool>(), 36),
        ) {
            let state = start("6x6dt", b"codec");
            for mv in [Move::Place { colour, x, y }, Move::Solve(ones.clone())] {
                prop_assert_eq!(Unruly::decode_move(&state, &encode(&mv)), Ok(mv));
            }
        }

        /// Untrusted text never panics.
        #[test]
        fn untrusted_text_never_panics(
            id in "\\PC{0,12}",
            desc in "[a-zA-Z0-9]{0,20}",
            mv in "[PS01, -]{0,12}|\\PC{0,12}",
        ) {
            let mut p = Unruly::default_params();
            Unruly::decode_params(&mut p, &id);
            let p = if Unruly::validate_params(&p, true).is_ok() && p.w2 * p.h2 <= 100 {
                p
            } else {
                params("6x6")
            };
            let _ = Unruly::encode_params(&p, true);
            if Unruly::validate_desc(&p, &desc).is_ok() {
                let state = Unruly::new_game(&mut GameCtx::new(), &p, &desc);
                if let Ok(mv) = Unruly::decode_move(&state, &mv) {
                    let _ = Unruly::execute_move(&mut GameCtx::new(), &state, &mv);
                }
                let _ = Unruly::solve(&state, &state, None);
            }
        }
    }
}
