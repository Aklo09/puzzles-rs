//! Port of third_party/puzzles-c/fifteen.c (upstream 616da16).
//!
//! The 15-puzzle: slide numbered tiles into order through one gap. A
//! click (or arrow key) slides every tile between the gap and that
//! square; `h` makes one move of a simple solving strategy, and Solve
//! jumps to the solved position (as practice, not as a hint).
//!
//! Where C asserts in the hint strategy (possible only for a typed-in
//! position that can't be solved), the hint does nothing instead.

use std::fmt::Write as _;

use puzzles_core::button::{Button, ButtonKind, CursorKey, MouseAction, MouseButton};
use puzzles_core::config::{ConfigItem, ConfigValue};
use puzzles_core::drawing::{Align, Drawing, FontType};
use puzzles_core::game::{
    Game, GameCtx, GameError, GameStatus, MoveParseError, MoveResult, NewDesc, Rect,
};
use puzzles_core::misc::{env_bool, move_cursor};
use puzzles_core::palette::PaletteEntry;
use puzzles_core::parse::{atoi, scan_int, skip_digits};
use puzzles_core::preset::PresetMenu;
use puzzles_core::random::RandomState;

const PREFERRED_TILE_SIZE: i32 = 48;
/// Seconds a slide takes.
const ANIM_TIME: f32 = 0.13;
/// Seconds per frame of the completion flash.
const FLASH_FRAME: f32 = 0.13;

puzzles_core::palette! {
    /// Fifteen's colours: a bevelled background and black numbers.
    pub enum Col {
        Background => Role(Background),
        Text => Role(Ink),
        Highlight => Role(Highlight),
        Lowlight => Role(Lowlight),
    }
}

/// Fifteen's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Width in tiles.
    pub w: i32,
    /// Height in tiles.
    pub h: i32,
}

/// A position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    w: i32,
    h: i32,
    /// The tile in each square, 0 for the gap.
    tiles: Vec<i32>,
    gap_pos: usize,
    /// The move count when the puzzle was completed; 0 until then.
    completed: i32,
    /// Solve was used (no completion flash).
    used_solve: bool,
    movecount: i32,
}

impl State {
    fn n(&self) -> usize {
        self.tiles.len()
    }

    fn xy(&self, i: usize) -> (i32, i32) {
        let i = i as i32;
        (i % self.w, i / self.w)
    }
}

/// A move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// Move the gap to `(x, y)`, in its row or column, sliding the tiles
    /// in between: `M<x>,<y>`.
    Slide {
        /// Column.
        x: i32,
        /// Row.
        y: i32,
    },
    /// Jump to the solved position: `S`.
    Solve,
}

/// The UI: only a preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ui {
    /// Arrow keys move the gap rather than a tile (preference
    /// `arrow-semantics`).
    invert_cursor: bool,
}

/// What was last drawn.
#[derive(Clone, Debug)]
pub struct DrawState {
    started: bool,
    bgcolour: Col,
    /// Per square, the tile last drawn there, or `None` if it must be
    /// drawn again (never drawn, or mid-animation).
    tiles: Vec<Option<i32>>,
    tilesize: i32,
}

impl DrawState {
    fn border(&self) -> i32 {
        self.tilesize / 2
    }

    fn highlight_width(&self) -> i32 {
        self.tilesize / 20
    }

    /// C's `COORD`.
    fn coord(&self, x: i32) -> i32 {
        x * self.tilesize + self.border()
    }

    /// C's `FROMCOORD`.
    fn tile_at(&self, x: i32) -> i32 {
        (x - self.border() + self.tilesize) / self.tilesize - 1
    }
}

/// The Fifteen puzzle.
#[derive(Clone, Copy, Debug)]
pub struct Fifteen;

/// C's `perm_parity`: the parity of the number of inversions.
fn perm_parity(perm: &[i32]) -> bool {
    let mut odd = false;
    for i in 0..perm.len() {
        for j in i + 1..perm.len() {
            if perm[i] > perm[j] {
                odd = !odd;
            }
        }
    }
    odd
}

/// C's `PARITY_P`: the permutation parity a position with the gap at
/// `gap` must have to be solvable.
fn required_parity(w: i32, h: i32, gap: usize) -> bool {
    let gap = gap as i32;
    let (gx, gy) = (gap % w, gap / w);
    ((gx - (w - 1)) ^ (gy - (h - 1)) ^ (w * h + 1)) & 1 != 0
}

