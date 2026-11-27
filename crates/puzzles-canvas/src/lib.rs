//! The puzzle canvas: a `tiny-skia` pixmap implementing [`DrawingApi`]
//! (D6a and the canvas crate decision in `docs/porting-plan.md`). It
//! knows nothing about the UI toolkit, so the desktop app, `puzzles-cli
//! render` and snapshot tests share it.
//!
//! The pixmap persists between redraws, as the games draw incrementally.
//! How it draws is chosen for looks and simplicity, not to copy a C
//! front end (canvas details decision): rectangles have hard edges,
//! one-pixel lines run through pixel centres with both ends included,
//! and polygons, circles and thick lines are anti-aliased. Text is
//! described in [`text`].

use std::collections::HashMap;
use std::fmt;

use puzzles_core::drawing::{Align, Blitter, BlitterWatch, ColourIdx, DrawingApi, FontType};
use puzzles_theme::Rgb;
use tiny_skia::{
    BlendMode, Color, FillRule, IntRect, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap,
    PixmapPaint, PremultipliedColorU8, Rect, Stroke, Transform,
};

pub use tiny_skia;

mod text;

/// A canvas size tiny-skia can't allocate: zero, or too large.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeError {
    /// The width asked for.
    pub width: u32,
    /// The height asked for.
    pub height: u32,
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "can't make a {} × {} canvas", self.width, self.height)
    }
}

impl std::error::Error for SizeError {}

/// Where drawing may go.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Clip {
    /// Anywhere.
    None,
    /// Nowhere: the clip rectangle is off the canvas or empty.
    Empty,
    /// Inside this rectangle, which `Canvas::mask` also holds.
    Rect(IntRect),
}

/// A blitter's saved pixels, kept while its handle exists.
struct Saved {
    watch: BlitterWatch,
    pixels: Pixmap,
}

/// A persistent puzzle canvas. Colours are the game's palette resolved
/// by a theme, in palette order.
pub struct Canvas {
    pixmap: Pixmap,
    colours: Vec<Color>,
    clip: Clip,
    /// Opaque inside the clip rectangle, transparent elsewhere; kept
    /// allocated so clipping each tile costs only the rectangle's area.
    mask: Mask,
    blitters: HashMap<u64, Saved>,
    dirty: Option<IntRect>,
}

impl fmt::Debug for Canvas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Canvas")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("colours", &self.colours.len())
            .field("clip", &self.clip)
            .field("blitters", &self.blitters.len())
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

fn to_color(rgb: Rgb) -> Color {
    let [r, g, b] = rgb.to_rgb8();
    Color::from_rgba8(r, g, b, 255)
}

fn new_pixmap(width: u32, height: u32) -> Result<(Pixmap, Mask), SizeError> {
    let error = SizeError { width, height };
    let pixmap = Pixmap::new(width, height).ok_or(error)?;
    let mask = Mask::new(width, height).ok_or(error)?;
    Ok((pixmap, mask))
}

impl Canvas {
    /// A `width` × `height` canvas filled with the first colour (every
    /// game's background), or white without colours.
    ///
    /// # Errors
    /// A zero or too large size.
    pub fn new(width: u32, height: u32, colours: &[Rgb]) -> Result<Self, SizeError> {
        let (pixmap, mask) = new_pixmap(width, height)?;
        let mut canvas = Self {
            pixmap,
            colours: colours.iter().copied().map(to_color).collect(),
            clip: Clip::None,
            mask,
            blitters: HashMap::new(),
            dirty: None,
        };
        canvas.fill_background();
        Ok(canvas)
    }

