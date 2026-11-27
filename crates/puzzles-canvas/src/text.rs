//! Text: the bundled fonts, laid out left to right and rasterised with
//! `ab_glyph` coverage (canvas details decision in
//! `docs/porting-plan.md`).
//!
//! * `size` is the em height in pixels.
//! * [`VAlign::Centre`] puts the middle of the capital height at `y`, so
//!   every string shares one baseline and digits look centred;
//!   [`VAlign::Baseline`] puts the baseline at `y`.
//! * Horizontal alignment uses advance widths plus kerning. There is no
//!   shaping, which puzzle text doesn't need.
//! * The start point and baseline are whole pixels, so stems stay sharp.

use std::sync::LazyLock;

use ab_glyph::{Font, FontRef, Glyph, PxScale, ScaleFont, point};
use puzzles_core::drawing::{Align, FontType, HAlign, VAlign};

/// A bundled font with the metrics layout needs.
pub(crate) struct Face {
    font: FontRef<'static>,
    /// Units per em.
    em: f32,
    /// Capital height in font units.
    cap_height: f32,
}

impl Face {
    fn load(data: &'static [u8]) -> Self {
        let font = FontRef::try_from_slice(data).expect("bundled fonts are valid");
        let em = font.units_per_em().unwrap_or(1000.0);
        // 'H' sits on the baseline, so its outline's height is the
        // capital height (ab_glyph's bounds run downwards from the top).
        let cap_height = font
            .outline(font.glyph_id('H'))
            .map_or(font.ascent_unscaled() * 0.7, |o| {
                (o.bounds.max.y - o.bounds.min.y).abs()
            });
        Self {
            font,
            em,
            cap_height,
        }
    }

    /// True if the font has a glyph for every character.
    pub(crate) fn covers(&self, text: &str) -> bool {
        text.chars().all(|c| self.font.glyph_id(c).0 != 0)
    }
}

static VARIABLE: LazyLock<Face> =
    LazyLock::new(|| Face::load(include_bytes!("../../../assets/fonts/Inter-Bold.ttf")));
static FIXED: LazyLock<Face> = LazyLock::new(|| {
    Face::load(include_bytes!(
        "../../../assets/fonts/JetBrainsMonoNL-Bold.ttf"
    ))
});

/// The face for a font type: Inter for proportional text, `JetBrains`
/// Mono for fixed.
pub(crate) fn face(font: FontType) -> &'static Face {
    match font {
        FontType::Variable => &VARIABLE,
        FontType::Fixed => &FIXED,
    }
}

/// `text` laid out at `size` pixels and aligned at `(x, y)`: glyphs
/// positioned on their baseline.
pub(crate) fn layout(
    face: &Face,
    x: i32,
    y: i32,
    size: i32,
    align: Align,
    text: &str,
) -> Vec<Glyph> {
    let size = size.max(1) as f32;
    let scale = PxScale::from(size * face.font.height_unscaled() / face.em);
    let scaled = face.font.as_scaled(scale);

    let mut glyphs = Vec::with_capacity(text.len());
    let mut caret = 0.0_f32;
    let mut previous = None;
    for c in text.chars() {
        let id = face.font.glyph_id(c);
        if let Some(previous) = previous {
            caret += scaled.kern(previous, id);
        }
        glyphs.push(id.with_scale_and_position(scale, point(caret, 0.0)));
        caret += scaled.h_advance(id);
        previous = Some(id);
    }

    let left = match align.h {
        HAlign::Left => x as f32,
        HAlign::Centre => x as f32 - caret / 2.0,
        HAlign::Right => x as f32 - caret,
    }
    .round();
    let baseline = match align.v {
        VAlign::Baseline => y as f32,
        VAlign::Centre => (y as f32 + face.cap_height * size / face.em / 2.0).round(),
    };
    for g in &mut glyphs {
        g.position.x += left;
        g.position.y = baseline;
    }
    glyphs
}

/// Rasterises laid-out glyphs, calling `plot(x, y, coverage)` for every
/// pixel a glyph touches.
pub(crate) fn rasterise(face: &Face, glyphs: Vec<Glyph>, mut plot: impl FnMut(i32, i32, f32)) {
    for glyph in glyphs {
        if let Some(outlined) = face.font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            let (ox, oy) = (bounds.min.x as i32, bounds.min.y as i32);
            outlined.draw(|gx, gy, coverage| {
                plot(ox + gx as i32, oy + gy as i32, coverage.clamp(0.0, 1.0));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(font: FontType, align: Align, text: &str) -> (f32, f32) {
        let glyphs = layout(face(font), 100, 50, 20, align, text);
        let first = glyphs.first().map_or(0.0, |g| g.position.x);
        (first, glyphs.first().map_or(0.0, |g| g.position.y))
    }

    #[test]
    fn alignment_moves_the_start_and_baseline() {
        let left = Align {
            h: HAlign::Left,
            v: VAlign::Baseline,
        };
        assert_eq!(span(FontType::Variable, left, "15"), (100.0, 50.0));
        let (x, baseline) = span(FontType::Variable, Align::CENTRE, "15");
        assert!(x < 100.0 && x > 80.0, "centred start {x}");
        // Inter's capitals are about 0.73 em: half of that below y.
        assert!(
            (56.0..=58.0).contains(&baseline),
            "centred baseline {baseline}"
        );
        let right = Align {
            h: HAlign::Right,
            v: VAlign::Baseline,
        };
        assert!(span(FontType::Fixed, right, "15").0 < x);
    }

    #[test]
    fn every_string_centred_on_one_line_shares_its_baseline() {
        // Baselines are whole pixels, so they compare exactly as integers.
        let baselines: Vec<i32> = ["15", "g", "-", "Ag"]
            .iter()
            .map(|t| span(FontType::Variable, Align::CENTRE, t).1 as i32)
            .collect();
        assert!(baselines.windows(2).all(|w| w[0] == w[1]), "{baselines:?}");
    }

    #[test]
    fn the_fonts_cover_puzzle_symbols() {
        for font in [FontType::Variable, FontType::Fixed] {
            assert!(face(font).covers("0123456789ABCabc?!+-×"));
        }
    }
}
