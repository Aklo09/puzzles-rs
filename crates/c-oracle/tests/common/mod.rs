//! Game-level parity driver shared by the `*_parity.rs` tests of ported
//! games: [`Pair`] runs a Rust [`Game`] and the C game side by side
//! through the same back-end calls the mid-end makes, and fails on the
//! first difference in move strings, states (seen through `text_format`
//! and `status`), animation lengths, draw traces or status text.
//!
//! Each game's test supplies the parameters, descriptions and input
//! scripts (usually from proptest strategies).

#![allow(dead_code, reason = "each test binary uses part of the driver")]

use c_oracle::{CGame, CMoveResult, CSession};
use proptest::prelude::*;
use puzzles_core::button::Button;
use puzzles_core::drawing::{
    Align, Blitter, ColourIdx, Drawing, DrawingApi, FontType, RecordingDrawing,
};
use puzzles_core::game::{Game, GameCtx, GameStatus, MoveResult};

/// The Rust half of a [`Pair`]: what `csrc/game_harness.c` keeps for C.
pub struct RustSession<G: Game> {
    pub params: G::Params,
    pub orig: G::State,
    pub prev: Option<G::State>,
    pub cur: G::State,
    pub ui: G::Ui,
    pub ds: G::DrawState,
    pub status: Option<String>,
    /// C's palette index for each of the port's ([`CColours`]); empty
    /// when they are the same.
    pub c_colours: &'static [usize],
}

impl<G: Game> RustSession<G> {
    pub fn new(params: &G::Params, desc: &str, tilesize: i32) -> Self {
        let cur = G::new_game(&mut GameCtx::new(), params, desc);
        let ui = G::new_ui(Some(&cur));
        let mut ds = G::new_drawstate(&cur);
        G::set_size(&mut ds, params, tilesize);
        Self {
            params: params.clone(),
            orig: cur.clone(),
            prev: None,
            cur,
            ui,
            ds,
            status: None,
            c_colours: &[],
        }
    }

    pub fn execute(&mut self, mv: &G::Move) -> bool {
        let Some(next) = G::execute_move(&mut GameCtx::new(), &self.cur, mv) else {
            return false;
        };
        G::changed_state(&mut self.ui, &self.cur, &next);
        self.prev = Some(std::mem::replace(&mut self.cur, next));
        true
    }

    pub fn redraw(&mut self, animate: bool, dir: i32, anim_time: f32, flash_time: f32) -> String {
        let mut rec = RecordingDrawing::default();
        let mut mapped = CColours::new(&mut rec, self.c_colours);
        let mut dr = Drawing::new(&mut mapped);
        let old = if animate { self.prev.as_ref() } else { None };
        G::redraw(
            &mut dr,
            &mut self.ds,
            old,
            &self.cur,
            dir,
            &self.ui,
            anim_time,
            flash_time,
        );
        if let Some(status) = dr.take_status() {
            self.status = Some(status);
        }
        rec.to_string()
    }
}

/// Encodes a move as text.
pub fn encode<G: Game>(mv: &G::Move) -> String {
    let mut s = String::new();
    G::encode_move(mv, &mut s);
    s
}

/// C's `status` value.
pub fn status_code(status: GameStatus) -> i32 {
    match status {
        GameStatus::Ongoing => 0,
        GameStatus::Won => 1,
        GameStatus::Lost => -1,
    }
}

/// One step of a scripted game.
#[derive(Clone, Debug)]
pub enum Step {
    /// An input event at a pixel, as a raw C button code the mid-end
    /// would pass on (modifiers already normalised).
    Input { x: i32, y: i32, button: i32 },
    /// Ask the solver, and play its move if there is one.
    Solve,
    /// Execute a move string directly, as from a save file (untrusted).
    Execute(String),
}

/// A Rust game and its C original, in step.
pub struct Pair<G: Game> {
    pub rust: RustSession<G>,
    pub c: CSession,
    pub tilesize: i32,
}

impl<G: Game> Pair<G> {
    /// Both sides on the same valid parameters and description; draws
    /// the first frame and compares it.
    pub fn new(
        game: CGame,
        params: &G::Params,
        desc: &str,
        tilesize: i32,
    ) -> Result<Self, TestCaseError> {
        Self::with_c_colours(game, params, desc, tilesize, &[])
    }

    /// As [`new`](Self::new), for a port whose palette differs from C's:
    /// `c_colours[i]` is C's index for the port's entry `i`, applied to
    /// the port's draw traces before comparing (see [`CColours`]).
    pub fn with_c_colours(
        game: CGame,
        params: &G::Params,
        desc: &str,
        tilesize: i32,
        c_colours: &'static [usize],
    ) -> Result<Self, TestCaseError> {
        let encoded = G::encode_params(params, true);
        let mut rust = RustSession::new(params, desc, tilesize);
        rust.c_colours = c_colours;
        let mut pair = Self {
            rust,
            c: CSession::new(game, &encoded, desc, tilesize),
            tilesize,
        };
        prop_assert_eq!(
            G::compute_size(params, tilesize, &pair.rust.ui),
            pair.c.size(tilesize)
        );
        pair.compare_state()?;
        pair.compare_redraw(false, 0, 0.0, 0.0)?;
        Ok(pair)
    }

