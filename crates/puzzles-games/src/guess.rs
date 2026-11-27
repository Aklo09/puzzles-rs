//! Port of third_party/puzzles-c/guess.c (upstream 616da16).
//!
//! Guess, a Mastermind clone: find the hidden row of coloured pegs.
//! Each guess is marked with a black peg per right colour in the right
//! place and a white one per right colour in the wrong place. Pegs are
//! dragged from the colour bar (or typed, or placed with the cursor);
//! `h` fills in the first row consistent with the marks so far, and
//! right-click or space holds a peg for the next guess.
//!
//! As in C, keypad digits (which carry the keypad flag) don't place pegs,
//! and with 10 colours `:` places colour 10 as well as `0`.
//!
//! One deliberate difference: C submits the guess on Enter with the
//! cursor on the marking pegs even when the guess is no longer
//! complete, and its mid-end then aborts on the rejected move; here
//! Enter only shows the cursor then.

use std::fmt::Write as _;

use puzzles_core::button::{Button, ButtonKind, CursorKey, MouseAction, MouseButton};
use puzzles_core::config::{ConfigItem, ConfigValue};
use puzzles_core::drawing::{Align, Blitter, ColourIdx, Drawing, FontType};
use puzzles_core::game::{
    Game, GameCtx, GameError, GameStatus, MoveParseError, MoveResult, NewDesc, Rect,
};
use puzzles_core::misc::{
    bytes_to_hex, deobfuscate_bitmap, env_bool, hex_to_bytes, move_cursor, obfuscate_bitmap,
};
use puzzles_core::palette::PaletteEntry;
use puzzles_core::parse::{atoi, skip_digits};
use puzzles_core::preset::PresetMenu;
use puzzles_core::random::RandomState;

/// Peg size in pixels at 100% (C's `PEG_PREFER_SZ`).
const PEG_PREFER_SZ: i32 = 32;
/// Gaps and hint pegs as fractions of a peg (C's `PEG_GAP`, `PEG_HINT`).
const PEG_GAP: f64 = 0.10;
const PEG_HINT: f64 = 0.35;
/// The border, in pegs (C's `BORDER`).
const BORDER: f64 = 0.5;

/// Marks: a right colour in the right place, or in the wrong place.
const FEEDBACK_CORRECTPLACE: i32 = 1;
const FEEDBACK_CORRECTCOLOUR: i32 = 2;

/// Flags in the draw state's record of a peg.
const PEG_CURSOR: i32 = 0x1000;
const PEG_HOLD: i32 = 0x2000;
const PEG_LABELLED: i32 = 0x4000;
const PEG_FLAGS: i32 = PEG_CURSOR | PEG_HOLD | PEG_LABELLED;

puzzles_core::palette! {
    /// Guess's colours: the board, the peg colours (a series), and the
    /// black and white marking pegs.
    pub enum Col {
        Background => Role(Background),
        Frame => Role(Ink),
        Cursor => Role(Cursor),
        /// The empty marking pegs of a guess ready to be marked.
        Markable => Specific("markable"),
        /// Held pegs, and the current row's marker.
        Hold => Specific("hold"),
        /// An empty peg hole; peg colour n is `Empty + n`.
        Empty => Derived(Background, Shade(2.0 / 3.0)),
        Peg1 => Series(Pieces, 0),
        Peg2 => Series(Pieces, 1),
        Peg3 => Series(Pieces, 2),
        Peg4 => Series(Pieces, 3),
        Peg5 => Series(Pieces, 4),
        Peg6 => Series(Pieces, 5),
        Peg7 => Series(Pieces, 6),
        Peg8 => Series(Pieces, 7),
        Peg9 => Series(Pieces, 8),
        Peg10 => Series(Pieces, 9),
        CorrectPlace => Specific("correct_place"),
        CorrectColour => Specific("correct_colour"),
    }
}

/// The colour index of peg colour `col` (0 for an empty hole).
fn peg_colour(col: i32) -> ColourIdx {
    Col::Empty as usize + col as usize
}

/// Guess's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Colours to choose from (2 to 10).
    pub ncolours: i32,
    /// Pegs per row.
    pub npegs: i32,
    /// Guesses allowed.
    pub nguesses: i32,
    /// Guesses may leave holes empty.
    pub allow_blank: bool,
    /// The hidden row may repeat a colour.
    pub allow_multiple: bool,
}

/// A row of pegs and its marks (C's `pegrow`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct PegRow {
    /// Colours, 0 for empty.
    pegs: Vec<i32>,
    /// Marks, 0 for none.
    feedback: Vec<i32>,
}

impl PegRow {
    fn new(n: usize) -> Self {
        Self {
            pegs: vec![0; n],
            feedback: vec![0; n],
        }
    }

    /// C's `invalidate_pegrow`: matches nothing, so all is redrawn.
    fn invalid(n: usize) -> Self {
        Self {
            pegs: vec![-1; n],
            feedback: vec![-1; n],
        }
    }
}

/// A position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    params: Params,
    guesses: Vec<PegRow>,
    /// The holds of the last guess.
    holds: Vec<bool>,
    solution: PegRow,
    /// The guess being made; `nguesses` once they have run out.
    next_go: usize,
    /// Won, or lost (the solution is shown, also after Solve).
    solved: GameStatus,
}

/// A move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Move {
    /// Make a guess: `G` and a peg per hole, comma-separated, each with
    /// `_` if held (`G1,2_,3,4`).
    Guess {
        /// Colours, 0 for empty.
        pegs: Vec<i32>,
        /// Held pegs.
        holds: Vec<bool>,
    },
    /// Reveal the solution: `S`.
    Solve,
}

/// The UI: the guess being built, cursors, dragging, and a preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ui {
    params: Params,
    curr_pegs: PegRow,
    holds: Vec<bool>,
    /// The colour bar cursor.
    colour_cur: i32,
    /// The peg cursor; `npegs` is the marking pegs (submit).
    peg_cur: i32,
    display_cur: bool,
    markable: bool,
    /// The colour being dragged (0 for none), and the peg centre.
    drag_col: i32,
    drag_x: i32,
    drag_y: i32,
    /// The peg it was dragged from, if from the current guess.
    drag_opeg: Option<usize>,
    /// Label the colours with numbers (preference `show-labels`).
    show_labels: bool,
    /// The hint search's progress, kept between hints.
    hint: Option<PegRow>,
}

