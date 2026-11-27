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
use puzzles_core::drawing::{Drawing, RecordingDrawing};
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
        let mut dr = Drawing::new(&mut rec);
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
        let encoded = G::encode_params(params, true);
        let mut pair = Self {
            rust: RustSession::new(params, desc, tilesize),
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
        prop_assert_eq!(&r_text, &c, "interpret_move({}, {}, {:?})", x, y, button);
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