    /// Runs the steps, comparing after each.
    pub fn run(&mut self, steps: &[Step]) -> Result<(), TestCaseError> {
        for step in steps {
            match step {
                &Step::Input { x, y, button } => self.input(x, y, button)?,
                Step::Solve => self.solve()?,
                Step::Execute(mv) => self.execute_text(mv)?,
            }
        }
        Ok(())
    }

    fn input(&mut self, x: i32, y: i32, raw: i32) -> Result<(), TestCaseError> {
        let button = Button::from_raw(raw).expect("scripts use valid buttons");
        prop_assert_eq!(
            G::current_key_label(&self.rust.ui, &self.rust.cur, button).unwrap_or(""),
            self.c.key_label(raw),
            "key label for {:?}",
            button
        );
        let r = G::interpret_move(
            &self.rust.cur,
            &mut self.rust.ui,
            &self.rust.ds,
            x,
            y,
            button,
        );
        let c = self.c.interpret(x, y, raw);
        let r_text = match &r {
            MoveResult::Unused => CMoveResult::Unused,
            MoveResult::NoEffect => CMoveResult::NoEffect,
            MoveResult::UiUpdate => CMoveResult::UiUpdate,
            MoveResult::Move(mv) => CMoveResult::Move(encode::<G>(mv)),
        };
        if let (MoveResult::UiUpdate | MoveResult::NoEffect, CMoveResult::Move(text)) = (&r, &c) {
            // A move C's own execute_move rejects would abort C's mid-end
            // (it asserts on it); a port may do anything else there.
            prop_assert!(
                !self.c.execute(text),
                "C made a move the port didn't: {}",
                text
            );
        } else {
            prop_assert_eq!(&r_text, &c, "interpret_move({}, {}, {:?})", x, y, button);
        }
        match r {
            MoveResult::Move(mv) => self.play(&mv),
            MoveResult::UiUpdate => {
                self.compare_state()?;
                self.compare_redraw(false, 0, 0.0, 0.0)
            }
            MoveResult::Unused | MoveResult::NoEffect => Ok(()),
        }
    }

    fn solve(&mut self) -> Result<(), TestCaseError> {
        let r = G::solve(&self.rust.orig, &self.rust.cur, None);
        let c = self.c.solve(None);
        match (r, c) {
            (Ok(mv), Ok(text)) => {
                prop_assert_eq!(encode::<G>(&mv), text);
                self.play(&mv)
            }
            (Err(e), Err(text)) => {
                prop_assert_eq!(e.message(), text);
                Ok(())
            }
            (r, c) => Err(TestCaseError::fail(format!("solve: Rust {r:?}, C {c:?}"))),
        }
    }

    /// Plays a move both sides made: the codec must round-trip C's text,
    /// both must accept it, and the animation must match frame by frame.
    fn play(&mut self, mv: &G::Move) -> Result<(), TestCaseError> {
        let text = encode::<G>(mv);
        let decoded = G::decode_move(&self.rust.cur, &text);
        prop_assert_eq!(decoded.as_ref(), Ok(mv), "decode_move({})", text);
        prop_assert!(self.rust.execute(mv), "Rust rejected its own move {}", text);
        prop_assert!(self.c.execute(&text), "C rejected {}", text);
        self.after_move()
    }

    /// A move string from outside (a save file): accepted by both or by
    /// neither.
    fn execute_text(&mut self, text: &str) -> Result<(), TestCaseError> {
        let rust_ok = G::decode_move(&self.rust.cur, text).is_ok_and(|mv| self.rust.execute(&mv));
        let c_ok = self.c.execute(text);
        prop_assert_eq!(rust_ok, c_ok, "execute_move({:?})", text);
        if rust_ok {
            self.after_move()?;
        }
        Ok(())
    }

    fn after_move(&mut self) -> Result<(), TestCaseError> {
        self.compare_state()?;
        let prev = self.rust.prev.as_ref().expect("a move was made");
        let anim = G::anim_length(prev, &self.rust.cur, 1, &mut self.rust.ui);
        prop_assert_eq!(
            anim.to_bits(),
            self.c.anim_length(1).to_bits(),
            "anim_length"
        );
        let prev = self.rust.prev.as_ref().expect("a move was made");
        let flash = G::flash_length(prev, &self.rust.cur, 1, &mut self.rust.ui);
        prop_assert_eq!(
            flash.to_bits(),
            self.c.flash_length(1).to_bits(),
            "flash_length"
        );

        // The mid-end's frames: the animation, then the flash, then still.
        // Frames through the animation, including near its end.
        for fraction in [0.0_f32, 0.25, 0.5, 0.75, 0.9, 0.97, 0.999] {
            let t = anim * fraction;
            if t < anim {
                self.compare_redraw(true, 1, t, 0.0)?;
            }
        }
        for k in 0..6_u8 {
            let t = flash * f32::from(k) / 6.0;
            if t < flash {
                self.compare_redraw(false, 1, 0.0, t)?;
            }
        }
        self.compare_redraw(false, 1, 0.0, 0.0)
    }