/// What was last drawn, and the layout.
#[derive(Debug)]
pub struct DrawState {
    nguesses: usize,
    guesses: Vec<PegRow>,
    solution: PegRow,
    /// One entry per colour, in `pegs`.
    colours: PegRow,
    pegsz: i32,
    hintsz: i32,
    gapsz: i32,
    pegrad: i32,
    hintrad: i32,
    border: i32,
    colx: i32,
    coly: i32,
    guessx: i32,
    guessy: i32,
    /// Marking pegs per line.
    hintw: i32,
    w: i32,
    h: i32,
    started: bool,
    solved: bool,
    next_go: usize,
    blit_peg: Option<Blitter>,
    drag_col: i32,
    blit_ox: i32,
    blit_oy: i32,
}

impl DrawState {
    fn pegoff(&self) -> i32 {
        self.pegsz + self.gapsz
    }

    fn hintoff(&self) -> i32 {
        self.hintsz + self.gapsz
    }

    fn cgap(&self) -> i32 {
        (self.gapsz / 2).max(1)
    }

    fn col_y(&self, c: i32) -> i32 {
        self.coly + c * self.pegoff()
    }

    fn col_h(&self) -> i32 {
        self.colours.pegs.len() as i32 * self.pegoff()
    }

    fn guess_x(&self, p: i32) -> i32 {
        self.guessx + p * self.pegoff()
    }

    fn guess_y(&self, g: i32) -> i32 {
        self.guessy + g * self.pegoff()
    }

    fn guess_w(&self) -> i32 {
        self.solution.pegs.len() as i32 * self.pegoff()
    }

    fn guess_h(&self) -> i32 {
        self.nguesses as i32 * self.pegoff()
    }

    fn hint_x(&self) -> i32 {
        self.guessx + self.guess_w() + self.gapsz
    }

    fn hint_y(&self, g: i32) -> i32 {
        self.guessy + (self.pegsz - self.hintoff() - self.hintsz) / 2 + g * self.pegoff()
    }

    fn hint_w(&self) -> i32 {
        self.hintw * self.hintoff() - self.gapsz
    }

    fn soln_y(&self) -> i32 {
        self.guessy + self.guess_h() + self.gapsz + 2
    }
}

/// C's `is_markable`: enough pegs set, and no repeats if those aren't
/// allowed.
fn is_markable(params: &Params, pegs: &PegRow) -> bool {
    let mut count = vec![0; params.ncolours.max(0) as usize];
    let mut nset = 0;
    for &c in &pegs.pegs {
        if c > 0 && c <= params.ncolours {
            count[(c - 1) as usize] += 1;
            nset += 1;
        }
    }
    let required = if params.allow_blank { 1 } else { params.npegs };
    nset >= required && (params.allow_multiple || count.iter().all(|&n| n <= 1))
}

/// C's `mark_pegs`: marks `guess` against `solution`, counting colours
/// 1 to `ncols`; returns how many are in the right place.
fn mark_pegs(guess: &mut PegRow, solution: &PegRow, ncols: i32) -> usize {
    let n = guess.pegs.len();
    let nc_place = (0..n)
        .filter(|&i| guess.pegs[i] == solution.pegs[i])
        .count();
    // sum over colours of min(#guess, #solution), less the exact
    // matches (Knuth).
    let mut nc_colour = 0;
    for colour in 1..=ncols {
        let n_guess = guess.pegs.iter().filter(|&&p| p == colour).count();
        let n_solution = solution.pegs.iter().filter(|&&p| p == colour).count();
        nc_colour += n_guess.min(n_solution);
    }
    let nc_colour = nc_colour.saturating_sub(nc_place);
    guess.feedback.fill(0);
    for (j, f) in guess
        .feedback
        .iter_mut()
        .enumerate()
        .take((nc_place + nc_colour).min(n))
    {
        *f = if j < nc_place {
            FEEDBACK_CORRECTPLACE
        } else {
            FEEDBACK_CORRECTCOLOUR
        };
    }
    nc_place
}

/// Reads a row in C's `%d` and `_` format (moves and the UI encoding):
/// for each peg, `atoi`, skip digits, an optional `_` (held), an
/// optional comma.
fn read_row(text: &[u8], npegs: usize) -> (Vec<i32>, Vec<bool>) {
    let mut p = text;
    let mut pegs = Vec::with_capacity(npegs);
    let mut holds = Vec::with_capacity(npegs);
    for _ in 0..npegs {
        pegs.push(atoi(p));
        p = skip_digits(p);
        let held = p.first() == Some(&b'_');
        if held {
            p = &p[1..];
        }
        holds.push(held);
        if p.first() == Some(&b',') {
            p = &p[1..];
        }
    }
    (pegs, holds)
}

/// Writes a row as `read_row` reads it.
fn write_row(out: &mut String, pegs: &[i32], holds: &[bool]) {
    for (i, (&peg, &held)) in pegs.iter().zip(holds).enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{peg}{}", if held { "_" } else { "" });
    }
}

/// The Guess puzzle.
#[derive(Clone, Copy, Debug)]
pub struct Guess;

impl Game for Guess {
    type Params = Params;
    type State = State;
    type Ui = Ui;
    type DrawState = DrawState;
    type Move = Move;

    const NAME: &'static str = "Guess";
    const PREFERRED_TILESIZE: i32 = PEG_PREFER_SZ;
    const PALETTE: &'static [PaletteEntry] = Col::PALETTE;
    const CAN_CONFIGURE: bool = true;
    const CAN_SOLVE: bool = true;

    fn default_params() -> Params {
        // The canonical Mastermind rules.
        Params {
            ncolours: 6,
            npegs: 4,
            nguesses: 10,
            allow_blank: false,
            allow_multiple: true,
        }
    }

    fn presets() -> PresetMenu<Params> {
        let mut menu = PresetMenu::new();
        menu.add_preset("Standard", Self::default_params());
        menu.add_preset(
            "Super",
            Params {
                ncolours: 8,
                npegs: 5,
                nguesses: 12,
                allow_blank: false,
                allow_multiple: true,
            },
        );
        menu
    }

    fn decode_params(params: &mut Params, s: &str) {
        // C starts from the defaults, not from the given parameters.
        *params = Self::default_params();
        let mut p = s.as_bytes();
        while let Some((&c, rest)) = p.split_first() {
            p = rest;
            match c {
                b'c' => {
                    params.ncolours = atoi(p);
                    p = skip_digits(p);
                }
                b'p' => {
                    params.npegs = atoi(p);
                    p = skip_digits(p);
                }
                b'g' => {
                    params.nguesses = atoi(p);
                    p = skip_digits(p);
                }
                b'b' => params.allow_blank = true,
                b'B' => params.allow_blank = false,
                b'm' => params.allow_multiple = true,
                b'M' => params.allow_multiple = false,
                _ => {}
            }
        }
    }

