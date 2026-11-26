//! Port of third_party/puzzles-c/flip.c (upstream 616da16).
//!
//! Flip: turn every light on. Clicking a square toggles a fixed set of
//! squares around it, its "shape", drawn as a small diagram in the
//! square. Shapes are either crosses or random ominoes. The puzzle is a
//! linear system over GF(2), which the solver reduces by Gaussian
//! elimination.
//!
//! Known C behaviour kept as is: random shapes on a 1 × 2 or 2 × 1 grid
//! always make two identical shapes, which the generator rejects
//! forever, so generation never ends there (front ends generate on a
//! cancellable worker thread).

use std::cmp::Ordering;
use std::fmt::Write as _;
use std::rc::Rc;

use puzzles_core::button::{Button, MouseAction, MouseButton};
use puzzles_core::config::ConfigItem;
use puzzles_core::drawing::Drawing;
use puzzles_core::game::{
    Game, GameCtx, GameError, GameStatus, MoveParseError, MoveResult, NewDesc, Rect,
};
use puzzles_core::misc::{env_bool, move_cursor};
use puzzles_core::palette::ColourSpec;
use puzzles_core::parse::{atoi, scan_int, skip_digits};
use puzzles_core::preset::PresetMenu;
use puzzles_core::random::RandomState;
use puzzles_core::sorted_set::{Relation, SortedSet, find_rel};

const PREFERRED_TILE_SIZE: i32 = 48;
/// Seconds a square takes to flip over.
const ANIM_TIME: f32 = 0.25;
/// Seconds per ring of the completion flash.
const FLASH_FRAME: f32 = 0.07;

/// Grid bit: the light is off (the square is wrong).
const WRONG: u8 = 1;
/// Grid bit: the solver says to click this square.
const HINT: u8 = 2;
/// Tile bit, drawing only: the keyboard cursor is here.
const CURSOR: u8 = 4;

puzzles_core::palette! {
    /// Flip's colours.
    pub enum Col {
        Background => Role(Background),
        /// A light that is off.
        Wrong => Derived(Background, Shade(1.0 / 3.0)),
        /// A light that is on.
        Right => Specific("right"),
        Grid => Role(Grid),
        /// The shape diagram in each square; C copies the grid colour.
        Diag => Derived(Grid, Shade(1.0)),
        Hint => Role(Hint),
        Cursor => Role(Cursor),
    }
}

/// How the shapes are made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixType {
    /// Each click toggles the square and its four neighbours.
    Crosses,
    /// Each click toggles a random omino within the square's 3 × 3
    /// neighbourhood.
    Random,
}

/// Flip's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Width in squares.
    pub w: i32,
    /// Height in squares.
    pub h: i32,
    /// How the shapes are made.
    pub matrix_type: MatrixType,
}

impl Params {
    /// Width, height and area as indices; the parameters are valid.
    fn dims(&self) -> (usize, usize, usize) {
        let (w, h) = (self.w as usize, self.h as usize);
        (w, h, w * h)
    }
}

/// A position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    w: i32,
    h: i32,
    moves: i32,
    completed: bool,
    cheated: bool,
    hints_active: bool,
    /// Per square: [`WRONG`] and [`HINT`] bits.
    grid: Vec<u8>,
    /// Row `i` (`w*h` entries) is 1 where clicking square `i` toggles.
    /// Shared by every state of a game.
    matrix: Rc<[u8]>,
}

impl State {
    fn area(&self) -> usize {
        self.grid.len()
    }

    /// Whether clicking square `i` toggles square `j`.
    fn toggles(&self, i: usize, j: usize) -> bool {
        self.matrix[i * self.area() + j] != 0
    }
}

/// A move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Move {
    /// Click the square at `(x, y)`: `M<x>,<y>`.
    Flip {
        /// Column.
        x: i32,
        /// Row.
        y: i32,
    },
    /// Show the squares to click, one flag per square: `S` and a `0` or
    /// `1` per square (C sets a hint for any character but `0`).
    Solve(Vec<bool>),
}

/// The keyboard cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ui {
    cx: i32,
    cy: i32,
    /// Whether the cursor is shown.
    cdraw: bool,
}

/// What was last drawn.
#[derive(Clone, Debug)]
pub struct DrawState {
    w: i32,
    h: i32,
    started: bool,
    /// Per square, the tile bits last drawn, or `None` if it must be
    /// drawn again (never drawn, or drawn mid-animation).
    tiles: Vec<Option<u8>>,
    tilesize: i32,
}

impl DrawState {
    fn border(&self) -> i32 {
        self.tilesize / 2
    }

    /// C's `COORD`.
    fn coord(&self, x: i32) -> i32 {
        x * self.tilesize + self.border()
    }

    /// C's `FROMCOORD`: the square at a pixel coordinate.
    fn tile_at(&self, x: i32) -> i32 {
        (x - self.border() + self.tilesize) / self.tilesize - 1
    }
}

/// The Flip puzzle.
#[derive(Clone, Copy, Debug)]
pub struct Flip;

impl Game for Flip {
    type Params = Params;
    type State = State;
    type Ui = Ui;
    type DrawState = DrawState;
    type Move = Move;

