//! The drawing interface between games and front ends (C's `drawing_api`
//! and `drawing.c`, plus the drawing helpers in `misc.c`; upstream
//! 616da16).
//!
//! Front ends implement [`DrawingApi`]. Games draw through [`Drawing`],
//! which wraps it and adds C's helper functions. Colours are palette
//! indices ([`ColourIdx`]), as in C; a theme turns them into RGB.
//!
//! Blitters are plain [`Blitter`] handles that games create without a
//! drawing, so a game's draw state can be built before anything is drawn
//! (decision: deferred redraws). A front end allocates a blitter's pixels
//! the first time it is saved to.
//!
//! [`RecordingDrawing`] logs every call as a line of text in a format the
//! C oracle's recording drawing API writes too, so draw traces from both
//! implementations compare directly (D1 level 2).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

/// A palette index (C's colour `int`): an entry of the game's palette.
pub type ColourIdx = usize;

/// C's `FONT_FIXED` / `FONT_VARIABLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontType {
    /// Monospaced (`FONT_FIXED`, 0).
    Fixed,
    /// Proportional (`FONT_VARIABLE`, 1).
    Variable,
}

/// Horizontal text alignment (C's `ALIGN_H*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HAlign {
    /// `x` is the left edge (`ALIGN_HLEFT`).
    Left,
    /// `x` is the centre (`ALIGN_HCENTRE`).
    Centre,
    /// `x` is the right edge (`ALIGN_HRIGHT`).
    Right,
}

/// Vertical text alignment (C's `ALIGN_V*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VAlign {
    /// `y` is the baseline (`ALIGN_VNORMAL`).
    Baseline,
    /// `y` is the vertical centre (`ALIGN_VCENTRE`).
    Centre,
}

/// Text alignment: C's `ALIGN_*` flags, combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Align {
    /// Horizontal.
    pub h: HAlign,
    /// Vertical.
    pub v: VAlign,
}

impl Align {
    /// `ALIGN_VCENTRE | ALIGN_HCENTRE`, the most common choice.
    pub const CENTRE: Self = Self {
        h: HAlign::Centre,
        v: VAlign::Centre,
    };

    /// C's flag value.
    #[must_use]
    pub const fn to_raw(self) -> i32 {
        let h = match self.h {
            HAlign::Left => 0,
            HAlign::Centre => 1,
            HAlign::Right => 2,
        };
        let v = match self.v {
            VAlign::Baseline => 0,
            VAlign::Centre => 0x100,
        };
        h | v
    }
}

/// A rectangle of saved pixels that a game can put back later (C's
/// `blitter`), for example the background under a dragged piece.
///
/// A plain handle: creating one needs no drawing. The front end keys its
/// pixel storage by [`id`](Self::id), allocates it on the first
/// [`DrawingApi::blitter_save`], and can free it once
/// [`watch`](Self::watch) says the handle is gone (C's `blitter_free`).
#[derive(Debug)]
pub struct Blitter {
    id: u64,
    w: i32,
    h: i32,
    alive: Arc<()>,
}

impl PartialEq for Blitter {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Blitter {}

impl std::hash::Hash for Blitter {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Tells a front end whether a [`Blitter`] still exists, so it can free
/// the pixels it keeps for one after the game drops it.
#[derive(Clone, Debug)]
pub struct BlitterWatch(Weak<()>);

impl BlitterWatch {
    /// True while the blitter exists.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}

impl Blitter {
    /// A new blitter of `w` × `h` pixels (C's `blitter_new`), with an id
    /// no other blitter in this process has.
    #[doc(alias = "blitter_new")]
    #[must_use]
    pub fn new(w: i32, h: i32) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            w,
            h,
            alive: Arc::new(()),
        }
    }

    /// A watch that reports when this blitter is dropped.
    #[must_use]
    pub fn watch(&self) -> BlitterWatch {
        BlitterWatch(Arc::downgrade(&self.alive))
    }