    fn encode_params(params: &Params, _full: bool) -> String {
        format!(
            "c{}p{}g{}{}{}",
            params.ncolours,
            params.npegs,
            params.nguesses,
            if params.allow_blank { "b" } else { "B" },
            if params.allow_multiple { "m" } else { "M" }
        )
    }

    fn configure(params: &Params) -> Option<Vec<ConfigItem>> {
        Some(vec![
            ConfigItem::string("Colours", params.ncolours.to_string()),
            ConfigItem::string("Pegs per guess", params.npegs.to_string()),
            ConfigItem::string("Guesses", params.nguesses.to_string()),
            ConfigItem::boolean("Allow blanks", params.allow_blank),
            ConfigItem::boolean("Allow duplicates", params.allow_multiple),
        ])
    }

    fn custom_params(config: &[ConfigItem]) -> Result<Params, GameError> {
        let wrong = || GameError::new("Unexpected configuration fields");
        let [c, p, g, b, m] = config else {
            return Err(wrong());
        };
        let number =
            |item: &ConfigItem| item.as_str().map(|s| atoi(s.as_bytes())).ok_or_else(wrong);
        Ok(Params {
            ncolours: number(c)?,
            npegs: number(p)?,
            nguesses: number(g)?,
            allow_blank: b.as_bool().ok_or_else(wrong)?,
            allow_multiple: m.as_bool().ok_or_else(wrong)?,
        })
    }

    fn validate_params(params: &Params, _full: bool) -> Result<(), GameError> {
        if params.ncolours < 2 || params.npegs < 2 {
            return Err(GameError::new("Trivial solutions are uninteresting"));
        }
        // The largest colour must also fit in a byte (new_desc).
        if params.ncolours > 10 {
            return Err(GameError::new("Too many colours"));
        }
        if params.nguesses < 1 {
            return Err(GameError::new("Must have at least one guess"));
        }
        if !params.allow_multiple && params.ncolours < params.npegs {
            return Err(GameError::new(
                "Disallowing multiple colours requires at least as many colours as pegs",
            ));
        }
        Ok(())
    }

    // C: guess.c, new_game_desc()
    fn new_desc(params: &Params, rng: &mut RandomState, _interactive: bool) -> NewDesc {
        let npegs = params.npegs as usize;
        let mut bmp = vec![0_u8; npegs];
        let mut count = vec![0; params.ncolours as usize];
        for b in &mut bmp {
            let c = loop {
                let c = rng.upto(params.ncolours as u32) as usize;
                if params.allow_multiple || count[c] == 0 {
                    break c;
                }
            };
            count[c] += 1;
            *b = (c + 1) as u8;
        }
        obfuscate_bitmap(&mut bmp, npegs * 8);
        NewDesc::new(bytes_to_hex(&bmp))
    }

    fn validate_desc(params: &Params, desc: &str) -> Result<(), GameError> {
        let npegs = params.npegs as usize;
        if desc.len() as u64 != 2 * u64::from(params.npegs as u32) {
            return Err(GameError::new("Game description is wrong length"));
        }
        let mut bmp = hex_to_bytes(desc, npegs).unwrap_or_default();
        deobfuscate_bitmap(&mut bmp, npegs * 8);
        if bmp.iter().any(|&b| b < 1 || i32::from(b) > params.ncolours) {
            return Err(GameError::new("Game description is corrupted"));
        }
        Ok(())
    }

    fn new_game(_ctx: &mut GameCtx, params: &Params, desc: &str) -> State {
        let (npegs, nguesses) = (params.npegs as usize, params.nguesses as usize);
        let mut bmp = hex_to_bytes(desc, npegs).unwrap_or_else(|| vec![0; npegs]);
        deobfuscate_bitmap(&mut bmp, npegs * 8);
        let mut solution = PegRow::new(npegs);
        for (s, &b) in solution.pegs.iter_mut().zip(&bmp) {
            *s = i32::from(b);
        }
        State {
            params: *params,
            guesses: vec![PegRow::new(npegs); nguesses],
            holds: vec![false; npegs],
            solution,
            next_go: 0,
            solved: GameStatus::Ongoing,
        }
    }

    fn solve(_original: &State, _current: &State, _aux: Option<&str>) -> Result<Move, GameError> {
        Ok(Move::Solve)
    }

    fn status(state: &State) -> GameStatus {
        // The solution is shown either way, even when it wasn't guessed.
        state.solved
    }

    fn new_ui(state: Option<&State>) -> Ui {
        let params = state.map_or(
            Params {
                ncolours: 0,
                npegs: 0,
                nguesses: 0,
                allow_blank: false,
                allow_multiple: false,
            },
            |s| s.params,
        );
        let npegs = params.npegs as usize;
        Ui {
            params,
            curr_pegs: PegRow::new(npegs),
            holds: vec![false; npegs],
            colour_cur: 0,
            peg_cur: 0,
            display_cur: env_bool("PUZZLES_SHOW_CURSOR").unwrap_or(false),
            markable: false,
            drag_col: 0,
            drag_x: 0,
            drag_y: 0,
            drag_opeg: None,
            show_labels: false,
            hint: None,
        }
    }

    fn prefs(ui: &Ui) -> Vec<ConfigItem> {
        vec![ConfigItem {
            name: "Label colours with numbers",
            keyword: Some("show-labels"),
            value: ConfigValue::Bool(ui.show_labels),
        }]
    }

    fn set_prefs(ui: &mut Ui, config: &[ConfigItem]) {
        if let Some(b) = config.first().and_then(ConfigItem::as_bool) {
            ui.show_labels = b;
        }
    }

    fn encode_ui(ui: &Ui) -> Option<String> {
        // The guess in progress and the holds.
        let mut out = String::new();
        write_row(&mut out, &ui.curr_pegs.pegs, &ui.holds);
        Some(out)
    }

    fn decode_ui(ui: &mut Ui, encoding: &str, _state: &State) {
        let (pegs, holds) = read_row(encoding.as_bytes(), ui.curr_pegs.pegs.len());
        for (i, peg) in pegs.into_iter().enumerate() {
            // Invalid pegs are removed.
            ui.curr_pegs.pegs[i] = if (0..=ui.params.ncolours).contains(&peg) {
                peg
            } else {
                0
            };
        }
        ui.holds = holds;
        ui.markable = is_markable(&ui.params, &ui.curr_pegs);
    }