/// Whether the tiles read 1, 2, …, n - 1 with the gap last.
fn is_completed(tiles: &[i32]) -> bool {
    let n = tiles.len();
    tiles
        .iter()
        .enumerate()
        .all(|(p, &t)| t == if p < n - 1 { p as i32 + 1 } else { 0 })
}

impl Game for Fifteen {
    type Params = Params;
    type State = State;
    type Ui = Ui;
    type DrawState = DrawState;
    type Move = Move;

    const NAME: &'static str = "Fifteen";
    const PREFERRED_TILESIZE: i32 = PREFERRED_TILE_SIZE;
    const PALETTE: &'static [PaletteEntry] = Col::PALETTE;
    const CAN_CONFIGURE: bool = true;
    const CAN_SOLVE: bool = true;
    const CAN_FORMAT_AS_TEXT_EVER: bool = true;
    const WANTS_STATUSBAR: bool = true;

    fn default_params() -> Params {
        Params { w: 4, h: 4 }
    }

    fn presets() -> PresetMenu<Params> {
        let mut menu = PresetMenu::new();
        menu.add_preset("4x4", Self::default_params());
        menu
    }

    fn decode_params(params: &mut Params, s: &str) {
        let s = s.as_bytes();
        params.w = atoi(s);
        params.h = params.w;
        if let [b'x', rest @ ..] = skip_digits(s) {
            params.h = atoi(rest);
        }
    }

    fn encode_params(params: &Params, _full: bool) -> String {
        format!("{}x{}", params.w, params.h)
    }

    fn configure(params: &Params) -> Option<Vec<ConfigItem>> {
        Some(vec![
            ConfigItem::string("Width", params.w.to_string()),
            ConfigItem::string("Height", params.h.to_string()),
        ])
    }

    fn custom_params(config: &[ConfigItem]) -> Result<Params, GameError> {
        let wrong = || GameError::new("Unexpected configuration fields");
        let [w, h] = config else {
            return Err(wrong());
        };
        Ok(Params {
            w: atoi(w.as_str().ok_or_else(wrong)?.as_bytes()),
            h: atoi(h.as_str().ok_or_else(wrong)?.as_bytes()),
        })
    }

    fn validate_params(params: &Params, _full: bool) -> Result<(), GameError> {
        if params.w < 2 || params.h < 2 {
            return Err(GameError::new("Width and height must both be at least two"));
        }
        if params.w > i32::MAX / params.h {
            return Err(GameError::new(
                "Width times height must not be unreasonably large",
            ));
        }
        Ok(())
    }

    // C: fifteen.c, new_game_desc()
    fn new_desc(params: &Params, rng: &mut RandomState, _interactive: bool) -> NewDesc {
        let n = (params.w * params.h) as usize;
        let mut tiles = vec![-1_i32; n];
        let mut used = vec![false; n];
        loop {
            tiles.fill(-1);
            used.fill(false);

            let gap = rng.upto(n as u32) as usize;
            tiles[gap] = 0;
            used[0] = true;

            // Everything but the last two tiles, at random.
            let mut x = 0;
            for i in (3..n).rev() {
                let k = rng.upto(i as u32) as usize;
                let j = (0..n)
                    .filter(|&j| !used[j])
                    .nth(k)
                    .expect("k is below the number of unused tiles");
                used[j] = true;
                while tiles[x] >= 0 {
                    x += 1;
                }
                tiles[x] = j as i32;
            }

            // The last two squares and the last two tiles.
            while tiles[x] >= 0 {
                x += 1;
            }
            let x1 = x;
            x += 1;
            while tiles[x] >= 0 {
                x += 1;
            }
            let x2 = x;
            let p1 = (0..n).find(|&i| !used[i]).expect("two tiles left");
            let p2 = (p1 + 1..n).find(|&i| !used[i]).expect("two tiles left");

            // They go in whichever order makes the puzzle solvable.
            let parity = required_parity(params.w, params.h, gap);
            tiles[x1] = p1 as i32;
            tiles[x2] = p2 as i32;
            if perm_parity(&tiles) != parity {
                tiles[x1] = p2 as i32;
                tiles[x2] = p1 as i32;
                assert_eq!(perm_parity(&tiles), parity);
            }

            if !is_completed(&tiles) {
                break;
            }
        }

        let mut desc = String::new();
        for (i, t) in tiles.iter().enumerate() {
            if i > 0 {
                desc.push(',');
            }
            let _ = write!(desc, "{t}");
        }
        NewDesc::new(desc)
    }