    /// The unique id.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> i32 {
        self.w
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> i32 {
        self.h
    }
}

/// What a front end implements to draw puzzles (C's `drawing_api`,
/// minus the printing hooks, which come with printing).
///
/// Coordinates are pixels from the top left of the puzzle area. Every
/// colour is a palette index.
pub trait DrawingApi {
    /// Draws `text` at `(x, y)`, aligned as `align` says (C's
    /// `draw_text`). `size` is the font height in pixels.
    #[allow(clippy::too_many_arguments, reason = "mirrors C's draw_text")]
    fn draw_text(
        &mut self,
        x: i32,
        y: i32,
        font: FontType,
        size: i32,
        align: Align,
        colour: ColourIdx,
        text: &str,
    );

    /// Fills a `w` × `h` rectangle (C's `draw_rect`).
    fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: ColourIdx);

    /// Draws a one-pixel line, both ends included (C's `draw_line`).
    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: ColourIdx);

    /// Draws a closed polygon, filled if `fill` is given (C's
    /// `draw_polygon`, where -1 means no fill).
    fn draw_polygon(&mut self, points: &[(i32, i32)], fill: Option<ColourIdx>, outline: ColourIdx);

    /// Draws a circle, filled if `fill` is given (C's `draw_circle`).
    fn draw_circle(
        &mut self,
        cx: i32,
        cy: i32,
        radius: i32,
        fill: Option<ColourIdx>,
        outline: ColourIdx,
    );

    /// Draws a line of the given thickness, at least 1 (C's
    /// `draw_thick_line`; [`Drawing`] clamps the thickness as C does).
    fn draw_thick_line(
        &mut self,
        thickness: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        colour: ColourIdx,
    );

    /// Marks a rectangle as changed, for front ends that copy only
    /// changed areas to the screen (C's `draw_update`, optional there).
    fn draw_update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let _ = (x, y, w, h);
    }

    /// Restricts drawing to a rectangle until [`unclip`](Self::unclip)
    /// (C's `clip`).
    fn clip(&mut self, x: i32, y: i32, w: i32, h: i32);

    /// Removes the clip rectangle (C's `unclip`).
    fn unclip(&mut self);

    /// Called before a redraw (C's `start_draw`).
    fn start_draw(&mut self) {}

    /// Called after a redraw (C's `end_draw`).
    fn end_draw(&mut self) {}

    /// Saves the pixels at `(x, y)` into the blitter (C's `blitter_save`).
    /// Areas outside the drawing may be saved; they come back unchanged.
    fn blitter_save(&mut self, blitter: &Blitter, x: i32, y: i32);

    /// Draws the blitter's saved pixels at `(x, y)` (C's `blitter_load`).
    fn blitter_load(&mut self, blitter: &Blitter, x: i32, y: i32);

    /// Picks the first of `strings` this front end can draw (C's
    /// `text_fallback`). The default, C's for front ends without one,
    /// picks the first that is plain ASCII.
    fn text_fallback<'s>(&self, strings: &[&'s str]) -> Option<&'s str> {
        strings.iter().copied().find(|s| s.is_ascii())
    }
}

/// What games draw with: a [`DrawingApi`] plus C's helpers from
/// `drawing.c` and `misc.c`. Colours can be given as a game's palette
/// enum.
pub struct Drawing<'a> {
    api: &'a mut dyn DrawingApi,
    status: Option<String>,
}