    const NAME: &'static str = "Flip";
    const PREFERRED_TILESIZE: i32 = PREFERRED_TILE_SIZE;
    const PALETTE: &'static [ColourSpec] = Col::PALETTE;
    const CAN_CONFIGURE: bool = true;
    const CAN_SOLVE: bool = true;
    const CAN_FORMAT_AS_TEXT_EVER: bool = true;
    const WANTS_STATUSBAR: bool = true;

    fn default_params() -> Params {
        Params {
            w: 5,
            h: 5,
            matrix_type: MatrixType::Crosses,
        }
    }

    fn presets() -> PresetMenu<Params> {
        let mut menu = PresetMenu::new();
        for matrix_type in [MatrixType::Crosses, MatrixType::Random] {
            for size in 3..=5 {
                let name = match matrix_type {
                    MatrixType::Crosses => "Crosses",
                    MatrixType::Random => "Random",
                };
                menu.add_preset(
                    format!("{size}x{size} {name}"),
                    Params {
                        w: size,
                        h: size,
                        matrix_type,
                    },
                );
            }
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
        match s.first() {
            Some(b'r') => params.matrix_type = MatrixType::Random,
            Some(b'c') => params.matrix_type = MatrixType::Crosses,
            _ => {}
        }
    }

    fn encode_params(params: &Params, full: bool) -> String {
        let suffix = match (full, params.matrix_type) {
            (false, _) => "",
            (true, MatrixType::Crosses) => "c",
            (true, MatrixType::Random) => "r",
        };
        format!("{}x{}{suffix}", params.w, params.h)
    }

    fn configure(params: &Params) -> Option<Vec<ConfigItem>> {
        Some(vec![
            ConfigItem::string("Width", params.w.to_string()),
            ConfigItem::string("Height", params.h.to_string()),
            ConfigItem::choices(
                "Shape type",
                &["Crosses", "Random"],
                match params.matrix_type {
                    MatrixType::Crosses => 0,
                    MatrixType::Random => 1,
                },
            ),
        ])
    }

    fn custom_params(config: &[ConfigItem]) -> Result<Params, GameError> {
        let wrong = || GameError::new("Unexpected configuration fields");
        let [w, h, shape] = config else {
            return Err(wrong());
        };
        Ok(Params {
            w: atoi(w.as_str().ok_or_else(wrong)?.as_bytes()),
            h: atoi(h.as_str().ok_or_else(wrong)?.as_bytes()),
            matrix_type: match shape.selected() {
                Some(0) => MatrixType::Crosses,
                Some(1) => MatrixType::Random,
                _ => return Err(wrong()),
            },
        })
    }

    fn validate_params(params: &Params, _full: bool) -> Result<(), GameError> {
        if params.w <= 0 || params.h <= 0 {
            return Err(GameError::new(
                "Width and height must both be greater than zero",
            ));
        }
        if params.w > (i32::MAX - 3) / params.h {
            return Err(GameError::new(
                "Width times height must not be unreasonably large",
            ));
        }
        let wh = params.w * params.h;
        if wh > (i32::MAX - 3) / wh {
            return Err(GameError::new("Width times height is too large"));
        }
        Ok(())
    }

    // C: flip.c, new_game_desc()
    fn new_desc(params: &Params, rng: &mut RandomState, _interactive: bool) -> NewDesc {
        let (_, _, wh) = params.dims();
        let matrix = match params.matrix_type {
            MatrixType::Crosses => crosses_matrix(params),
            MatrixType::Random => random_matrix(params, rng),
        };

        // Choose equiprobably from the input space and map it through
        // the matrix: every soluble light set is then equally likely
        // (each has as many preimages as the kernel has elements).
        let mut grid = vec![0_u8; wh];
        loop {
            grid.fill(0);
            for row in matrix.chunks_exact(wh) {
                if rng.upto(2) != 0 {
                    for (g, &m) in grid.iter_mut().zip(row) {
                        *g ^= m;
                    }
                }
            }
            // Never start solved.
            if grid.iter().any(|&g| g != 0) {
                break;
            }
        }
        NewDesc::new(format!(
            "{},{}",
            encode_bitmap(&matrix),
            encode_bitmap(&grid)
        ))
    }

    fn validate_desc(params: &Params, desc: &str) -> Result<(), GameError> {
        let (_, _, wh) = params.dims();
        let mlen = (wh * wh).div_ceil(4);
        let glen = wh.div_ceil(4);
        let desc = desc.as_bytes();
        let hex_run = |s: &[u8]| s.iter().take_while(|b| b.is_ascii_hexdigit()).count();

        if hex_run(desc) != mlen {
            return Err(GameError::new("Matrix description is wrong length"));
        }
        if desc.get(mlen) != Some(&b',') {
            return Err(GameError::new("Expected comma after matrix description"));
        }
        if hex_run(&desc[mlen + 1..]) != glen {
            return Err(GameError::new("Grid description is wrong length"));
        }
        if desc.len() > mlen + 1 + glen {
            return Err(GameError::new("Unexpected data after grid description"));
        }
        Ok(())
    }

    fn new_game(_ctx: &mut GameCtx, params: &Params, desc: &str) -> State {
        let (_, _, wh) = params.dims();
        let mlen = (wh * wh).div_ceil(4);
        let desc = desc.as_bytes();
        State {
            w: params.w,
            h: params.h,
            moves: 0,
            completed: false,
            cheated: false,
            hints_active: false,
            grid: decode_bitmap(&desc[mlen + 1..], wh),
            matrix: decode_bitmap(desc, wh * wh).into(),
        }
    }

    // C: flip.c, solve_game()
    fn solve(_original: &State, current: &State, _aux: Option<&str>) -> Result<Move, GameError> {
        let wh = current.area();
        let width = wh + 1;

        // One equation per square: the clicks that toggle it, then
        // whether it needs toggling.
        let mut eq = vec![0_u8; width * wh];
        for (i, row) in eq.chunks_exact_mut(width).enumerate() {
            for (j, e) in row[..wh].iter_mut().enumerate() {
                *e = current.matrix[j * wh + i];
            }
            row[wh] = current.grid[i] & WRONG;
        }

        // Gaussian elimination over GF(2).
        let mut rows_done = 0;
        let mut cols_done = 0;
        let mut undetermined = Vec::new();
        loop {
            // The leftmost column with a 1 outside the rows done; columns
            // passed on the way have no equation controlling them.
            let mut pivot = None;
            for i in cols_done..wh {
                if let Some(j) = (rows_done..wh).find(|&j| eq[j * width + i] != 0) {
                    pivot = Some((i, j));
                    break;
                }
                undetermined.push(i);
            }

            let Some((i, j)) = pivot else {
                // Every remaining equation reads 0 = constant.
                if (rows_done..wh).any(|j| eq[j * width + wh] != 0) {
                    return Err(GameError::new("No solution exists for this position"));
                }
                break;
            };

            if j > rows_done {
                row_xor(&mut eq, width, rows_done, j);
            }
            for j in rows_done + 1..wh {
                if eq[j * width + i] != 0 {
                    row_xor(&mut eq, width, j, rows_done);
                }
            }
            rows_done += 1;
            cols_done = i + 1;
            if rows_done >= wh {
                break;
            }
        }

        // Try every value of the undetermined variables, keeping the
        // solution with the fewest clicks (the first one found on ties).
        let mut solution = vec![false; wh];
        let mut shortest = solution.clone();
        let mut best_len = wh + 1;
        loop {
            for j in (0..rows_done).rev() {
                let row = &eq[j * width..(j + 1) * width];
                let i = row[..wh]
                    .iter()
                    .position(|&e| e != 0)
                    .expect("a reduced equation has a leading 1");
                let mut v = row[wh] != 0;
                for k in i + 1..wh {
                    if row[k] != 0 {
                        v ^= solution[k];
                    }
                }
                solution[i] = v;
            }

            let len = solution.iter().filter(|&&s| s).count();
            if len < best_len {
                best_len = len;
                shortest.clone_from(&solution);
            }

            // Binary increment of the undetermined variables; wrapping
            // round to all zeros means every choice has been tried.
            let mut carried_out = true;
            for &u in &undetermined {
                solution[u] = !solution[u];
                if solution[u] {
                    carried_out = false;
                    break;
                }
            }
            if carried_out {
                break;
            }
        }

        Ok(Move::Solve(shortest))
    }

    fn text_format(state: &State) -> Option<String> {
        Some(text_format(state))
    }

    fn status(state: &State) -> GameStatus {
        if state.completed {
            GameStatus::Won
        } else {
            GameStatus::Ongoing
        }
    }

    fn new_ui(_state: &State) -> Ui {
        Ui {
            cx: 0,
            cy: 0,
            cdraw: env_bool("PUZZLES_SHOW_CURSOR").unwrap_or(false),
        }
    }

    fn current_key_label(_ui: &Ui, _state: &State, button: Button) -> Option<&'static str> {
        button.is_cursor_select().then_some("Flip")
    }