    /// Replaces the pixels with a blank `width` × `height` canvas (the
    /// front end then asks the mid-end for a full redraw). The clip is
    /// removed; saved blitters are kept.
    ///
    /// # Errors
    /// A zero or too large size; the canvas is unchanged then.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SizeError> {
        let (pixmap, mask) = new_pixmap(width, height)?;
        self.pixmap = pixmap;
        self.mask = mask;
        self.clip = Clip::None;
        self.fill_background();
        Ok(())
    }

    /// Replaces the colours, such as after a theme change (then redraw
    /// in full).
    pub fn set_colours(&mut self, colours: &[Rgb]) {
        self.colours = colours.iter().copied().map(to_color).collect();
    }

    /// Fills the canvas with the background colour and marks it all
    /// changed.
    pub fn fill_background(&mut self) {
        self.pixmap
            .fill(self.colours.first().copied().unwrap_or(Color::WHITE));
        self.dirty = IntRect::from_xywh(0, 0, self.width(), self.height());
    }

    /// The pixels.
    #[must_use]
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// The area changed since the last call, for uploading to the screen:
    /// everything drawn (with a pixel's margin for anti-aliasing), any
    /// refill, and the games' `draw_update` calls. Games' `draw_update`
    /// is only a hint in C, and the canvas's shapes can reach a pixel
    /// past what a game reports, so the canvas doesn't rely on it.
    pub fn take_dirty(&mut self) -> Option<IntRect> {
        self.dirty.take()
    }

    /// How many blitters hold saved pixels.
    #[must_use]
    pub fn saved_blitters(&self) -> usize {
        self.blitters.len()
    }

    fn colour(&self, idx: ColourIdx) -> Option<Color> {
        self.colours.get(idx).copied()
    }

    /// True when the clip hides everything.
    fn clipped_out(&self) -> bool {
        self.clip == Clip::Empty
    }

    /// The rectangle drawing may touch, or `None` if nothing.
    fn visible(&self) -> Option<IntRect> {
        match self.clip {
            Clip::None => IntRect::from_xywh(0, 0, self.width(), self.height()),
            Clip::Empty => None,
            Clip::Rect(r) => Some(r),
        }
    }

    /// Records that pixels within `left..right` × `top..bottom` may have
    /// changed (clipped to what drawing can touch).
    fn touch(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        let Some(visible) = self.visible() else {
            return;
        };
        let clamp = |v: f32| v.clamp(-1e9, 1e9) as i32;
        let rect = IntRect::from_ltrb(
            clamp(left.floor()),
            clamp(top.floor()),
            clamp(right.ceil()),
            clamp(bottom.ceil()),
        )
        .and_then(|r| r.intersect(&visible));
        if let Some(rect) = rect {
            self.dirty = Some(match self.dirty {
                Some(d) => union(d, rect),
                None => rect,
            });
        }
    }

    /// [`touch`](Self::touch) for a path's bounds, widened by `margin`.
    fn touch_path(&mut self, path: &tiny_skia::Path, margin: f32) {
        let b = path.bounds();
        self.touch(
            b.left() - margin,
            b.top() - margin,
            b.right() + margin,
            b.bottom() + margin,
        );
    }

    fn set_mask(&mut self, rect: IntRect, value: u8) {
        let stride = self.width() as usize;
        let data = self.mask.data_mut();
        for y in rect.top()..rect.bottom() {
            let row = y as usize * stride;
            data[row + rect.left() as usize..row + rect.right() as usize].fill(value);
        }
    }

    fn fill_path(&mut self, pb: PathBuilder, colour: Color) {
        let Some(path) = pb.finish() else { return };
        if self.clipped_out() {
            return;
        }
        self.touch_path(&path, 1.0);
        let paint = paint(colour, true);
        let mask = clip_mask(self.clip, &self.mask);
        self.pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            mask,
        );
    }

    fn stroke_path(&mut self, pb: PathBuilder, colour: Color, stroke: &Stroke) {
        let Some(path) = pb.finish() else { return };
        if self.clipped_out() {
            return;
        }
        // Square and round caps reach half the width past the ends.
        self.touch_path(&path, stroke.width / 2.0 + 1.0);
        let paint = paint(colour, true);
        let mask = clip_mask(self.clip, &self.mask);
        self.pixmap
            .stroke_path(&path, &paint, stroke, Transform::identity(), mask);
    }

    /// Fills one pixel, for degenerate lines and circles.
    fn dot(&mut self, x: i32, y: i32, colour: ColourIdx) {
        self.draw_rect(x, y, 1, 1, colour);
    }
}

/// The mask to draw through for a clip, or `None` to draw anywhere. A
/// free function so it borrows only the mask, not the whole canvas.
fn clip_mask(clip: Clip, mask: &Mask) -> Option<&Mask> {
    matches!(clip, Clip::Rect(_)).then_some(mask)
}

fn paint(colour: Color, anti_alias: bool) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(colour);
    paint.anti_alias = anti_alias;
    paint
}