impl fmt::Debug for Drawing<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Drawing")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl<'a> Drawing<'a> {
    /// Wraps a drawing implementation (C's `drawing_new`).
    pub fn new(api: &'a mut dyn DrawingApi) -> Self {
        Self { api, status: None }
    }

    /// The wrapped implementation.
    pub fn api(&mut self) -> &mut dyn DrawingApi {
        self.api
    }

    /// C's `draw_text`.
    #[allow(clippy::too_many_arguments, reason = "mirrors C's draw_text")]
    pub fn text(
        &mut self,
        x: i32,
        y: i32,
        font: FontType,
        size: i32,
        align: Align,
        colour: impl Into<ColourIdx>,
        text: &str,
    ) {
        self.api
            .draw_text(x, y, font, size, align, colour.into(), text);
    }

    /// C's `draw_rect`.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: impl Into<ColourIdx>) {
        self.api.draw_rect(x, y, w, h, colour.into());
    }

    /// C's `draw_line`.
    pub fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: impl Into<ColourIdx>) {
        self.api.draw_line(x1, y1, x2, y2, colour.into());
    }

    /// C's `draw_polygon`.
    pub fn polygon(
        &mut self,
        points: &[(i32, i32)],
        fill: Option<impl Into<ColourIdx>>,
        outline: impl Into<ColourIdx>,
    ) {
        self.api
            .draw_polygon(points, fill.map(Into::into), outline.into());
    }

    /// C's `draw_circle`.
    pub fn circle(
        &mut self,
        cx: i32,
        cy: i32,
        radius: i32,
        fill: Option<impl Into<ColourIdx>>,
        outline: impl Into<ColourIdx>,
    ) {
        self.api
            .draw_circle(cx, cy, radius, fill.map(Into::into), outline.into());
    }

    /// C's `draw_thick_line`: a thickness below 1 is drawn as 1.
    #[allow(clippy::too_many_arguments, reason = "mirrors C's draw_thick_line")]
    pub fn thick_line(
        &mut self,
        thickness: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        colour: impl Into<ColourIdx>,
    ) {
        let thickness = if thickness < 1.0 { 1.0 } else { thickness };
        self.api
            .draw_thick_line(thickness, x1, y1, x2, y2, colour.into());
    }

    /// C's `draw_update`.
    pub fn update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.api.draw_update(x, y, w, h);
    }

    /// C's `clip`.
    pub fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.api.clip(x, y, w, h);
    }

    /// C's `unclip`.
    pub fn unclip(&mut self) {
        self.api.unclip();
    }

    /// C's `blitter_save`.
    pub fn blitter_save(&mut self, blitter: &Blitter, x: i32, y: i32) {
        self.api.blitter_save(blitter, x, y);
    }

    /// C's `blitter_load`.
    pub fn blitter_load(&mut self, blitter: &Blitter, x: i32, y: i32) {
        self.api.blitter_load(blitter, x, y);
    }

    /// The outline of a `w` × `h` rectangle, as an unfilled polygon (C's
    /// `draw_rect_outline`).
    #[doc(alias = "draw_rect_outline")]
    pub fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, colour: impl Into<ColourIdx>) {
        let (x0, x1, y0, y1) = (x, x + w - 1, y, y + h - 1);
        self.api.draw_polygon(
            &[(x0, y0), (x0, y1), (x1, y1), (x1, y0)],
            None,
            colour.into(),
        );
    }

    /// The four corners of a square of half-size `r` around `(cx, cy)`,
    /// as eight short lines (C's `draw_rect_corners`, used for cursors).
    #[doc(alias = "draw_rect_corners")]
    pub fn rect_corners(&mut self, cx: i32, cy: i32, r: i32, colour: impl Into<ColourIdx>) {
        let c = colour.into();
        // C's r / 2 truncates towards zero, as Rust's does.
        let half = r / 2;
        // Top left, bottom left, top right, bottom right; each corner's
        // vertical stroke, then its horizontal one.
        for (sx, sy) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
            let (x, y) = (cx + sx * r, cy + sy * r);
            self.api.draw_line(x, y, x, cy + sy * half, c);
            self.api.draw_line(x, y, cx + sx * half, y, c);
        }
    }

    /// Text with a one-pixel outline drawn first in `outline`, if given
    /// (C's `draw_text_outline`, where -1 means no outline).
    #[doc(alias = "draw_text_outline")]
    #[allow(clippy::too_many_arguments, reason = "mirrors C's draw_text_outline")]
    pub fn text_outline(
        &mut self,
        x: i32,
        y: i32,
        font: FontType,
        size: i32,
        align: Align,
        colour: impl Into<ColourIdx>,
        outline: Option<impl Into<ColourIdx>>,
        text: &str,
    ) {
        if let Some(outline) = outline {
            let o = outline.into();
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                self.api
                    .draw_text(x + dx, y + dy, font, size, align, o, text);
            }
        }
        self.api
            .draw_text(x, y, font, size, align, colour.into(), text);
    }

    /// The first of `strings` the front end can draw (C's
    /// `text_fallback`).
    ///
    /// # Panics
    /// If none can be drawn: C asserts, and callers always include a
    /// plain-ASCII choice.
    #[must_use]
    pub fn text_fallback<'s>(&self, strings: &[&'s str]) -> &'s str {
        self.api
            .text_fallback(strings)
            .expect("text_fallback: no drawable string (include a plain-ASCII one)")
    }

    /// Sets the status bar text (C's `status_bar`). The mid-end reads it
    /// after the redraw, adds the timer and hands it to the front end.
    pub fn status_bar(&mut self, text: &str) {
        self.status = Some(text.to_owned());
    }

    /// The status bar text set during this redraw, if any.
    #[must_use]
    pub fn take_status(&mut self) -> Option<String> {
        self.status.take()
    }
}