    fn interpret_move(
        state: &State,
        ui: &mut Ui,
        ds: &DrawState,
        x: i32,
        y: i32,
        button: Button,
    ) -> MoveResult<Move> {
        let left_click = button == Button::mouse(MouseButton::Left, MouseAction::Press);
        if left_click || button.is_cursor_select() {
            let (tx, ty) = if left_click {
                ui.cdraw = false;
                (ds.tile_at(x), ds.tile_at(y))
            } else {
                ui.cdraw = true;
                (ui.cx, ui.cy)
            };
            if (0..state.w).contains(&tx) && (0..state.h).contains(&ty) {
                // A typed-in game ID can have a square that toggles
                // nothing; clicking it isn't a move.
                let i = (ty * state.w + tx) as usize;
                return if (0..state.area()).any(|j| state.toggles(i, j)) {
                    MoveResult::Move(Move::Flip { x: tx, y: ty })
                } else {
                    MoveResult::NoEffect
                };
            }
            MoveResult::UiUpdate
        } else if button.is_cursor_move() {
            move_cursor(
                button,
                &mut ui.cx,
                &mut ui.cy,
                state.w,
                state.h,
                false,
                Some(&mut ui.cdraw),
            )
        } else {
            MoveResult::Unused
        }
    }

    fn execute_move(_ctx: &mut GameCtx, from: &State, mv: &Move) -> Option<State> {
        let wh = from.area();
        match mv {
            Move::Solve(hints) => {
                if hints.len() != wh {
                    return None;
                }
                let mut state = from.clone();
                state.hints_active = true;
                state.cheated = true;
                for (g, &hint) in state.grid.iter_mut().zip(hints) {
                    *g &= !HINT;
                    if hint {
                        *g |= HINT;
                    }
                }
                Some(state)
            }
            &Move::Flip { x, y } => {
                if !(0..from.w).contains(&x) || !(0..from.h).contains(&y) {
                    return None;
                }
                let mut state = from.clone();
                if !state.completed {
                    state.moves += 1;
                }
                let i = (y * state.w + x) as usize;
                let row = &state.matrix[i * wh..(i + 1) * wh];
                for (g, &m) in state.grid.iter_mut().zip(row) {
                    *g ^= m;
                }
                let done = state.grid.iter().all(|&g| g & WRONG == 0);
                state.grid[i] ^= HINT;
                if done {
                    state.completed = true;
                    state.hints_active = false;
                }
                Some(state)
            }
        }
    }