    fn validate_desc(params: &Params, desc: &str) -> Result<(), GameError> {
        let area = (params.w * params.h) as usize;
        let desc = desc.as_bytes();
        let mut used = vec![false; area];
        let mut p = 0;
        for i in 0..area {
            let q = p;
            if !desc.get(p).is_some_and(u8::is_ascii_digit) {
                return Err(GameError::new("Not enough numbers in string"));
            }
            while desc.get(p).is_some_and(u8::is_ascii_digit) {
                p += 1;
            }
            if i < area - 1 && desc.get(p) != Some(&b',') {
                return Err(GameError::new("Expected comma after number"));
            } else if i == area - 1 && p < desc.len() {
                return Err(GameError::new("Excess junk at end of string"));
            }
            let n = atoi(&desc[q..]);
            let Some(slot) = usize::try_from(n).ok().filter(|&n| n < area) else {
                return Err(GameError::new("Number out of range"));
            };
            if used[slot] {
                return Err(GameError::new("Number used twice"));
            }
            used[slot] = true;
            if p < desc.len() {
                p += 1;
            }
        }
        Ok(())
    }

    fn new_game(_ctx: &mut GameCtx, params: &Params, desc: &str) -> State {
        let tiles: Vec<i32> = desc.split(',').map(|t| atoi(t.as_bytes())).collect();
        let gap_pos = tiles.iter().position(|&t| t == 0).unwrap_or(0);
        State {
            w: params.w,
            h: params.h,
            tiles,
            gap_pos,
            completed: 0,
            used_solve: false,
            movecount: 0,
        }
    }

    fn solve(_original: &State, _current: &State, _aux: Option<&str>) -> Result<Move, GameError> {
        Ok(Move::Solve)
    }

    fn text_format(state: &State) -> Option<String> {
        let col = (state.n() - 1).to_string().len();
        let mut out = String::with_capacity(state.n() * (col + 1));
        for (i, &v) in state.tiles.iter().enumerate() {
            if v == 0 {
                out.extend(std::iter::repeat_n(' ', col));
            } else {
                let _ = write!(out, "{v:>col$}");
            }
            out.push(if (i as i32 + 1) % state.w == 0 {
                '\n'
            } else {
                ' '
            });
        }
        Some(out)
    }

    fn status(state: &State) -> GameStatus {
        if state.completed != 0 {
            GameStatus::Won
        } else {
            GameStatus::Ongoing
        }
    }

    fn new_ui(_state: Option<&State>) -> Ui {
        // C reads the legacy variable once per process; it can't change
        // while a front end runs.
        Ui {
            invert_cursor: env_bool("FIFTEEN_INVERT_CURSOR").unwrap_or(false),
        }
    }

    fn prefs(ui: &Ui) -> Vec<ConfigItem> {
        vec![ConfigItem {
            name: "Sense of arrow keys",
            keyword: Some("arrow-semantics"),
            value: ConfigValue::Choices {
                names: &["Move the tile", "Move the gap"],
                keywords: Some(&["tile", "gap"]),
                selected: usize::from(ui.invert_cursor),
            },
        }]
    }

    fn set_prefs(ui: &mut Ui, config: &[ConfigItem]) {
        if let Some(selected) = config.first().and_then(ConfigItem::selected) {
            ui.invert_cursor = selected != 0;
        }
    }