/// A drawing that discards everything (for tests and headless runs).
#[derive(Clone, Copy, Debug, Default)]
pub struct NullDrawing;

impl DrawingApi for NullDrawing {
    fn draw_text(&mut self, _: i32, _: i32, _: FontType, _: i32, _: Align, _: ColourIdx, _: &str) {}
    fn draw_rect(&mut self, _: i32, _: i32, _: i32, _: i32, _: ColourIdx) {}
    fn draw_line(&mut self, _: i32, _: i32, _: i32, _: i32, _: ColourIdx) {}
    fn draw_polygon(&mut self, _: &[(i32, i32)], _: Option<ColourIdx>, _: ColourIdx) {}
    fn draw_circle(&mut self, _: i32, _: i32, _: i32, _: Option<ColourIdx>, _: ColourIdx) {}
    fn draw_thick_line(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: ColourIdx) {}
    fn clip(&mut self, _: i32, _: i32, _: i32, _: i32) {}
    fn unclip(&mut self) {}
    fn blitter_save(&mut self, _: &Blitter, _: i32, _: i32) {}
    fn blitter_load(&mut self, _: &Blitter, _: i32, _: i32) {}
}

/// A drawing that records every call, for draw-trace tests.
///
/// Its [`Display`](fmt::Display) output is one line per call, in the
/// format the C oracle's recording drawing API writes: integers in
/// decimal, a missing fill as `-1`, floats as the hex of their bits (so
/// they compare exactly), text last and verbatim.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordingDrawing {
    /// The calls, in order.
    pub calls: Vec<DrawCall>,
}

/// One recorded drawing call.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs, reason = "fields mirror the DrawingApi parameters")]
pub enum DrawCall {
    Text {
        x: i32,
        y: i32,
        font: FontType,
        size: i32,
        align: Align,
        colour: ColourIdx,
        text: String,
    },
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        colour: ColourIdx,
    },
    Line {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        colour: ColourIdx,
    },
    Polygon {
        points: Vec<(i32, i32)>,
        fill: Option<ColourIdx>,
        outline: ColourIdx,
    },
    Circle {
        cx: i32,
        cy: i32,
        radius: i32,
        fill: Option<ColourIdx>,
        outline: ColourIdx,
    },
    ThickLine {
        thickness: f32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        colour: ColourIdx,
    },
    Update {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    Clip {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    Unclip,
    StartDraw,
    EndDraw,
    BlitterSave {
        w: i32,
        h: i32,
        x: i32,
        y: i32,
    },
    BlitterLoad {
        w: i32,
        h: i32,
        x: i32,
        y: i32,
    },
}

/// A palette index or C's -1 for none.
struct Fill(Option<ColourIdx>);

impl fmt::Display for Fill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(c) => write!(f, "{c}"),
            None => f.write_str("-1"),
        }
    }
}

/// A float as the hex of its bits.
struct Bits(f32);

impl fmt::Display for Bits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0.to_bits())
    }
}