    fn encode_move(mv: &Move, out: &mut String) {
        match mv {
            Move::Flip { x, y } => {
                let _ = write!(out, "M{x},{y}");
            }
            Move::Solve(hints) => {
                out.push('S');
                out.extend(hints.iter().map(|&h| if h { '1' } else { '0' }));
            }
        }
    }

    fn decode_move(state: &State, text: &str) -> Result<Move, MoveParseError> {
        // As C's execute_move: `S` with exactly one byte per square, or
        // `sscanf("M%d,%d")` (trailing text ignored) in range.
        let bytes = text.as_bytes();
        match bytes.split_first() {
            Some((b'S', hints)) if hints.len() == state.area() => {
                Ok(Move::Solve(hints.iter().map(|&b| b != b'0').collect()))
            }
            Some((b'M', rest)) => {
                let (x, rest) = scan_int(rest).ok_or(MoveParseError)?;
                let rest = rest.strip_prefix(b",").ok_or(MoveParseError)?;
                let (y, _) = scan_int(rest).ok_or(MoveParseError)?;
                ((0..state.w).contains(&x) && (0..state.h).contains(&y))
                    .then_some(Move::Flip { x, y })
                    .ok_or(MoveParseError)
            }
            _ => Err(MoveParseError),
        }
    }

    fn compute_size(params: &Params, tilesize: i32, _ui: &Ui) -> (i32, i32) {
        let border = tilesize / 2;
        (
            tilesize * params.w + 2 * border,
            tilesize * params.h + 2 * border,
        )
    }

    fn new_drawstate(state: &State) -> DrawState {
        DrawState {
            w: state.w,
            h: state.h,
            started: false,
            tiles: vec![None; state.area()],
            tilesize: 0,
        }
    }

    fn set_size(ds: &mut DrawState, _params: &Params, tilesize: i32) {
        ds.tilesize = tilesize;
    }

    // C: flip.c, game_redraw()
    fn redraw(
        dr: &mut Drawing<'_>,
        ds: &mut DrawState,
        old: Option<&State>,
        state: &State,
        _dir: i32,
        ui: &Ui,
        anim_time: f32,
        flash_time: f32,
    ) {
        let (w, h, ts) = (ds.w, ds.h, ds.tilesize);
        let border = ds.border();

        if !ds.started {
            for i in 0..=w {
                dr.line(
                    i * ts + border,
                    border,
                    i * ts + border,
                    h * ts + border,
                    Col::Grid,
                );
            }
            for i in 0..=h {
                dr.line(
                    border,
                    i * ts + border,
                    w * ts + border,
                    i * ts + border,
                    Col::Grid,
                );
            }
            dr.update(0, 0, ts * w + 2 * border, ts * h + 2 * border);
            ds.started = true;
        }

        let flash_frame = if flash_time == 0.0 {
            -1
        } else {
            (flash_time / FLASH_FRAME) as i32
        };
        // From 0 to 1 over the animation.
        let anim_time = anim_time / ANIM_TIME;

        for (i, &cell) in state.grid.iter().enumerate() {
            let (x, y) = ((i as i32) % w, (i as i32) / w);
            let mut v = cell;

            if flash_frame >= 0 {
                // Rings from the centre outwards light up in turn.
                let fx = (w + 1) / 2 - (x + 1).min(w - x);
                let fy = (h + 1) / 2 - (y + 1).min(h - y);
                let fd = fx.max(fy);
                if fd == flash_frame {
                    v |= WRONG;
                } else if fd == flash_frame - 1 {
                    v &= !WRONG;
                }
            }
            if !state.hints_active {
                v &= !HINT;
            }
            if ui.cdraw && ui.cx == x && ui.cy == y {
                v |= CURSOR;
            }

            let animated = old.is_some_and(|old| (cell ^ old.grid[i]) & !HINT != 0);
            if animated || ds.tiles[i] != Some(v) {
                draw_tile(dr, ds, state, x, y, v, animated, anim_time);
                ds.tiles[i] = (!animated).then_some(v);
            }
        }

        let prefix = match (state.completed, state.cheated) {
            (true, true) => "Auto-solved. ",
            (true, false) => "COMPLETED! ",
            (false, true) => "Auto-solver used. ",
            (false, false) => "",
        };
        dr.status_bar(&format!("{prefix}Moves: {}", state.moves));
    }