    fn interpret_move(
        state: &State,
        ui: &mut Ui,
        ds: &DrawState,
        x: i32,
        y: i32,
        button: Button,
    ) -> MoveResult<Move> {
        let (cx, cy) = state.xy(state.gap_pos);
        let (mut nx, mut ny) = (cx, cy);
        let button = Button::new(button.kind); // C strips the modifiers

        match button.kind {
            ButtonKind::Mouse(MouseButton::Left, MouseAction::Press) => {
                nx = ds.tile_at(x);
                ny = ds.tile_at(y);
                if !(0..state.w).contains(&nx) || !(0..state.h).contains(&ny) {
                    return MoveResult::Unused;
                }
            }
            ButtonKind::Cursor(key) if button.is_cursor_move() => {
                // By default an arrow moves a tile, so the gap goes the
                // other way.
                let key = if ui.invert_cursor {
                    key
                } else {
                    flip_cursor(key)
                };
                let _: MoveResult<()> = move_cursor(
                    Button::cursor(key),
                    &mut nx,
                    &mut ny,
                    state.w,
                    state.h,
                    false,
                    None,
                );
            }
            ButtonKind::Key('h' | 'H') if state.completed == 0 => match compute_hint(state) {
                Some((hx, hy)) => (nx, ny) = (hx, hy),
                None => return MoveResult::NoEffect,
            },
            _ => return MoveResult::Unused,
        }

        // The target must share exactly one coordinate with the gap.
        if (cx == nx) ^ (cy == ny) {
            MoveResult::Move(Move::Slide { x: nx, y: ny })
        } else {
            MoveResult::NoEffect
        }
    }

    fn execute_move(_ctx: &mut GameCtx, from: &State, mv: &Move) -> Option<State> {
        let n = from.n();
        match *mv {
            Move::Solve => {
                let mut state = from.clone();
                for (i, t) in state.tiles.iter_mut().enumerate() {
                    *t = ((i + 1) % n) as i32;
                }
                state.gap_pos = n - 1;
                state.used_solve = true;
                state.completed = 1;
                state.movecount = 1;
                Some(state)
            }
            Move::Slide { x: dx, y: dy } => {
                let (gx, gy) = from.xy(from.gap_pos);
                if (dx == gx && dy == gy) || (dx != gx && dy != gy) {
                    return None;
                }
                if !(0..from.w).contains(&dx) || !(0..from.h).contains(&dy) {
                    return None;
                }
                let ux = (dx - gx).signum();
                let uy = (dy - gy).signum();
                let up = (uy * from.w + ux) as isize;

                let mut state = from.clone();
                state.gap_pos = (dy * from.w + dx) as usize;
                state.tiles[state.gap_pos] = 0;
                let mut p = from.gap_pos as isize;
                while p != state.gap_pos as isize {
                    state.tiles[p as usize] = from.tiles[(p + up) as usize];
                    state.movecount += 1;
                    p += up;
                }
                if state.completed == 0 && is_completed(&state.tiles) {
                    state.completed = state.movecount;
                }
                Some(state)
            }
        }
    }

    fn encode_move(mv: &Move, out: &mut String) {
        match mv {
            Move::Slide { x, y } => {
                let _ = write!(out, "M{x},{y}");
            }
            Move::Solve => out.push('S'),
        }
    }

    fn decode_move(state: &State, text: &str) -> Result<Move, MoveParseError> {
        // As C's execute_move: exactly `S`, or `sscanf("M%d,%d")`
        // (trailing text ignored) on the grid.
        if text == "S" {
            return Ok(Move::Solve);
        }
        let rest = text.as_bytes().strip_prefix(b"M").ok_or(MoveParseError)?;
        let (x, rest) = scan_int(rest).ok_or(MoveParseError)?;
        let (y, _) =
            scan_int(rest.strip_prefix(b",").ok_or(MoveParseError)?).ok_or(MoveParseError)?;
        if (0..state.w).contains(&x) && (0..state.h).contains(&y) {
            Ok(Move::Slide { x, y })
        } else {
            Err(MoveParseError)
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
            started: false,
            bgcolour: Col::Background,
            tiles: vec![None; state.n()],
            tilesize: 0,
        }
    }

    fn set_size(ds: &mut DrawState, _params: &Params, tilesize: i32) {
        ds.tilesize = tilesize;
    }