fn stroke(width: f32, line_cap: LineCap) -> Stroke {
    Stroke {
        width,
        line_cap,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

impl DrawingApi for Canvas {
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
        let (Some(colour), Some(visible)) = (self.colour(colour), self.visible()) else {
            return;
        };
        let [r, g, b] = [colour.red(), colour.green(), colour.blue()];
        let face = text::face(font);
        let glyphs = text::layout(face, x, y, size, align, text);
        let width = self.width() as usize;
        let pixels = self.pixmap.pixels_mut();
        let mut inked: Option<(i32, i32, i32, i32)> = None;
        text::rasterise(face, glyphs, |px, py, coverage| {
            if px < visible.left()
                || px >= visible.right()
                || py < visible.top()
                || py >= visible.bottom()
            {
                return;
            }
            let i = py as usize * width + px as usize;
            pixels[i] = blend(pixels[i], [r, g, b], coverage);
            inked = Some(match inked {
                Some((l, t, r, b)) => (l.min(px), t.min(py), r.max(px + 1), b.max(py + 1)),
                None => (px, py, px + 1, py + 1),
            });
        });
        if let Some((l, t, r, b)) = inked {
            self.touch(l as f32, t as f32, r as f32, b as f32);
        }
    }

    fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, colour: ColourIdx) {
        let Some(colour) = self.colour(colour).filter(|_| w > 0 && h > 0) else {
            return;
        };
        let Some(rect) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) else {
            return;
        };
        if self.clipped_out() {
            return;
        }
        self.touch(rect.left(), rect.top(), rect.right(), rect.bottom());
        let mask = clip_mask(self.clip, &self.mask);
        self.pixmap
            .fill_rect(rect, &paint(colour, false), Transform::identity(), mask);
    }

    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, colour: ColourIdx) {
        if (x1, y1) == (x2, y2) {
            self.dot(x1, y1, colour);
            return;
        }
        let Some(colour) = self.colour(colour) else {
            return;
        };
        let mut pb = PathBuilder::new();
        pb.move_to(x1 as f32 + 0.5, y1 as f32 + 0.5);
        pb.line_to(x2 as f32 + 0.5, y2 as f32 + 0.5);
        self.stroke_path(pb, colour, &stroke(1.0, LineCap::Square));
    }

    fn draw_polygon(&mut self, points: &[(i32, i32)], fill: Option<ColourIdx>, outline: ColourIdx) {
        let build = || {
            let mut pb = PathBuilder::new();
            for (i, &(x, y)) in points.iter().enumerate() {
                let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
                if i == 0 {
                    pb.move_to(x, y);
                } else {
                    pb.line_to(x, y);
                }
            }
            // The closing edge explicitly: tiny-skia 0.12 anti-aliases
            // an implicit closing edge a quarter pixel off, blurring it.
            if let (Some(&(x, y)), Some(last)) = (points.first(), points.last())
                && last != &(x, y)
            {
                pb.line_to(x as f32 + 0.5, y as f32 + 0.5);
            }
            pb.close();
            pb
        };
        if let Some(fill) = fill.and_then(|c| self.colour(c)) {
            self.fill_path(build(), fill);
        }
        if let Some(outline) = self.colour(outline) {
            self.stroke_path(build(), outline, &stroke(1.0, LineCap::Square));
        }
    }

    fn draw_circle(
        &mut self,
        cx: i32,
        cy: i32,
        radius: i32,
        fill: Option<ColourIdx>,
        outline: ColourIdx,
    ) {
        if radius <= 0 {
            self.dot(cx, cy, outline);
            return;
        }
        let build = || {
            let mut pb = PathBuilder::new();
            pb.push_circle(cx as f32 + 0.5, cy as f32 + 0.5, radius as f32);
            pb
        };
        if let Some(fill) = fill.and_then(|c| self.colour(c)) {
            self.fill_path(build(), fill);
        }
        if let Some(outline) = self.colour(outline) {
            self.stroke_path(build(), outline, &stroke(1.0, LineCap::Butt));
        }
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
        let Some(colour) = self.colour(colour) else {
            return;
        };
        let mut pb = PathBuilder::new();
        pb.move_to(x1, y1);
        pb.line_to(x2, y2);
        self.stroke_path(pb, colour, &stroke(thickness.max(1.0), LineCap::Round));
    }

    fn draw_update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let canvas = IntRect::from_xywh(0, 0, self.width(), self.height());
        let rect = (w > 0 && h > 0)
            .then(|| IntRect::from_xywh(x, y, w as u32, h as u32))
            .flatten()
            .and_then(|r| canvas.and_then(|c| r.intersect(&c)));
        if let Some(rect) = rect {
            self.dirty = Some(match self.dirty {
                Some(d) => union(d, rect),
                None => rect,
            });
        }
    }

    fn clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if let Clip::Rect(old) = self.clip {
            self.set_mask(old, 0);
        }
        let canvas = IntRect::from_xywh(0, 0, self.width(), self.height());
        let rect = (w > 0 && h > 0)
            .then(|| IntRect::from_xywh(x, y, w as u32, h as u32))
            .flatten()
            .and_then(|r| canvas.and_then(|c| r.intersect(&c)));
        self.clip = match rect {
            Some(rect) => {
                self.set_mask(rect, 255);
                Clip::Rect(rect)
            }
            None => Clip::Empty,
        };
    }

    fn unclip(&mut self) {
        if let Clip::Rect(old) = self.clip {
            self.set_mask(old, 0);
        }
        self.clip = Clip::None;
    }

    fn start_draw(&mut self) {
        // Forget the pixels of blitters the game has dropped.
        self.blitters.retain(|_, saved| saved.watch.is_alive());
    }

    fn blitter_save(&mut self, blitter: &Blitter, x: i32, y: i32) {
        let (Ok(w), Ok(h)) = (
            u32::try_from(blitter.width()),
            u32::try_from(blitter.height()),
        ) else {
            return;
        };
        let saved = match self.blitters.entry(blitter.id()) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let Some(pixels) = Pixmap::new(w, h) else {
                    return;
                };
                e.insert(Saved {
                    watch: blitter.watch(),
                    pixels,
                })
            }
        };
        // Areas off the canvas stay transparent, so loading leaves them
        // unchanged.
        saved.pixels.fill(Color::TRANSPARENT);
        let copy = PixmapPaint {
            blend_mode: BlendMode::Source,
            ..PixmapPaint::default()
        };
        saved.pixels.draw_pixmap(
            -x,
            -y,
            self.pixmap.as_ref(),
            &copy,
            Transform::identity(),
            None,
        );
    }

    fn blitter_load(&mut self, blitter: &Blitter, x: i32, y: i32) {
        if self.clipped_out() {
            return;
        }
        let Some((w, h)) = self
            .blitters
            .get(&blitter.id())
            .map(|s| (s.pixels.width() as f32, s.pixels.height() as f32))
        else {
            return;
        };
        self.touch(x as f32, y as f32, x as f32 + w, y as f32 + h);
        let Some(saved) = self.blitters.get(&blitter.id()) else {
            return;
        };
        let mask = clip_mask(self.clip, &self.mask);
        self.pixmap.draw_pixmap(
            x,
            y,
            saved.pixels.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            mask,
        );
    }

    fn text_fallback<'s>(&self, strings: &[&'s str]) -> Option<&'s str> {
        strings.iter().copied().find(|s| {
            text::face(FontType::Variable).covers(s) && text::face(FontType::Fixed).covers(s)
        })
    }
}

fn union(a: IntRect, b: IntRect) -> IntRect {
    let left = a.left().min(b.left());
    let top = a.top().min(b.top());
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    IntRect::from_ltrb(left, top, right, bottom).unwrap_or(a)
}

/// Blends an opaque colour over a pixel with the given coverage.
fn blend(dst: PremultipliedColorU8, [r, g, b]: [f32; 3], coverage: f32) -> PremultipliedColorU8 {
    let keep = 1.0 - coverage;
    let mix = |src: f32, dst: u8| (src * 255.0 * coverage + f32::from(dst) * keep).round() as u8;
    let a = (255.0 * coverage + f32::from(dst.alpha()) * keep).round() as u8;
    PremultipliedColorU8::from_rgba(
        mix(r, dst.red()),
        mix(g, dst.green()),
        mix(b, dst.blue()),
        a,
    )
    .unwrap_or(dst)
}

#[cfg(test)]
mod tests;