    fn changed_state(ui: &mut Ui, old: &State, new: &State) {
        if new.next_go < old.next_go {
            ui.hint = None;
        }
        // Held pegs carry over, the rest clear (also, arguably rightly,
        // on undo).
        let solved = new.solved != GameStatus::Ongoing;
        for i in 0..new.solution.pegs.len() {
            ui.holds[i] = !solved && new.holds[i];
            ui.curr_pegs.pegs[i] = if solved || new.next_go == 0 || !ui.holds[i] {
                0
            } else {
                new.guesses[new.next_go - 1].pegs[i]
            };
        }
        ui.markable = is_markable(&new.params, &ui.curr_pegs);
        if !ui.markable && ui.peg_cur == new.params.npegs {
            ui.peg_cur = 0;
        }
    }

    fn current_key_label(ui: &Ui, state: &State, button: Button) -> Option<&'static str> {
        if state.solved != GameStatus::Ongoing {
            return None;
        }
        if button == Button::cursor(CursorKey::Select) {
            return Some(if ui.peg_cur == state.params.npegs {
                "Submit"
            } else {
                "Place"
            });
        }
        if button == Button::cursor(CursorKey::Select2) && ui.peg_cur != state.params.npegs {
            return Some("Hold");
        }
        None
    }

    // C: guess.c, interpret_move()
    fn interpret_move(
        from: &State,
        ui: &mut Ui,
        ds: &DrawState,
        x: i32,
        y: i32,
        button: Button,
    ) -> MoveResult<Move> {
        let key = |c: char| button == Button::key(c);
        if key('l') || key('L') {
            ui.show_labels = !ui.show_labels;
            return MoveResult::UiUpdate;
        }
        if from.solved != GameStatus::Ongoing {
            return MoveResult::Unused;
        }

        let pegoff = ds.pegoff();
        let next_go = from.next_go as i32;
        let (guess_ox, guess_oy) = (ds.guess_x(0), ds.guess_y(next_go));
        let mut over_col = 0; // from 1
        let mut over_guess = None;
        let mut over_past = None;
        let mut over_hint = false;
        if x >= ds.colx && x < ds.colx + pegoff && y >= ds.coly && y < ds.coly + ds.col_h() {
            over_col = (y - ds.coly) / pegoff + 1;
        } else if x >= guess_ox && y >= guess_oy && y < guess_oy + ds.guess_h() {
            if x < guess_ox + ds.guess_w() {
                over_guess = Some(((x - guess_ox) / pegoff) as usize);
            } else {
                over_hint = true;
            }
        } else if x >= guess_ox && x < guess_ox + ds.guess_w() && y >= ds.guessy && y < guess_oy {
            over_past = Some((
                ((y - ds.guessy) / pegoff) as usize,
                ((x - guess_ox) / pegoff) as usize,
            ));
        }

        let mouse = |b: MouseButton, a: MouseAction| button == Button::mouse(b, a);
        let mut ret = MoveResult::Unused;
        if mouse(MouseButton::Left, MouseAction::Press) {
            if over_col > 0 {
                ui.drag_col = over_col;
                ui.drag_opeg = None;
            } else if let Some(g) = over_guess {
                let col = ui.curr_pegs.pegs[g];
                if col != 0 {
                    ui.drag_col = col;
                    ui.drag_opeg = Some(g);
                }
            } else if let Some((gy, gx)) = over_past {
                let col = from.guesses[gy].pegs[gx];
                if col != 0 {
                    ui.drag_col = col;
                    ui.drag_opeg = None;
                }
            }
            if ui.drag_col != 0 {
                ui.drag_x = x;
                ui.drag_y = y;
                ret = MoveResult::UiUpdate;
            }
        } else if mouse(MouseButton::Left, MouseAction::Drag) && ui.drag_col != 0 {
            ui.drag_x = x;
            ui.drag_y = y;
            ret = MoveResult::UiUpdate;
        } else if mouse(MouseButton::Left, MouseAction::Release) && ui.drag_col != 0 {
            if let Some(g) = over_guess {
                set_peg(&from.params, ui, g, ui.drag_col);
            } else if let Some(o) = ui.drag_opeg {
                set_peg(&from.params, ui, o, 0);
            }
            ui.drag_col = 0;
            ui.drag_opeg = None;
            ui.display_cur = false;
            ret = MoveResult::UiUpdate;
        } else if mouse(MouseButton::Right, MouseAction::Press) {
            if let Some(g) = over_guess {
                // Carry this peg on to the next guess.
                ui.holds[g] = !ui.holds[g];
                ret = MoveResult::UiUpdate;
            }
        } else if mouse(MouseButton::Left, MouseAction::Release) && over_hint && ui.markable {
            // Not at the end of a drag, in case of a slip.
            ret = MoveResult::Move(guess_move(ui));
        }

        let npegs = from.params.npegs;
        let ncolours = from.params.ncolours;
        // C compares the whole code, so a modified arrow isn't one.
        if button.mods.is_empty() && button.is_cursor_move() {
            let maxcur = npegs + i32::from(ui.markable);
            ret = move_cursor(
                button,
                &mut ui.peg_cur,
                &mut ui.colour_cur,
                maxcur,
                ncolours,
                false,
                Some(&mut ui.display_cur),
            );
        } else if key('h') || key('H') || key('?') {
            compute_hint(from, ui);
            ret = MoveResult::UiUpdate;
        } else if button == Button::cursor(CursorKey::Select) {
            ui.display_cur = true;
            if ui.peg_cur == npegs {
                // C submits even an incomplete guess here, which its
                // mid-end then aborts on.
                ret = if ui.markable {
                    MoveResult::Move(guess_move(ui))
                } else {
                    MoveResult::UiUpdate
                };
            } else {
                set_peg(&from.params, ui, ui.peg_cur as usize, ui.colour_cur + 1);
                ret = MoveResult::UiUpdate;
            }
        } else if let Some(colour) = digit_colour(button, ncolours).filter(|_| ui.peg_cur < npegs) {
            // A number key places a peg and moves on.
            ui.display_cur = true;
            set_peg(&from.params, ui, ui.peg_cur as usize, colour);
            if ui.peg_cur + 1 < npegs + i32::from(ui.markable) {
                ui.peg_cur += 1;
            }
            ret = MoveResult::UiUpdate;
        } else if key('D') || key('d') || key('\u{8}') {
            if !ui.display_cur
                || ui
                    .curr_pegs
                    .pegs
                    .get(ui.peg_cur as usize)
                    .is_some_and(|&p| p != 0)
            {
                ui.display_cur = true;
                if (ui.peg_cur as usize) < ui.curr_pegs.pegs.len() {
                    set_peg(&from.params, ui, ui.peg_cur as usize, 0);
                }
                ret = MoveResult::UiUpdate;
            } else {
                ret = MoveResult::NoEffect;
            }
        } else if button == Button::cursor(CursorKey::Select2) {
            if ui.peg_cur == npegs {
                return MoveResult::Unused;
            }
            ui.display_cur = true;
            let p = ui.peg_cur as usize;
            ui.holds[p] = !ui.holds[p];
            ret = MoveResult::UiUpdate;
        }
        ret
    }

    fn execute_move(_ctx: &mut GameCtx, from: &State, mv: &Move) -> Option<State> {
        match mv {
            Move::Solve => {
                let mut state = from.clone();
                state.solved = GameStatus::Lost;
                Some(state)
            }
            Move::Guess { pegs, holds } => {
                // No guesses once the game is over.
                if from.solved != GameStatus::Ongoing || pegs.len() != from.solution.pegs.len() {
                    return None;
                }
                let min = i32::from(!from.params.allow_blank);
                if pegs.iter().any(|&v| v < min || v > from.params.ncolours) {
                    return None;
                }
                let mut state = from.clone();
                let go = from.next_go;
                state.guesses[go].pegs.clone_from(pegs);
                state.holds.clone_from(holds);
                let solution = state.solution.clone();
                let nc_place = mark_pegs(&mut state.guesses[go], &solution, state.params.ncolours);
                if nc_place == solution.pegs.len() {
                    state.solved = GameStatus::Won;
                } else {
                    state.next_go = go + 1;
                    if state.next_go >= state.params.nguesses as usize {
                        state.solved = GameStatus::Lost; // shows the pegs
                    }
                }
                Some(state)
            }
        }
    }

    fn encode_move(mv: &Move, out: &mut String) {
        match mv {
            Move::Guess { pegs, holds } => {
                out.push('G');
                write_row(out, pegs, holds);
            }
            Move::Solve => out.push('S'),
        }
    }

    fn decode_move(state: &State, text: &str) -> Result<Move, MoveParseError> {
        // As C's execute_move: exactly `S`, or `G` and a peg per hole
        // read leniently (missing ones read as 0), each in range.
        if text == "S" {
            return Ok(Move::Solve);
        }
        let rest = text.as_bytes().strip_prefix(b"G").ok_or(MoveParseError)?;
        let (pegs, holds) = read_row(rest, state.solution.pegs.len());
        let min = i32::from(!state.params.allow_blank);
        if pegs.iter().any(|&v| v < min || v > state.params.ncolours) {
            return Err(MoveParseError);
        }
        Ok(Move::Guess { pegs, holds })
    }

    fn compute_size(params: &Params, tilesize: i32, _ui: &Ui) -> (i32, i32) {
        let hintw = f64::from((params.npegs + 1) / 2);
        let npegs = f64::from(params.npegs);
        let hmul = BORDER * 2.0
            + 2.0
            + npegs
            + PEG_GAP * npegs
            + PEG_HINT * hintw
            + PEG_GAP * (hintw - 1.0);
        let vmul_c =
            BORDER * 2.0 + f64::from(params.ncolours) + PEG_GAP * f64::from(params.ncolours - 1);
        let vmul_g = BORDER * 2.0
            + f64::from(params.nguesses + 1)
            + PEG_GAP * f64::from(params.nguesses + 1);
        let vmul = vmul_c.max(vmul_g);
        (
            (f64::from(tilesize) * hmul).ceil() as i32,
            (f64::from(tilesize) * vmul).ceil() as i32,
        )
    }

    fn new_drawstate(state: &State) -> DrawState {
        let (npegs, nguesses) = (state.params.npegs as usize, state.params.nguesses as usize);
        DrawState {
            nguesses,
            guesses: vec![PegRow::invalid(npegs); nguesses],
            solution: PegRow::invalid(npegs),
            colours: PegRow::invalid(state.params.ncolours as usize),
            pegsz: 0,
            hintsz: 0,
            gapsz: 0,
            pegrad: 0,
            hintrad: 0,
            border: 0,
            colx: 0,
            coly: 0,
            guessx: 0,
            guessy: 0,
            hintw: (state.params.npegs + 1) / 2,
            w: 0,
            h: 0,
            started: false,
            solved: false,
            next_go: 0,
            blit_peg: None,
            drag_col: 0,
            blit_ox: 0,
            blit_oy: 0,
        }
    }

    fn set_size(ds: &mut DrawState, params: &Params, tilesize: i32) {
        ds.pegsz = tilesize;
        ds.hintsz = (f64::from(ds.pegsz) * PEG_HINT) as i32;
        ds.gapsz = (f64::from(ds.pegsz) * PEG_GAP) as i32;
        ds.border = (f64::from(ds.pegsz) * BORDER) as i32;
        // A peg fits in pegsz = 2r + 1.
        ds.pegrad = (ds.pegsz - 1) / 2;
        ds.hintrad = (ds.hintsz - 1) / 2;

        let colh = (ds.pegsz + ds.gapsz) * params.ncolours - ds.gapsz;
        let guessh = (ds.pegsz + ds.gapsz) * params.nguesses + ds.gapsz + ds.pegsz;
        let unused_ui = Self::new_ui(None);
        (ds.w, ds.h) = Self::compute_size(params, tilesize, &unused_ui);
        ds.colx = ds.border;
        ds.coly = (ds.h - colh) / 2;
        ds.guessx = ds.border + ds.pegsz * 2;
        ds.guessy = (ds.h - guessh) / 2;
        ds.blit_peg = Some(Blitter::new(ds.pegsz + 2, ds.pegsz + 2));
    }

    // C: guess.c, game_redraw()
    fn redraw(
        dr: &mut Drawing<'_>,
        ds: &mut DrawState,
        _old: Option<&State>,
        state: &State,
        _dir: i32,
        ui: &Ui,
        _anim_time: f32,
        _flash_time: f32,
    ) {
        let solved = state.solved != GameStatus::Ongoing;
        let npegs = state.params.npegs;
        let new_move = state.next_go != ds.next_go || !ds.started;

        if !ds.started {
            let soln_w = ds.guess_w();
            dr.rect(ds.guessx, ds.soln_y() - ds.gapsz - 1, soln_w, 2, Col::Frame);
            dr.update(0, 0, ds.w, ds.h);
        }

        if ds.drag_col != 0
            && let Some(blitter) = &ds.blit_peg
        {
            dr.blitter_load(blitter, ds.blit_ox, ds.blit_oy);
            dr.update(ds.blit_ox, ds.blit_oy, ds.pegsz, ds.pegsz);
        }

        // The colour bar.
        for i in 0..state.params.ncolours {
            let mut val = i + 1;
            if ui.display_cur && ui.colour_cur == i {
                val |= PEG_CURSOR;
            }
            if ui.show_labels {
                val |= PEG_HOLD;
            }
            if ds.colours.pegs[i as usize] != val {
                draw_peg(dr, ds, ds.colx, ds.col_y(i), false, ui.show_labels, i + 1);
                if val & PEG_CURSOR != 0 {
                    draw_cursor(dr, ds, ds.colx, ds.col_y(i));
                }
                ds.colours.pegs[i as usize] = val;
            }
        }

        // The guesses so far and their marks, from the bottom, leaving the
        // current one until last so the cursor around it isn't covered.
        for i in (0..state.params.nguesses as usize).rev() {
            if i < state.next_go || solved {
                guess_redraw(
                    dr,
                    ds,
                    Some(i),
                    Some(&state.guesses[i]),
                    None,
                    -1,
                    false,
                    ui.show_labels,
                );
                let force = i + 1 == state.next_go;
                hint_redraw(dr, ds, i, Some(&state.guesses[i]), force, false, false);
            } else if i > state.next_go {
                guess_redraw(dr, ds, Some(i), None, None, -1, false, ui.show_labels);
                hint_redraw(dr, ds, i, None, false, false, false);
            }
        }
        if !solved {
            // The guess being built lives in the UI.
            let cursor = if ui.display_cur { ui.peg_cur } else { -1 };
            guess_redraw(
                dr,
                ds,
                Some(state.next_go),
                Some(&ui.curr_pegs),
                Some(&ui.holds),
                cursor,
                false,
                ui.show_labels,
            );
            hint_redraw(
                dr,
                ds,
                state.next_go,
                None,
                true,
                ui.display_cur && ui.peg_cur == npegs,
                ui.markable,
            );
        }

        // The current-row marker.
        if new_move {
            currmove_redraw(dr, ds, ds.next_go as i32, Col::Background);
        }
        if !solved {
            currmove_redraw(dr, ds, state.next_go as i32, Col::Hold);
        }

        // The solution, or the box hiding it.
        if (solved != ds.solved) || !ds.started {
            let (sx, sy, sw, sh) = (ds.guessx, ds.soln_y(), ds.guess_w(), ds.pegoff());
            dr.rect(
                sx,
                sy,
                sw,
                sh,
                if solved { Col::Background } else { Col::Empty },
            );
            dr.update(sx, sy, sw, sh);
        }
        if solved {
            let force = !ds.solved;
            guess_redraw(
                dr,
                ds,
                None,
                Some(&state.solution),
                None,
                -1,
                force,
                ui.show_labels,
            );
        }
        ds.solved = solved;
        ds.next_go = state.next_go;

        // A dragged peg: save what's under it, then draw it.
        if ui.drag_col != 0 {
            let ox = ui.drag_x - ds.pegsz / 2;
            let oy = ui.drag_y - ds.pegsz / 2;
            ds.blit_ox = ox - 1;
            ds.blit_oy = oy - 1;
            if let Some(blitter) = &ds.blit_peg {
                dr.blitter_save(blitter, ds.blit_ox, ds.blit_oy);
            }
            draw_peg(dr, ds, ox, oy, true, ui.show_labels, ui.drag_col);
        }
        ds.drag_col = ui.drag_col;
        ds.started = true;
    }

    fn cursor_location(ui: &Ui, ds: &DrawState, state: &State, _params: &Params) -> Option<Rect> {
        (ui.display_cur && state.solved == GameStatus::Ongoing).then(|| {
            let size = 2 * (ds.pegrad + ds.cgap()) + 1;
            Rect {
                x: ds.guess_x(ui.peg_cur) - ds.cgap(),
                y: ds.guess_y(state.next_go as i32) - ds.cgap(),
                w: size,
                h: size,
            }
        })
    }
}