    fn anim_length(_old: &State, _new: &State, _dir: i32, _ui: &mut Ui) -> f32 {
        ANIM_TIME
    }

    fn flash_length(old: &State, new: &State, _dir: i32, _ui: &mut Ui) -> f32 {
        if !old.completed && new.completed {
            FLASH_FRAME * (((new.w + 1) / 2).max((new.h + 1) / 2) + 1) as f32
        } else {
            0.0
        }
    }

    fn cursor_location(ui: &Ui, ds: &DrawState, _state: &State, _params: &Params) -> Option<Rect> {
        ui.cdraw.then(|| Rect {
            x: ds.coord(ui.cx),
            y: ds.coord(ui.cy),
            w: ds.tilesize,
            h: ds.tilesize,
        })
    }
}

/// Each click toggles its square and the four neighbours.
fn crosses_matrix(params: &Params) -> Vec<u8> {
    let (w, _, wh) = params.dims();
    let mut matrix = vec![0_u8; wh * wh];
    for i in 0..wh {
        let (ix, iy) = (i % w, i / w);
        for j in 0..wh {
            let (jx, jy) = (j % w, j / w);
            matrix[i * wh + j] = u8::from(ix.abs_diff(jx) + iy.abs_diff(jy) <= 1);
        }
    }
    matrix
}

/// A candidate entry for the random matrix: clicking `(cx, cy)` would
/// also toggle `(x, y)` (C's `struct sq`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sq {
    cx: i32,
    cy: i32,
    x: i32,
    y: i32,
    /// How many clicks toggle `(x, y)` so far.
    coverage: i32,
    /// How many squares clicking `(cx, cy)` toggles so far.
    ominosize: i32,
}

/// Wrappers ordering [`Sq`] by C's three comparators. Each compares all
/// fields, so two entries are equal under one exactly when they are
/// under all.
macro_rules! sq_order {
    ($(#[$meta:meta])* $name:ident: $($field:ident),*) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct $name(Sq);

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> Ordering {
                ($(self.0.$field,)*).cmp(&($(other.0.$field,)*))
            }
        }

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
    };
}

sq_order!(
    /// For choosing the next entry: least covered, then smallest omino
    /// (C's `sqcmp_pick`).
    ByPick: coverage, ominosize, cy, cx, y, x
);
sq_order!(
    /// For updating coverage: by coverage and output square (C's
    /// `sqcmp_cov`).
    ByCoverage: coverage, y, x, ominosize, cy, cx
);
sq_order!(
    /// For updating omino sizes: by size and click square (C's
    /// `sqcmp_osize`).
    ByOminoSize: ominosize, cy, cx, coverage, y, x
);

/// The candidates, in C's three trees (which always hold the same
/// entries).
#[derive(Default)]
struct Candidates {
    pick: SortedSet<ByPick>,
    cov: SortedSet<ByCoverage>,
    osize: SortedSet<ByOminoSize>,
}

impl Candidates {
    fn insert(&mut self, sq: Sq) {
        // C's add234: an equal entry already present is kept.
        self.pick.find_or_insert(ByPick(sq));
        self.cov.find_or_insert(ByCoverage(sq));
        self.osize.find_or_insert(ByOminoSize(sq));
    }

    fn remove(&mut self, sq: Sq) {
        self.pick.remove_item(&ByPick(sq));
        self.cov.remove_item(&ByCoverage(sq));
        self.osize.remove_item(&ByOminoSize(sq));
    }

    /// C's `addsq`: offers `(x, y)` for click `(cx, cy)` if it is on the
    /// grid, within the click's 3 × 3 neighbourhood and not yet toggled.
    fn offer(&mut self, w: i32, h: i32, (cx, cy): (i32, i32), (x, y): (i32, i32), matrix: &[u8]) {
        if !(0..w).contains(&x) || !(0..h).contains(&y) {
            return;
        }
        if (x - cx).abs() > 1 || (y - cy).abs() > 1 {
            return;
        }
        let wh = (w * h) as usize;
        let (click, out) = ((cy * w + cx) as usize, (y * w + x) as usize);
        if matrix[click * wh + out] != 0 {
            return;
        }
        let mut sq = Sq {
            cx,
            cy,
            x,
            y,
            coverage: 0,
            ominosize: 0,
        };
        for i in 0..wh {
            if matrix[i * wh + out] != 0 {
                sq.coverage += 1;
            }
            if matrix[click * wh + i] != 0 {
                sq.ominosize += 1;
            }
        }
        self.insert(sq);
    }

    /// C's `addneighbours`: offers the four neighbours of `(x, y)`.
    fn offer_neighbours(
        &mut self,
        w: i32,
        h: i32,
        click: (i32, i32),
        (x, y): (i32, i32),
        matrix: &[u8],
    ) {
        for out in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            self.offer(w, h, click, out, matrix);
        }
    }
}