    // C: fifteen.c, game_redraw()
    fn redraw(
        dr: &mut Drawing<'_>,
        ds: &mut DrawState,
        old: Option<&State>,
        state: &State,
        _dir: i32,
        _ui: &Ui,
        anim_time: f32,
        flash_time: f32,
    ) {
        let ts = ds.tilesize;
        let hw = ds.highlight_width();
        let bgcolour = if flash_time > 0.0 {
            let frame = (flash_time / FLASH_FRAME) as i32;
            if frame % 2 != 0 {
                Col::Lowlight
            } else {
                Col::Highlight
            }
        } else {
            Col::Background
        };

        if !ds.started {
            // The recessed area holding the puzzle.
            let right = ds.coord(state.w) + hw - 1;
            let bottom = ds.coord(state.h) + hw - 1;
            let left = ds.coord(0) - hw;
            let top = ds.coord(0) - hw;
            let mut coords = [
                (right, bottom),
                (right, top),
                (right - ts, top + ts),
                (left + ts, bottom - ts),
                (left, bottom),
            ];
            dr.polygon(&coords, Some(Col::Highlight), Col::Highlight);
            coords[0] = (left, top);
            dr.polygon(&coords, Some(Col::Lowlight), Col::Lowlight);
            ds.started = true;
        }

        // Two passes, so moving tiles are drawn over the blanked squares.
        for pass in 0..2 {
            for i in 0..state.n() {
                // None: a square in transition, always drawn.
                let t = match old {
                    Some(old) if old.tiles[i] != state.tiles[i] => None,
                    _ => Some(state.tiles[i]),
                };
                if ds.bgcolour != bgcolour || ds.tiles[i].is_none() || ds.tiles[i] != t {
                    // As in C, a square skipped in this pass keeps its
                    // old record, so the next pass still draws it.
                    let (sx, sy) = state.xy(i);
                    let (x, y, tile) = match t {
                        None if pass == 0 => (ds.coord(sx), ds.coord(sy), 0),
                        None => {
                            let tile = state.tiles[i];
                            if tile == 0 {
                                continue; // the gap needn't move
                            }
                            let old = old.expect("a transition has an old state");
                            let j = old
                                .tiles
                                .iter()
                                .position(|&o| o == tile)
                                .expect("the tile was somewhere");
                            let (ox, oy) = state.xy(j);
                            let (x0, y0) = (ds.coord(ox), ds.coord(oy));
                            let (x1, y1) = (ds.coord(sx), ds.coord(sy));
                            let c = (anim_time / ANIM_TIME).clamp(0.0, 1.0);
                            (
                                x0 + (c * (x1 - x0) as f32) as i32,
                                y0 + (c * (y1 - y0) as f32) as i32,
                                tile,
                            )
                        }
                        Some(_) if pass == 0 => continue,
                        Some(tile) => (ds.coord(sx), ds.coord(sy), tile),
                    };
                    draw_tile(dr, ts, x, y, tile, bgcolour);
                }
                ds.tiles[i] = t;
            }
        }
        ds.bgcolour = bgcolour;

        // The new status only once the animation shows the new state.
        let shown = old.unwrap_or(state);
        let status = if shown.used_solve {
            format!(
                "Moves since auto-solve: {}",
                shown.movecount - shown.completed
            )
        } else if shown.completed != 0 {
            format!("COMPLETED! Moves: {}", shown.completed)
        } else {
            format!("Moves: {}", shown.movecount)
        };
        dr.status_bar(&status);
    }

    fn anim_length(_old: &State, _new: &State, _dir: i32, _ui: &mut Ui) -> f32 {
        ANIM_TIME
    }

    fn flash_length(old: &State, new: &State, _dir: i32, _ui: &mut Ui) -> f32 {
        if old.completed == 0 && new.completed != 0 && !old.used_solve && !new.used_solve {
            2.0 * FLASH_FRAME
        } else {
            0.0
        }
    }

    fn cursor_location(_ui: &Ui, ds: &DrawState, state: &State, _params: &Params) -> Option<Rect> {
        let (gx, gy) = state.xy(state.gap_pos);
        Some(Rect {
            x: ds.coord(gx),
            y: ds.coord(gy),
            w: ds.tilesize,
            h: ds.tilesize,
        })
    }
}

/// C's `flip_cursor`: the opposite arrow.
fn flip_cursor(key: CursorKey) -> CursorKey {
    match key {
        CursorKey::Up => CursorKey::Down,
        CursorKey::Down => CursorKey::Up,
        CursorKey::Left => CursorKey::Right,
        CursorKey::Right => CursorKey::Left,
        other => other,
    }
}