impl fmt::Display for DrawCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text {
                x,
                y,
                font,
                size,
                align,
                colour,
                text,
            } => {
                let font = match font {
                    FontType::Fixed => 0,
                    FontType::Variable => 1,
                };
                let align = align.to_raw();
                write!(f, "text {x} {y} {font} {size} {align} {colour} {text}")
            }
            Self::Rect { x, y, w, h, colour } => write!(f, "rect {x} {y} {w} {h} {colour}"),
            Self::Line {
                x1,
                y1,
                x2,
                y2,
                colour,
            } => write!(f, "line {x1} {y1} {x2} {y2} {colour}"),
            Self::Polygon {
                points,
                fill,
                outline,
            } => {
                write!(f, "polygon {} {outline} {}", Fill(*fill), points.len())?;
                for (x, y) in points {
                    write!(f, " {x} {y}")?;
                }
                Ok(())
            }
            Self::Circle {
                cx,
                cy,
                radius,
                fill,
                outline,
            } => write!(f, "circle {cx} {cy} {radius} {} {outline}", Fill(*fill)),
            Self::ThickLine {
                thickness,
                x1,
                y1,
                x2,
                y2,
                colour,
            } => write!(
                f,
                "thick {} {} {} {} {} {colour}",
                Bits(*thickness),
                Bits(*x1),
                Bits(*y1),
                Bits(*x2),
                Bits(*y2)
            ),
            Self::Update { x, y, w, h } => write!(f, "update {x} {y} {w} {h}"),
            Self::Clip { x, y, w, h } => write!(f, "clip {x} {y} {w} {h}"),
            Self::Unclip => f.write_str("unclip"),
            Self::StartDraw => f.write_str("start"),
            Self::EndDraw => f.write_str("end"),
            Self::BlitterSave { w, h, x, y } => write!(f, "blitter_save {w} {h} {x} {y}"),
            Self::BlitterLoad { w, h, x, y } => write!(f, "blitter_load {w} {h} {x} {y}"),
        }
    }
}

impl fmt::Display for RecordingDrawing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for call in &self.calls {
            writeln!(f, "{call}")?;
        }
        Ok(())
    }
}