/// The colour a key places: C's `button >= '1' && button <= '0' +
/// ncolours` (so with 10 colours `:` too) or `'0'` for colour 10. C
/// compares the whole code, so a keypad digit never places one.
fn digit_colour(button: Button, ncolours: i32) -> Option<i32> {
    if !button.mods.is_empty() {
        return None;
    }
    let ButtonKind::Key(c) = button.kind else {
        return None;
    };
    let code = c as i32 - i32::from(b'0');
    if (1..=ncolours).contains(&code) {
        Some(code)
    } else if code == 0 && ncolours == 10 {
        Some(10)
    } else {
        None
    }
}

/// C's `set_peg`.
fn set_peg(params: &Params, ui: &mut Ui, peg: usize, col: i32) {
    ui.curr_pegs.pegs[peg] = col;
    ui.markable = is_markable(params, &ui.curr_pegs);
}

/// C's `encode_move`: the guess in the UI as a move.
fn guess_move(ui: &Ui) -> Move {
    Move::Guess {
        pegs: ui.curr_pegs.pegs.clone(),
        holds: ui.holds.clone(),
    }
}

/// C's `compute_hint`: puts the lexicographically first row consistent
/// with every mark so far into the guess. The search resumes where the
/// last hint stopped (forgotten on undo, which can bring back rows it
/// passed). Should none fit (impossible for a real puzzle), it changes
/// the UI trivially, as C does, so the key visibly did something.
fn compute_hint(state: &State, ui: &mut Ui) {
    let params = &state.params;
    let npegs = params.npegs as usize;
    let past = &state.guesses[..state.next_go];

    // Colours above the largest used so far (plus the ones a new colour
    // could add) give no new information.
    let mut maxcolour = past
        .iter()
        .flat_map(|g| g.pegs.iter().copied())
        .max()
        .unwrap_or(0)
        .max(0);
    maxcolour = if params.allow_multiple {
        (maxcolour + 1).min(params.ncolours)
    } else {
        (maxcolour + params.npegs).min(params.ncolours)
    };
    // Colours below a guess of all one colour that got no marks are
    // ruled out.
    let mut mincolour = 1;
    while past
        .iter()
        .any(|g| g.feedback[0] == 0 && g.pegs.iter().all(|&p| p == mincolour))
    {
        mincolour += 1;
    }

    let mut hint = ui.hint.take().unwrap_or_else(|| {
        let mut row = PegRow::new(npegs);
        row.pegs.fill(1);
        row
    });
    let mut found = false;
    while hint.pegs[0] <= params.ncolours {
        let fits = is_markable(params, &hint)
            && past.iter().all(|g| {
                mark_pegs(&mut hint, g, maxcolour);
                hint.feedback == g.feedback
            });
        if fits {
            ui.curr_pegs.pegs.clone_from(&hint.pegs);
            ui.markable = true;
            ui.peg_cur = params.npegs;
            ui.display_cur = true;
            found = true;
            break;
        }
        // An odometer over mincolour..=maxcolour, except that the first
        // peg keeps counting up (to ncolours, ending the search).
        let mut i = npegs;
        loop {
            i -= 1;
            hint.pegs[i] += 1;
            if i == 0 || hint.pegs[i] <= maxcolour {
                break;
            }
            hint.pegs[i] = mincolour;
        }
    }
    ui.hint = Some(hint);
    if found {
        return;
    }
    if !ui.display_cur {
        ui.display_cur = true;
    } else if params.npegs == 1 {
        ui.display_cur = false;
    } else {
        ui.peg_cur = (ui.peg_cur + 1) % params.npegs;
    }
}