/// Random ominoes, grown one square at a time from the least covered
/// squares, until there are as many entries as crosses would have;
/// rejected and redone if two clicks would toggle the same squares.
fn random_matrix(params: &Params, rng: &mut RandomState) -> Vec<u8> {
    let (w, h) = (params.w, params.h);
    let (_, _, wh) = params.dims();
    let mut matrix = vec![0_u8; wh * wh];
    loop {
        let mut cands = Candidates::default();
        matrix.fill(0);
        for i in 0..wh {
            matrix[i * wh + i] = 1;
        }
        for i in 0..wh as i32 {
            let sq = (i % w, i / w);
            cands.offer_neighbours(w, h, sq, sq, &matrix);
        }

        // The centre squares are already in.
        let limit = 4 * wh as i32 - 2 * (w + h);
        for _ in 0..limit {
            // The last entry as good as the best one: same coverage and
            // omino size, every other field above any real value.
            let Some(&ByPick(best)) = cands.pick.first() else {
                break; // C would crash here; no valid size gets here
            };
            let big = wh as i32;
            let key = ByPick(Sq {
                cx: big,
                cy: big,
                x: big,
                y: big,
                ..best
            });
            let k = find_rel(&cands.pick, Relation::Lt, |e| e.cmp(&key))
                .expect("the best entry is below the key");

            let k = rng.upto(k as u32 + 1) as usize;
            let ByPick(sq) = cands.pick.remove_index(k);
            cands.cov.remove_item(&ByCoverage(sq));
            cands.osize.remove_item(&ByOminoSize(sq));

            let (click, out) = ((sq.cy * w + sq.cx) as usize, (sq.y * w + sq.x) as usize);
            matrix[click * wh + out] = 1;

            // Every entry toggling the same output square is now one
            // more covered.
            let key = ByCoverage(Sq {
                cx: -1,
                cy: -1,
                ominosize: -1,
                ..sq
            });
            while let Some(pos) = find_rel(&cands.cov, Relation::Gt, |e| e.cmp(&key)) {
                let ByCoverage(sq2) = cands.cov[pos];
                if sq2.coverage != sq.coverage || (sq2.x, sq2.y) != (sq.x, sq.y) {
                    break;
                }
                cands.remove(sq2);
                cands.insert(Sq {
                    coverage: sq2.coverage + 1,
                    ..sq2
                });
            }

            // Every entry for the same click now has a bigger omino.
            let key = ByOminoSize(Sq {
                x: -1,
                y: -1,
                coverage: -1,
                ..sq
            });
            while let Some(pos) = find_rel(&cands.osize, Relation::Gt, |e| e.cmp(&key)) {
                let ByOminoSize(sq2) = cands.osize[pos];
                if sq2.ominosize != sq.ominosize || (sq2.cx, sq2.cy) != (sq.cx, sq.cy) {
                    break;
                }
                cands.remove(sq2);
                cands.insert(Sq {
                    ominosize: sq2.ominosize + 1,
                    ..sq2
                });
            }

            // The omino can now grow from the new square.
            cands.offer_neighbours(w, h, (sq.cx, sq.cy), (sq.x, sq.y), &matrix);
        }

        let rows: Vec<&[u8]> = matrix.chunks_exact(wh).collect();
        let repeated = (0..wh).any(|i| (0..wh).any(|j| i != j && rows[i] == rows[j]));
        if !repeated {
            return matrix;
        }
    }
}

/// C's `encode_bitmap`: four entries per hex digit, high bit first.
fn encode_bitmap(bits: &[u8]) -> String {
    bits.chunks(4)
        .map(|chunk| {
            let v = chunk
                .iter()
                .enumerate()
                .filter(|&(_, &b)| b != 0)
                .fold(0, |v, (j, _)| v | (8 >> j));
            char::from(b"0123456789abcdef"[v])
        })
        .collect()
}

/// C's `decode_bitmap` of `len` entries; `hex` has been validated.
fn decode_bitmap(hex: &[u8], len: usize) -> Vec<u8> {
    let mut bits = vec![0_u8; len];
    for (chunk, &c) in bits.chunks_mut(4).zip(hex) {
        let v = char::from(c).to_digit(16).unwrap_or(0);
        for (j, b) in chunk.iter_mut().enumerate() {
            *b = u8::from(v & (8 >> j) != 0);
        }
    }
    bits
}

/// XORs equation `src` into equation `dst` (C's `rowxor`).
fn row_xor(eq: &mut [u8], width: usize, dst: usize, src: usize) {
    for k in 0..width {
        eq[dst * width + k] ^= eq[src * width + k];
    }
}