impl DrawingApi for RecordingDrawing {
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
        self.calls.push(DrawCall::Text {
            x,
            y,
            font,
            size,
            align,
            colour,
            text: text.to_owned(),
        });
    }

    fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: ColourIdx) {
        self.calls.push(DrawCall::Rect { x, y, w, h, colour });
    }

    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: ColourIdx) {
        self.calls.push(DrawCall::Line {
            x1,
            y1,
            x2,
            y2,
            colour,
        });
    }

    fn draw_polygon(&mut self, points: &[(i32, i32)], fill: Option<ColourIdx>, outline: ColourIdx) {
        self.calls.push(DrawCall::Polygon {
            points: points.to_vec(),
            fill,
            outline,
        });
    }

    fn draw_circle(
        &mut self,
        cx: i32,
        cy: i32,
        radius: i32,
        fill: Option<ColourIdx>,
        outline: ColourIdx,
    ) {
        self.calls.push(DrawCall::Circle {
            cx,
            cy,
            radius,
            fill,
            outline,
        });
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
        self.calls.push(DrawCall::ThickLine {
            thickness,
            x1,
            y1,
            x2,
            y2,
            colour,
        });
    }

    fn draw_update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.calls.push(DrawCall::Update { x, y, w, h });
    }

    fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.calls.push(DrawCall::Clip { x, y, w, h });
    }

    fn unclip(&mut self) {
        self.calls.push(DrawCall::Unclip);
    }

    fn start_draw(&mut self) {
        self.calls.push(DrawCall::StartDraw);
    }

    fn end_draw(&mut self) {
        self.calls.push(DrawCall::EndDraw);
    }

    fn blitter_save(&mut self, blitter: &Blitter, x: i32, y: i32) {
        self.calls.push(DrawCall::BlitterSave {
            w: blitter.width(),
            h: blitter.height(),
            x,
            y,
        });
    }

    fn blitter_load(&mut self, blitter: &Blitter, x: i32, y: i32) {
        self.calls.push(DrawCall::BlitterLoad {
            w: blitter.width(),
            h: blitter.height(),
            x,
            y,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws through `Drawing` and returns the trace.
    fn trace(draw: impl FnOnce(&mut Drawing<'_>)) -> String {
        let mut rec = RecordingDrawing::default();
        draw(&mut Drawing::new(&mut rec));
        rec.to_string()
    }

    #[test]
    fn a_blitter_watch_sees_the_drop() {
        let a = Blitter::new(4, 4);
        let b = Blitter::new(4, 4);
        assert_ne!(a, b);
        let watch = a.watch();
        assert!(watch.is_alive());
        drop(a);
        assert!(!watch.is_alive());
        assert!(b.watch().is_alive());
    }

    // The expected traces below are C's: the oracle test
    // `trace_format_is_cs` checks the same calls against drawing.c.

    #[test]
    fn rect_corners_draws_eight_strokes_in_cs_order() {
        let t = trace(|dr| dr.rect_corners(10, 10, 5, 1_usize));
        assert_eq!(
            t,
            "line 5 5 5 8 1\nline 5 5 8 5 1\nline 5 15 5 12 1\nline 5 15 8 15 1\n\
             line 15 5 15 8 1\nline 15 5 12 5 1\nline 15 15 15 12 1\nline 15 15 12 15 1\n"
        );
    }

    #[test]
    fn thick_lines_are_at_least_one_pixel() {
        let t = trace(|dr| {
            dr.thick_line(0.5, 1.0, 2.0, 3.5, 4.0, 7_usize);
            dr.thick_line(2.5, 0.0, 0.0, 1.0, 1.0, 7_usize);
        });
        assert_eq!(
            t,
            "thick 3f800000 3f800000 40000000 40600000 40800000 7\n\
             thick 40200000 00000000 00000000 3f800000 3f800000 7\n"
        );
    }

    #[test]
    fn text_outline_draws_four_offsets_then_the_text() {
        let left_baseline = Align {
            h: HAlign::Left,
            v: VAlign::Baseline,
        };
        let t = trace(|dr| {
            dr.text_outline(
                1,
                1,
                FontType::Variable,
                8,
                left_baseline,
                2_usize,
                Some(9_usize),
                "x",
            );
            dr.text_outline(
                1,
                1,
                FontType::Fixed,
                8,
                Align::CENTRE,
                2_usize,
                None::<usize>,
                "y",
            );
        });
        assert_eq!(
            t,
            "text 0 1 1 8 0 9 x\ntext 2 1 1 8 0 9 x\ntext 1 0 1 8 0 9 x\ntext 1 2 1 8 0 9 x\n\
             text 1 1 1 8 0 2 x\ntext 1 1 0 8 257 2 y\n"
        );
    }

    #[test]
    fn rect_outline_is_an_unfilled_polygon() {
        let t = trace(|dr| dr.rect_outline(2, 3, 4, 5, 6_usize));
        assert_eq!(t, "polygon -1 6 4 2 3 2 7 5 7 5 3\n");
    }

    #[test]
    fn status_text_is_kept_for_the_mid_end() {
        let mut rec = RecordingDrawing::default();
        let mut dr = Drawing::new(&mut rec);
        dr.status_bar("Moves: 3");
        assert_eq!(dr.take_status().as_deref(), Some("Moves: 3"));
        assert_eq!(dr.take_status(), None);
        assert_eq!(rec.calls, []);
    }

    #[test]
    fn text_fallback_prefers_the_first_ascii_choice() {
        let mut rec = RecordingDrawing::default();
        let dr = Drawing::new(&mut rec);
        assert_eq!(dr.text_fallback(&["\u{221a}", "sqrt", "v"]), "sqrt");
    }

    #[test]
    fn blitters_have_unique_ids() {
        let a = Blitter::new(3, 4);
        let b = Blitter::new(3, 4);
        assert_ne!(a.id(), b.id());
        assert_eq!((a.width(), a.height()), (3, 4));
    }
}