/// C's `draw_peg`: a peg (or empty hole) with its top left at `(cx, cy)`.
fn draw_peg(
    dr: &mut Drawing<'_>,
    ds: &DrawState,
    cx: i32,
    cy: i32,
    moving: bool,
    labelled: bool,
    col: i32,
) {
    let (cgap, pegsz, pegrad) = (ds.cgap(), ds.pegsz, ds.pegrad);
    // Antialiased circles need the background cleared first, except for
    // the dragged peg, which should blend into what's behind it.
    if !moving {
        dr.rect(
            cx - cgap,
            cy - cgap,
            pegsz + cgap * 2,
            pegsz + cgap * 2,
            Col::Background,
        );
    }
    if pegrad > 0 {
        let outline = if col != 0 {
            Col::Frame as usize
        } else {
            Col::Empty as usize
        };
        dr.circle(
            cx + pegrad,
            cy + pegrad,
            pegrad,
            Some(peg_colour(col)),
            outline,
        );
    } else {
        dr.rect(cx, cy, pegsz, pegsz, peg_colour(col));
    }
    if labelled && col != 0 {
        let label = char::from(b'0' + (col % 10) as u8).to_string();
        dr.text(
            cx + pegrad,
            cy + pegrad,
            FontType::Variable,
            pegrad,
            Align::CENTRE,
            Col::Frame,
            &label,
        );
    }
    dr.update(cx - cgap, cy - cgap, pegsz + cgap * 2, pegsz + cgap * 2);
}