/// C's `game_text_format`: a box per square, with the square's shape
/// drawn in `#` (light off) or `.` (on).
fn text_format(state: &State) -> String {
    let (w, h) = (state.w as usize, state.h as usize);
    let wh = w * h;
    let (cw, ch) = (4, 4);
    let gw = w * cw + 2;
    let gh = h * ch + 1;
    let len = gw * gh;
    let (right, down) = (1, gw);
    let mut board = vec![b' '; len];

    for r in 0..h {
        for c in 0..w {
            let cell = r * ch * gw + c * cw;
            let center = cell + (ch / 2) * down + cw / 2 * right;
            let flip = if state.grid[r * w + c] & WRONG != 0 {
                b'#'
            } else {
                b'.'
            };
            let dys = -isize::from(r != 0)..=isize::from(r != h - 1);
            for dy in dys {
                let dxs = -isize::from(c != 0)..=isize::from(c != w - 1);
                for dx in dxs {
                    let out = (r as isize + dy) as usize * w + (c as isize + dx) as usize;
                    if state.matrix[(r * w + c) * wh + out] != 0 {
                        let at = center as isize + dy * down as isize + dx * right as isize;
                        board[at as usize] = flip;
                    }
                }
            }
            board[cell] = b'+';
            for dx in 1..cw {
                board[cell + dx * right] = b'-';
            }
            for dy in 1..ch {
                board[cell + dy * down] = b'|';
            }
        }
        board[r * ch * gw + gw - 2] = b'+';
        board[r * ch * gw + gw - 1] = b'\n';
        for dy in 1..ch {
            board[r * ch * gw + gw - 2 + dy * down] = b'|';
            board[r * ch * gw + gw - 1 + dy * down] = b'\n';
        }
    }
    board[len - gw..len - 2].fill(b'-');
    for c in 0..=w {
        board[len - gw + cw * c] = b'+';
    }
    board[len - 1] = b'\n';
    String::from_utf8(board).expect("the board is ASCII")
}