    fn compare_state(&mut self) -> Result<(), TestCaseError> {
        let params = &self.rust.params;
        let rust_text = G::can_format_as_text_now(params)
            .then(|| G::text_format(&self.rust.cur))
            .flatten();
        prop_assert_eq!(rust_text, self.c.text_format(), "text_format");
        prop_assert_eq!(status_code(G::status(&self.rust.cur)), self.c.status());
        let cursor = G::cursor_location(&self.rust.ui, &self.rust.ds, &self.rust.cur, params)
            .map(|r| (r.x, r.y, r.w, r.h));
        prop_assert_eq!(cursor, self.c.cursor(), "cursor location");
        prop_assert_eq!(G::encode_ui(&self.rust.ui), self.c.encode_ui(), "saved UI");
        Ok(())
    }

    fn compare_redraw(
        &mut self,
        animate: bool,
        dir: i32,
        anim_time: f32,
        flash_time: f32,
    ) -> Result<(), TestCaseError> {
        let rust = self.rust.redraw(animate, dir, anim_time, flash_time);
        let (c, c_status) = self.c.redraw(animate, dir, anim_time, flash_time);
        prop_assert_eq!(
            &rust,
            &c,
            "redraw(animate {}, dir {}, anim {}, flash {})",
            animate,
            dir,
            anim_time,
            flash_time
        );
        prop_assert_eq!(&self.rust.status, &c_status, "status bar");
        Ok(())
    }
}

/// A drawing that passes every call on with the port's palette indices
/// replaced by C's, for a port that splits a C colour into entries of
/// its own: C's aliases (Mosaic's `COL_TEXT_DARK = COL_MARKED`) draw
/// with the aliased colour's index, the port with the new entry's. An
/// empty map changes nothing.
pub struct CColours<'a> {
    inner: &'a mut dyn DrawingApi,
    map: &'a [usize],
}

impl<'a> CColours<'a> {
    pub fn new(inner: &'a mut dyn DrawingApi, map: &'a [usize]) -> Self {
        Self { inner, map }
    }

    fn c(&self, colour: ColourIdx) -> ColourIdx {
        self.map.get(colour).copied().unwrap_or(colour)
    }
}

impl DrawingApi for CColours<'_> {
    fn draw_text(
        &mut self,
        x: i32,
        y: i32,
        font: FontType,
        size: i32,
        align: Align,
        colour: ColourIdx,
        text: &str,
    ) {
        let colour = self.c(colour);
        self.inner.draw_text(x, y, font, size, align, colour, text);
    }

    fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: ColourIdx) {
        let colour = self.c(colour);
        self.inner.draw_rect(x, y, w, h, colour);
    }

    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: ColourIdx) {
        let colour = self.c(colour);
        self.inner.draw_line(x1, y1, x2, y2, colour);
    }

    fn draw_polygon(&mut self, points: &[(i32, i32)], fill: Option<ColourIdx>, outline: ColourIdx) {
        let (fill, outline) = (fill.map(|f| self.c(f)), self.c(outline));
        self.inner.draw_polygon(points, fill, outline);
    }

    fn draw_circle(
        &mut self,
        cx: i32,
        cy: i32,
        radius: i32,
        fill: Option<ColourIdx>,
        outline: ColourIdx,
    ) {
        let (fill, outline) = (fill.map(|f| self.c(f)), self.c(outline));
        self.inner.draw_circle(cx, cy, radius, fill, outline);
    }

    fn draw_thick_line(
        &mut self,
        thickness: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        colour: ColourIdx,
    ) {
        let colour = self.c(colour);
        self.inner
            .draw_thick_line(thickness, x1, y1, x2, y2, colour);
    }

    fn draw_update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.inner.draw_update(x, y, w, h);
    }

    fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.inner.clip(x, y, w, h);
    }

    fn unclip(&mut self) {
        self.inner.unclip();
    }

    fn start_draw(&mut self) {
        self.inner.start_draw();
    }

    fn end_draw(&mut self) {
        self.inner.end_draw();
    }

    fn blitter_save(&mut self, blitter: &Blitter, x: i32, y: i32) {
        self.inner.blitter_save(blitter, x, y);
    }

    fn blitter_load(&mut self, blitter: &Blitter, x: i32, y: i32) {
        self.inner.blitter_load(blitter, x, y);
    }

    fn text_fallback<'s>(&self, strings: &[&'s str]) -> Option<&'s str> {
        self.inner.text_fallback(strings)
    }
}