/// C's `draw_tile`: a bevelled tile with its number, or the gap.
fn draw_tile(dr: &mut Drawing<'_>, ts: i32, x: i32, y: i32, tile: i32, flash_colour: Col) {
    if tile == 0 {
        dr.rect(x, y, ts, ts, flash_colour);
    } else {
        let mut coords = [(x + ts - 1, y + ts - 1), (x + ts - 1, y), (x, y + ts - 1)];
        dr.polygon(&coords, Some(Col::Lowlight), Col::Lowlight);
        coords[0] = (x, y);
        dr.polygon(&coords, Some(Col::Highlight), Col::Highlight);
        let hw = ts / 20;
        dr.rect(x + hw, y + hw, ts - 2 * hw, ts - 2 * hw, flash_colour);
        dr.text(
            x + ts / 2,
            y + ts / 2,
            FontType::Variable,
            ts / 3,
            Align::CENTRE,
            Col::Text,
            &tile.to_string(),
        );
    }
    dr.update(x, y, ts, ts);
}

// ----------------------------------------------------------------------
// The hint strategy (C's `compute_hint` and helpers)

/// C's `next_move_3x2`: the hard-coded shortest move for the last two
/// tiles of a row, with `a` going top left, `b` bottom left and the gap
/// at `g` in a 3 × 2 box.
fn next_move_3x2(a: (i32, i32), b: (i32, i32), g: (i32, i32)) -> Option<(i32, i32)> {
    #[rustfmt::skip]
    const MOVE: [u8; 120] = [
        1,2,0,1,2,2, 2,0,0,2,0,0, 0,0,2,0,2,0, 0,0,0,2,0,2, 2,0,0,0,2,0,
        0,3,0,1,1,1, 3,0,3,2,1,2, 2,1,1,0,1,0, 2,1,2,1,0,1, 1,2,0,2,1,2,
        0,1,3,1,3,0, 1,3,1,3,0,3, 0,0,3,3,0,0, 0,0,0,1,2,1, 3,0,0,1,1,1,
        3,1,1,1,3,0, 1,1,1,1,1,1, 1,3,1,1,3,0, 1,1,3,3,1,3, 1,3,0,0,0,0,
    ];
    const D: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let ea = 3 * a.1 + a.0;
    let mut eb = 3 * b.1 + b.0;
    let mut eg = 3 * g.1 + g.0;
    if eb > ea {
        eb -= 1;
    }
    if eg > ea {
        eg -= 1;
    }
    if eg > eb {
        eg -= 1;
    }
    let index = usize::try_from(ea + eb * 6 + eg * 5 * 6).ok()?;
    Some(D[usize::from(*MOVE.get(index)?)])
}

/// C's `next_move`: the gap's next step `(dx, dy)` to bring the piece at
/// `n` to its target `t`, with the following piece at `o` and the gap
/// at `g`; `w` is the width. `None` where C would assert.
#[allow(clippy::too_many_arguments, reason = "mirrors C's next_move")]
#[allow(clippy::many_single_char_names, reason = "C's names")]
fn next_move(
    (nx, ny): (i32, i32),
    (ox, oy): (i32, i32),
    (gx, gy): (i32, i32),
    (tx, ty): (i32, i32),
    w: i32,
) -> Option<(i32, i32)> {
    let to_tile_x = if gx < nx { 1 } else { -1 };
    let to_goal_x = if gx < tx { 1 } else { -1 };
    let gap_x_on_goal_side = (nx - tx) * (nx - gx) > 0;

    // C's assertions: not in place yet, not the gap, and above the
    // target row (the strategy is greedy).
    if (nx == tx && ny == ty) || (nx == gx && ny == gy) || ty > ny || ty > gy {
        return None;
    }

    // The end of a row, with its last two pieces in the top right 3 × 2.
    if tx == w - 2
        && ny <= ty + 2
        && (nx == tx || nx == tx + 1)
        && oy <= ty + 2
        && (ox == tx || ox == tx + 1)
        && gy <= ty + 2
        && (gx == tx || gx == tx + 1)
    {
        let (dy, dx) = next_move_3x2(
            (oy - ty, tx + 1 - ox),
            (ny - ty, tx + 1 - nx),
            (gy - ty, tx + 1 - gx),
        )?;
        return Some((-dx, dy));
    }

    if tx == w - 1 {
        return if ny <= ty + 2
            && (nx == tx || nx == tx - 1)
            && gy <= ty + 2
            && (gx == tx || gx == tx - 1)
        {
            let (dy, dx) = next_move_3x2((ny - ty, tx - nx), (0, 1), (gy - ty, tx - gx))?;
            Some((-dx, dy))
        } else if gy == ty {
            Some((0, 1))
        } else if nx != tx || ny != ty + 1 {
            let (dx, dy) = next_move(
                ((w - 1) - nx, ny),
                (-1, -1),
                ((w - 1) - gx, gy),
                (0, ty + 1),
                -1,
            )?;
            Some((-dx, dy))
        } else if gx == nx {
            Some((0, -1))
        } else {
            Some((1, 0))
        };
    }

    // Moving the gap up is unsafe when gy = ty + 1 and gx < tx.
    let (dx, dy) = if gy < ny {
        if nx == gx || (gy == ty && gx == tx) {
            (0, 1)
        } else if !gap_x_on_goal_side || ny - ty > (nx - tx).abs() {
            (to_tile_x, 0)
        } else {
            (0, 1)
        }
    } else if gy == ny {
        if nx == tx {
            // Then ny > ty.
            if gx > nx || ny > ty + 1 {
                (0, -1)
            } else {
                (0, 1)
            }
        } else if gap_x_on_goal_side {
            (to_tile_x, 0)
        } else if gy == ty || (gy == ty + 1 && gx < tx) {
            (0, 1)
        } else {
            (0, -1)
        }
    } else if nx == tx {
        // gy > ny.
        if gx > nx { (0, -1) } else { (1, 0) }
    } else if gx == nx {
        (to_goal_x, 0)
    } else if gap_x_on_goal_side {
        if gy == ty + 1 && gx < tx {
            (to_tile_x, 0)
        } else {
            (0, -1)
        }
    } else if ny - ty > (nx - tx).abs() {
        (0, -1)
    } else {
        (to_tile_x, 0)
    };
    Some((dx, dy))
}