/// C's `draw_tile`: one square, flipping over diagonally if `anim`.
#[allow(clippy::too_many_arguments, reason = "mirrors C's draw_tile")]
fn draw_tile(
    dr: &mut Drawing<'_>,
    ds: &DrawState,
    state: &State,
    x: i32,
    y: i32,
    tile: u8,
    anim: bool,
    anim_time: f32,
) {
    let (w, h, ts) = (ds.w, ds.h, ds.tilesize);
    let wh = (w * h) as usize;
    let (bx, by) = (ds.coord(x), ds.coord(y));
    let dcol = if tile & CURSOR != 0 {
        Col::Cursor
    } else {
        Col::Diag
    };
    let face = if tile & WRONG != 0 {
        Col::Wrong
    } else {
        Col::Right
    };

    dr.clip(bx + 1, by + 1, ts - 1, ts - 1);
    dr.rect(
        bx + 1,
        by + 1,
        ts - 1,
        ts - 1,
        if anim { Col::Background } else { face },
    );

    if anim {
        // A polygon showing the square flipping over its diagonal.
        let d = (ts as f32 * anim_time) as i32;
        let coords = [
            (bx + ts, by),
            (bx + d, by + d),
            (bx, by + ts),
            (bx + ts - d, by + ts - d),
        ];
        // The old face shows for the first half.
        let colour = match (face, anim_time < 0.5) {
            (Col::Wrong, true) => Col::Right,
            (_, true) => Col::Wrong,
            (face, false) => face,
        };
        dr.polygon(&coords, Some(colour), Col::Grid);
    }

    // The shape: which squares this click toggles.
    let i = (y * w + x) as usize;
    for oi in 0..h {
        for oj in 0..w {
            if state.matrix[i * wh + (oi * w + oj) as usize] == 0 {
                continue;
            }
            let (ox, oy) = (oj - x, oi - y);
            let td = if ts / 16 == 0 { 1 } else { ts / 16 };
            let cx = (bx + ts / 2) + (2 * ox - 1) * td;
            let cy = (by + ts / 2) + (2 * oy - 1) * td;
            if ox == 0 && oy == 0 {
                dr.rect(cx, cy, 2 * td + 1, 2 * td + 1, dcol);
            } else {
                dr.line(cx, cy, cx + 2 * td, cy, dcol);
                dr.line(cx, cy + 2 * td, cx + 2 * td, cy + 2 * td, dcol);
                dr.line(cx, cy, cx, cy + 2 * td, dcol);
                dr.line(cx + 2 * td, cy, cx + 2 * td, cy + 2 * td, dcol);
            }
        }
    }

    if tile & HINT != 0 {
        let (mut x1, mut x2) = (bx + ts / 20, bx + ts - ts / 20);
        let (mut y1, mut y2) = (by + ts / 20, by + ts - ts / 20);
        for _ in 0..3 {
            dr.line(x1, y1, x2, y1, Col::Hint);
            dr.line(x1, y2, x2, y2, Col::Hint);
            dr.line(x1, y1, x1, y2, Col::Hint);
            dr.line(x2, y1, x2, y2, Col::Hint);
            x1 += 1;
            y1 += 1;
            x2 -= 1;
            y2 -= 1;
        }
    }

    dr.unclip();
    dr.update(bx + 1, by + 1, ts - 1, ts - 1);
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn params(id: &str) -> Params {
        let mut p = Flip::default_params();
        Flip::decode_params(&mut p, id);
        p
    }

    fn start(id: &str, seed: &[u8]) -> State {
        let p = params(id);
        let desc = Flip::new_desc(&p, &mut RandomState::new(seed), true).desc;
        Flip::new_game(&mut GameCtx::new(), &p, &desc)
    }

    fn encode(mv: &Move) -> String {
        let mut s = String::new();
        Flip::encode_move(mv, &mut s);
        s
    }

    /// Descriptions and solutions from C (checked by the oracle tests).
    #[test]
    fn golden_descriptions_and_solutions() {
        for (id, desc, solution) in [
            ("3x3c", "d074191345d1644c17058,f80", "S011011001"),
            (
                "4x4r",
                "c800e200720033008c80c460172031100c8808c80037013100cc004e00070013,1e5c",
                "S0111100100001000",
            ),
            ("2x3r", "f34a972cd,1c", "S101111"),
        ] {
            let p = params(id);
            assert_eq!(
                Flip::new_desc(&p, &mut RandomState::new(b"42"), true).desc,
                desc
            );
            let state = Flip::new_game(&mut GameCtx::new(), &p, desc);
            assert_eq!(
                Flip::solve(&state, &state, None)
                    .map(|m| encode(&m))
                    .as_deref(),
                Ok(solution)
            );
        }
    }

    #[test]
    fn text_format_draws_shapes() {
        let p = params("2x3r");
        let state = Flip::new_game(&mut GameCtx::new(), &p, "f34a972cd,1c");
        let expected = "\
+---+---+
|   |   |
| ..|.. |
| ..| . |
+---+---+
| . | # |
| . | # |
| . |## |
+---+---+
| # |## |
| ##| # |
|   |   |
+---+---+
";
        assert_eq!(Flip::text_format(&state).as_deref(), Some(expected));
    }

    #[test]
    fn params_parse_like_c() {
        assert_eq!(
            params("4"),
            Params {
                w: 4,
                h: 4,
                matrix_type: MatrixType::Crosses
            }
        );
        assert_eq!(
            params("3x5r"),
            Params {
                w: 3,
                h: 5,
                matrix_type: MatrixType::Random
            }
        );
        // C's atoi takes a sign, but the digit skip doesn't, so the `x`
        // is never reached.
        assert_eq!(params("-3x5").h, -3);
        assert_eq!(Flip::encode_params(&params("3x5r"), false), "3x5");
        assert!(Flip::validate_params(&params("0x5"), true).is_err());
        assert!(Flip::validate_params(&params("50000x50000"), true).is_err());
        assert!(Flip::validate_params(&params("300x300"), true).is_err());
    }

    #[test]
    fn moves_decode_leniently_like_sscanf() {
        let state = start("3x3c", b"1");
        let flip = Ok(Move::Flip { x: 1, y: 2 });
        assert_eq!(Flip::decode_move(&state, "M1,2"), flip);
        assert_eq!(Flip::decode_move(&state, "M 1, +2junk"), flip);
        for bad in ["", "M", "M1", "M1 ,2", "M3,0", "M-1,0", "S0101", "X1,1"] {
            assert_eq!(
                Flip::decode_move(&state, bad),
                Err(MoveParseError),
                "{bad:?}"
            );
        }
        assert_eq!(
            Flip::decode_move(&state, "S0x0000000"),
            Ok(Move::Solve(
                [false, true].into_iter().chain([false; 7]).collect()
            ))
        );
    }

    #[test]
    fn following_the_hints_wins() {
        for id in ["3x3c", "5x5c", "4x4r", "5x5r", "1x1r", "1x5r"] {
            let mut state = start(id, id.as_bytes());
            let Ok(Move::Solve(hints)) = Flip::solve(&state, &state, None) else {
                panic!("{id}: no solution");
            };
            state = Flip::execute_move(&mut GameCtx::new(), &state, &Move::Solve(hints.clone()))
                .expect("solve executes");
            assert!(state.cheated && state.hints_active);
            for (i, _) in hints.iter().enumerate().filter(|&(_, &h)| h) {
                let (x, y) = (i as i32 % state.w, i as i32 / state.w);
                state = Flip::execute_move(&mut GameCtx::new(), &state, &Move::Flip { x, y })
                    .expect("in range");
            }
            assert_eq!(Flip::status(&state), GameStatus::Won, "{id}");
            assert!(!state.hints_active);
        }
    }

    #[test]
    fn unsolvable_position_is_reported() {
        // Every click toggles only the first square, so the second can
        // never be fixed.
        let p = params("2x1");
        let state = Flip::new_game(&mut GameCtx::new(), &p, "a,4");
        assert_eq!(
            Flip::solve(&state, &state, None),
            Err(GameError::new("No solution exists for this position"))
        );
    }

    proptest! {
        #[test]
        fn move_codec_round_trips(x in 0..5_i32, y in 0..5_i32, hints in prop::collection::vec(any::<bool>(), 25)) {
            let state = start("5x5c", b"codec");
            for mv in [Move::Flip { x, y }, Move::Solve(hints.clone())] {
                prop_assert_eq!(Flip::decode_move(&state, &encode(&mv)), Ok(mv));
            }
        }

        /// Untrusted text never panics.
        #[test]
        fn untrusted_text_never_panics(id in "\\PC{0,12}", desc in "[0-9a-fA-F,g ]{0,40}", mv in "\\PC{0,12}") {
            let mut p = Flip::default_params();
            Flip::decode_params(&mut p, &id);
            let p = if Flip::validate_params(&p, true).is_ok() && p.w * p.h <= 64 {
                p
            } else {
                params("3x2")
            };
            if Flip::validate_desc(&p, &desc).is_ok() {
                let state = Flip::new_game(&mut GameCtx::new(), &p, &desc);
                if let Ok(mv) = Flip::decode_move(&state, &mv) {
                    let _ = Flip::execute_move(&mut GameCtx::new(), &state, &mv);
                }
                let _ = Flip::solve(&state, &state, None);
            }
        }
    }
}