/// C's `draw_cursor`.
fn draw_cursor(dr: &mut Drawing<'_>, ds: &DrawState, x: i32, y: i32) {
    let (cgap, pegrad) = (ds.cgap(), ds.pegrad);
    dr.circle(
        x + pegrad,
        y + pegrad,
        pegrad + cgap,
        None::<usize>,
        Col::Cursor,
    );
    dr.update(x - cgap, y - cgap, ds.pegsz + cgap * 2, ds.pegsz + cgap * 2);
}

/// C's `guess_redraw`: guess row `guess` (`None` for the solution) from
/// `src` (`None` for empty).
#[allow(clippy::too_many_arguments, reason = "mirrors C's guess_redraw")]
fn guess_redraw(
    dr: &mut Drawing<'_>,
    ds: &mut DrawState,
    guess: Option<usize>,
    src: Option<&PegRow>,
    holds: Option<&[bool]>,
    cur_col: i32,
    force: bool,
    labelled: bool,
) {
    let (rowx, rowy) = match guess {
        None => (ds.guessx, ds.soln_y()),
        Some(g) => (ds.guess_x(0), ds.guess_y(g as i32)),
    };
    let pegoff = ds.pegoff();
    let n = ds.solution.pegs.len();
    for i in 0..n {
        let mut scol = src.map_or(0, |s| s.pegs[i]);
        if i as i32 == cur_col {
            scol |= PEG_CURSOR;
        }
        if holds.is_some_and(|h| h[i]) {
            scol |= PEG_HOLD;
        }
        if labelled {
            scol |= PEG_LABELLED;
        }
        let drawn = match guess {
            None => ds.solution.pegs[i],
            Some(g) => ds.guesses[g].pegs[i],
        };
        if drawn != scol || force {
            let px = rowx + pegoff * i as i32;
            draw_peg(dr, ds, px, rowy, false, labelled, scol & !PEG_FLAGS);
            if scol & PEG_CURSOR != 0 {
                draw_cursor(dr, ds, px, rowy);
            }
            // The hold marker.
            if scol & PEG_HOLD != 0 {
                dr.rect(
                    px,
                    rowy + ds.pegsz + ds.gapsz / 2 - 2,
                    ds.pegsz,
                    2,
                    Col::Hold,
                );
            }
            dr.update(px, rowy + ds.pegsz + ds.gapsz / 2 - 2, ds.pegsz, 2);
        }
        match guess {
            None => ds.solution.pegs[i] = scol,
            Some(g) => ds.guesses[g].pegs[i] = scol,
        }
    }
}

/// C's `hint_redraw`: the marking pegs of guess `guess`, all or none of
/// them (the cursor can surround the lot).
fn hint_redraw(
    dr: &mut Drawing<'_>,
    ds: &mut DrawState,
    guess: usize,
    src: Option<&PegRow>,
    force: bool,
    cursor: bool,
    markable: bool,
) {
    let emptycol = if markable { Col::Markable } else { Col::Empty };
    let n = ds.guesses[guess].feedback.len();
    let hintlen = n.div_ceil(2);

    let mut need_redraw = false;
    for i in 0..n {
        let mut scol = src.map_or(0, |s| s.feedback[i]);
        if i == 0 && cursor {
            scol |= PEG_CURSOR;
        }
        if i == 0 && markable {
            scol |= PEG_HOLD;
        }
        if scol != ds.guesses[guess].feedback[i] || force {
            need_redraw = true;
        }
        ds.guesses[guess].feedback[i] = scol;
    }
    if !need_redraw {
        return;
    }

    let (gap, hintsz, hintrad, hintoff) = (ds.gapsz, ds.hintsz, ds.hintrad, ds.hintoff());
    let hinth = hintsz + gap + hintsz;
    let (hx, hy) = (ds.hint_x() - gap, ds.hint_y(guess as i32) - gap);
    let (hw, hh) = (ds.hint_w() + gap * 2, hinth + gap * 2);

    // Erase a large background rectangle.
    dr.rect(hx, hy, hw, hh, Col::Background);
    for i in 0..n {
        let scol = src.map_or(0, |s| s.feedback[i]);
        let col = match scol {
            FEEDBACK_CORRECTPLACE => Col::CorrectPlace,
            FEEDBACK_CORRECTCOLOUR => Col::CorrectColour,
            _ => emptycol,
        };
        let (mut rowx, mut rowy) = (ds.hint_x(), ds.hint_y(guess as i32));
        if i < hintlen {
            rowx += hintoff * i as i32;
        } else {
            rowx += hintoff * (i - hintlen) as i32;
            rowy += hintoff;
        }
        if hintrad > 0 {
            let outline = if col == emptycol {
                emptycol
            } else {
                Col::Frame
            };
            dr.circle(rowx + hintrad, rowy + hintrad, hintrad, Some(col), outline);
        } else {
            dr.rect(rowx, rowy, hintsz, hintsz, col);
        }
    }
    if cursor {
        let cgap = ds.cgap();
        let (x1, y1, x2, y2) = (hx + cgap, hy + cgap, hx + hw - cgap, hy + hh - cgap);
        dr.line(x1, y1, x2, y1, Col::Cursor);
        dr.line(x2, y1, x2, y2, Col::Cursor);
        dr.line(x2, y2, x1, y2, Col::Cursor);
        dr.line(x1, y2, x1, y1, Col::Cursor);
    }
    dr.update(hx, hy, hw, hh);
}