/// C's `compute_hint`: where the gap should go next, filling the top row
/// or left column (whichever has more left to do) one piece at a time;
/// `None` if solved, or where C would assert.
fn compute_hint(state: &State) -> Option<(i32, i32)> {
    let (w, h) = (state.w, state.h);
    let n = state.n() as i32;
    let (gx, gy) = state.xy(state.gap_pos);

    let (mut next_piece, mut next_piece_2) = (0, 0);
    let (mut solr, mut solc) = (0, 0);
    let (mut unsolved_rows, mut unsolved_cols) = (h, w);
    while solr < h && solc < w {
        let rows_first = unsolved_cols <= unsolved_rows;
        let (step, stop) = if rows_first {
            (1, unsolved_cols)
        } else {
            (w, unsolved_rows)
        };
        let start = solr * w + solc;
        if let Some(j) = (0..stop)
            .map(|i| start + i * step)
            .find(|&j| state.tiles[j as usize] != j + 1)
        {
            next_piece = j + 1;
            next_piece_2 = next_piece + step;
            break;
        }
        if rows_first {
            solr += 1;
            unsolved_rows -= 1;
        } else {
            solc += 1;
            unsolved_cols -= 1;
        }
    }
    if next_piece == n {
        return None;
    }

    let at = |piece: i32| {
        let i = state
            .tiles
            .iter()
            .position(|&t| t == piece)
            .unwrap_or(state.n()) as i32;
        (i % w, i / w)
    };
    let target = ((next_piece - 1) % w, (next_piece - 1) / w);
    let (nx, ny) = at(next_piece);
    let (ox, oy) = at(next_piece_2);

    let (dx, dy) = if unsolved_cols <= unsolved_rows {
        next_move((nx, ny), (ox, oy), (gx, gy), target, w)?
    } else {
        let (dy, dx) = next_move((ny, nx), (oy, ox), (gy, gx), (target.1, target.0), h)?;
        (dx, dy)
    };
    if dx == 0 && dy == 0 {
        return None;
    }
    Some((gx + dx, gy + dy))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn start(id: &str, seed: &[u8]) -> State {
        let mut p = Fifteen::default_params();
        Fifteen::decode_params(&mut p, id);
        let desc = Fifteen::new_desc(&p, &mut RandomState::new(seed), true).desc;
        Fifteen::new_game(&mut GameCtx::new(), &p, &desc)
    }

    fn hint_moves(mut state: State) -> State {
        for _ in 0..5 * state.n().pow(3) {
            if state.completed != 0 {
                break;
            }
            let (x, y) = compute_hint(&state).expect("a hint");
            state = Fifteen::execute_move(&mut GameCtx::new(), &state, &Move::Slide { x, y })
                .expect("hints are legal");
        }
        state
    }

    #[test]
    fn hints_solve_generated_puzzles() {
        for (id, seeds) in [
            ("4x4", 20),
            ("3x3", 20),
            ("2x2", 10),
            ("5x3", 10),
            ("3x5", 10),
            ("6x6", 5),
        ] {
            for seed in 0..seeds {
                let state = hint_moves(start(id, format!("{seed}").as_bytes()));
                assert_eq!(Fifteen::status(&state), GameStatus::Won, "{id} seed {seed}");
            }
        }
    }

    #[test]
    fn text_format_pads_numbers() {
        let p = Params { w: 4, h: 4 };
        let state = Fifteen::new_game(
            &mut GameCtx::new(),
            &p,
            "1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,0",
        );
        assert_eq!(
            Fifteen::text_format(&state).unwrap(),
            " 1  2  3  4\n 5  6  7  8\n 9 10 11 12\n13 14 15   \n"
        );
    }

    #[test]
    fn descriptions_validate_like_c() {
        let p = Params { w: 2, h: 2 };
        for (desc, err) in [
            ("1,2,3,0", None),
            ("1,2,3", Some("Expected comma after number")),
            ("1,2,3,", Some("Not enough numbers in string")),
            ("1,2,3 0", Some("Expected comma after number")),
            ("1,2,3,0,", Some("Excess junk at end of string")),
            ("1,2,3,4", Some("Number out of range")),
            ("1,1,3,0", Some("Number used twice")),
            ("", Some("Not enough numbers in string")),
        ] {
            assert_eq!(
                Fifteen::validate_desc(&p, desc).err().map(|e| e.0),
                err.map(Into::into),
                "{desc:?}"
            );
        }
    }

    #[test]
    fn slides_move_every_tile_between() {
        let p = Params { w: 3, h: 2 };
        let state = Fifteen::new_game(&mut GameCtx::new(), &p, "0,1,2,3,4,5");
        let next = Fifteen::execute_move(&mut GameCtx::new(), &state, &Move::Slide { x: 2, y: 0 })
            .unwrap();
        assert_eq!(next.tiles, [1, 2, 0, 3, 4, 5]);
        assert_eq!(next.movecount, 2);
        assert!(
            Fifteen::execute_move(&mut GameCtx::new(), &state, &Move::Slide { x: 1, y: 1 })
                .is_none()
        );
        assert!(
            Fifteen::execute_move(&mut GameCtx::new(), &state, &Move::Slide { x: 0, y: 0 })
                .is_none()
        );
    }

    #[test]
    fn solve_jumps_to_the_end_without_a_flash() {
        let state = start("3x3", b"s");
        let solved = Fifteen::execute_move(&mut GameCtx::new(), &state, &Move::Solve).unwrap();
        assert_eq!(Fifteen::status(&solved), GameStatus::Won);
        let mut ui = Fifteen::new_ui(None);
        assert!(Fifteen::flash_length(&state, &solved, 1, &mut ui).abs() < f32::EPSILON);
    }

    proptest! {
        #[test]
        fn move_codec_round_trips(x in 0..5_i32, y in 0..5_i32) {
            let state = start("5x5", b"codec");
            for mv in [Move::Slide { x, y }, Move::Solve] {
                let mut s = String::new();
                Fifteen::encode_move(&mv, &mut s);
                prop_assert_eq!(Fifteen::decode_move(&state, &s), Ok(mv));
            }
        }

        /// Untrusted text never panics; nor does a hint on any valid
        /// position, solvable or not.
        #[test]
        fn untrusted_text_and_hints_never_panic(
            desc in "[0-9,]{0,20}",
            mv in "\\PC{0,8}",
            perm in Just((0..9).collect::<Vec<i32>>()).prop_shuffle(),
        ) {
            let p = Params { w: 3, h: 3 };
            if Fifteen::validate_desc(&p, &desc).is_ok() {
                let state = Fifteen::new_game(&mut GameCtx::new(), &p, &desc);
                if let Ok(mv) = Fifteen::decode_move(&state, &mv) {
                    let _ = Fifteen::execute_move(&mut GameCtx::new(), &state, &mv);
                }
            }
            let desc = perm.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
            let state = Fifteen::new_game(&mut GameCtx::new(), &p, &desc);
            let _ = compute_hint(&state);
        }
    }
}