/// C's `currmove_redraw`: the marker left of guess row `guess`.
fn currmove_redraw(dr: &mut Drawing<'_>, ds: &DrawState, guess: i32, col: Col) {
    let (ox, oy, off) = (ds.guess_x(0), ds.guess_y(guess), ds.pegsz / 4);
    dr.rect(ox - off - 1, oy, 2, ds.pegsz, col);
    dr.update(ox - off - 1, oy, 2, ds.pegsz);
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn start(id: &str, seed: &[u8]) -> State {
        let mut p = Guess::default_params();
        Guess::decode_params(&mut p, id);
        let desc = Guess::new_desc(&p, &mut RandomState::new(seed), true).desc;
        Guess::new_game(&mut GameCtx::new(), &p, &desc)
    }

    fn guess(pegs: &[i32]) -> Move {
        Move::Guess {
            pegs: pegs.to_vec(),
            holds: vec![false; pegs.len()],
        }
    }

    #[test]
    fn marks_count_place_and_colour() {
        let solution = PegRow {
            pegs: vec![1, 2, 3, 4],
            feedback: vec![0; 4],
        };
        let mut g = PegRow {
            pegs: vec![1, 3, 2, 6],
            feedback: vec![0; 4],
        };
        assert_eq!(mark_pegs(&mut g, &solution, 6), 1);
        assert_eq!(
            g.feedback,
            [
                FEEDBACK_CORRECTPLACE,
                FEEDBACK_CORRECTCOLOUR,
                FEEDBACK_CORRECTCOLOUR,
                0
            ]
        );
    }

    #[test]
    fn hints_always_find_the_solution() {
        for (id, seeds) in [("c6p4g10Bm", 30), ("c8p5g12Bm", 10), ("c4p3g10bM", 10)] {
            for seed in 0..seeds {
                let mut state = start(id, seed.to_string().as_bytes());
                let mut ui = Guess::new_ui(Some(&state));
                for _ in 0..20 {
                    if state.solved != GameStatus::Ongoing {
                        break;
                    }
                    compute_hint(&state, &mut ui);
                    assert!(ui.markable, "{id} seed {seed}: no hint");
                    let next =
                        Guess::execute_move(&mut GameCtx::new(), &state, &guess_move(&ui)).unwrap();
                    Guess::changed_state(&mut ui, &state, &next);
                    state = next;
                }
                assert_eq!(state.solved, GameStatus::Won, "{id} seed {seed}");
            }
        }
    }

    #[test]
    fn running_out_of_guesses_loses() {
        let mut state = start("c6p4g2Bm", b"lose");
        let wrong: Vec<i32> = state.solution.pegs.iter().map(|&p| p % 6 + 1).collect();
        for _ in 0..2 {
            state = Guess::execute_move(&mut GameCtx::new(), &state, &guess(&wrong)).unwrap();
        }
        assert_eq!(Guess::status(&state), GameStatus::Lost);
        assert!(Guess::execute_move(&mut GameCtx::new(), &state, &guess(&wrong)).is_none());
    }

    #[test]
    fn params_reset_to_defaults_first() {
        let mut p = Guess::presets().presets().nth(1).map(|(_, p)| *p).unwrap();
        Guess::decode_params(&mut p, "p3");
        assert_eq!(
            p,
            Params {
                npegs: 3,
                ..Guess::default_params()
            }
        );
        assert_eq!(Guess::encode_params(&p, true), "c6p3g10Bm");
    }

    #[test]
    fn moves_and_ui_read_leniently_like_c() {
        let state = start("c6p4g10bm", b"x");
        assert_eq!(
            Guess::decode_move(&state, "G1,2_,3"),
            Ok(Move::Guess {
                pegs: vec![1, 2, 3, 0],
                holds: vec![false, true, false, false]
            })
        );
        assert!(Guess::decode_move(&state, "G7,1,1,1").is_err());
        assert!(Guess::decode_move(&state, "S ").is_err());
        let mut ui = Guess::new_ui(Some(&state));
        Guess::decode_ui(&mut ui, "1_,9,3,-2", &state);
        assert_eq!(Guess::encode_ui(&ui).as_deref(), Some("1_,0,3,0"));
    }

    proptest! {
        #[test]
        fn move_codec_round_trips(pegs in prop::collection::vec(0..=6_i32, 4), holds in prop::collection::vec(any::<bool>(), 4)) {
            let state = start("c6p4g10bm", b"codec");
            for mv in [Move::Guess { pegs: pegs.clone(), holds: holds.clone() }, Move::Solve] {
                let mut s = String::new();
                Guess::encode_move(&mv, &mut s);
                prop_assert_eq!(Guess::decode_move(&state, &s), Ok(mv));
            }
        }

        /// Untrusted text never panics.
        #[test]
        fn untrusted_text_never_panics(id in "[cpgbBmM0-9]{0,10}", desc in "[0-9a-f]{0,12}", text in "\\PC{0,12}") {
            let mut p = Guess::default_params();
            Guess::decode_params(&mut p, &id);
            let p = if Guess::validate_params(&p, true).is_ok() && p.npegs <= 10 && p.nguesses <= 20 { p } else { Guess::default_params() };
            if Guess::validate_desc(&p, &desc).is_ok() {
                let state = Guess::new_game(&mut GameCtx::new(), &p, &desc);
                if let Ok(mv) = Guess::decode_move(&state, &text) {
                    let _ = Guess::execute_move(&mut GameCtx::new(), &state, &mv);
                }
                let mut ui = Guess::new_ui(Some(&state));
                Guess::decode_ui(&mut ui, &text, &state);
            }
        }
    }
}
