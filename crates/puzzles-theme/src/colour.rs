//! Colours as C keeps them, and C's colour arithmetic (`misc.c`,
//! upstream 616da16), ported in `f32` with C's operation order so the
//! results match bit for bit.

use std::fmt;

/// A colour: red, green and blue from 0 to 1, in `f32` as C's palettes.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rgb {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
}

impl Rgb {
    /// Black.
    pub const BLACK: Self = Self::grey(0.0);
    /// White.
    pub const WHITE: Self = Self::grey(1.0);

    /// A colour from its components.
    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    /// A grey with all three components `v`.
    #[must_use]
    pub const fn grey(v: f32) -> Self {
        Self::new(v, v, v)
    }

    /// The components as an array.
    #[must_use]
    pub const fn to_array(self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }

    /// Each component through `f`.
    #[must_use]
    pub fn map(self, f: impl Fn(f32) -> f32) -> Self {
        Self::new(f(self.r), f(self.g), f(self.b))
    }

    /// The largest difference between corresponding components.
    #[must_use]
    pub fn max_difference(self, other: Self) -> f32 {
        (self.r - other.r)
            .abs()
            .max((self.g - other.g).abs())
            .max((self.b - other.b).abs())
    }

    /// The colour as 8-bit sRGB, rounding to nearest.
    #[must_use]
    pub fn to_rgb8(self) -> [u8; 3] {
        self.to_array()
            .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}, {:?}, {:?}]", self.r, self.g, self.b)
    }
}

/// C's `colour_distance`: Euclidean, in `f32` (C's `<tgmath.h>` makes
/// it `sqrtf`).
fn distance(a: Rgb, b: Rgb) -> f32 {
    let (dr, dg, db) = (a.r - b.r, a.g - b.g, a.b - b.b);
    (dr * dr + dg * dg + db * db).sqrt()
}

/// C's `colour_mix`: `a * (1 - p) + b * p` per component.
#[must_use]
pub fn mix(a: Rgb, b: Rgb, p: f32) -> Rgb {
    let m = |x: f32, y: f32| x * (1.0 - p) + y * p;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// What C's `game_mkhighlight_specific` makes of a base colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bevel {
    /// The base itself, moved away from white or black if it was too
    /// close for a visible bevel (C rewrites the game's base entry).
    pub base: Rgb,
    /// The light side.
    pub highlight: Rgb,
    /// The dark side.
    pub lowlight: Rgb,
}

/// C's `game_mkhighlight_specific` with both a highlight and a lowlight:
/// colours a distance K = √3/6 from the base towards white and black.
/// If one would be beyond white or black, it is white or black and the
/// base moves to keep the distance (and C then recomputes the lowlight
/// with the original distance to black, which is kept here too).
#[doc(alias("game_mkhighlight", "game_mkhighlight_specific"))]
#[must_use]
pub fn bevel(base: Rgb) -> Bevel {
    let k = (3.0_f64.sqrt() / 6.0) as f32;
    let k_over_root3 = (f64::from(k) / 3.0_f64.sqrt()) as f32;
    let mut base = base;
    let lowlight;
    let highlight;

    let db = distance(base, Rgb::BLACK);
    if db < k {
        lowlight = Rgb::BLACK;
        base = if db == 0.0 {
            mix(Rgb::BLACK, Rgb::WHITE, k_over_root3)
        } else {
            mix(Rgb::BLACK, base, k / db)
        };
    } else {
        lowlight = mix(base, Rgb::BLACK, k / db);
    }

    let dw = distance(base, Rgb::WHITE);
    let mut lowlight = lowlight;
    if dw < k {
        highlight = Rgb::WHITE;
        base = if dw == 0.0 {
            mix(Rgb::WHITE, Rgb::BLACK, k_over_root3)
        } else {
            mix(Rgb::WHITE, base, k / dw)
        };
        lowlight = mix(base, Rgb::BLACK, k / db);
    } else {
        highlight = mix(base, Rgb::WHITE, k / dw);
    }

    Bevel {
        base,
        highlight,
        lowlight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_grey_saturates_to_white_and_darkens() {
        let b = bevel(Rgb::grey(0.9));
        assert_eq!(b.highlight, Rgb::WHITE);
        assert!(b.base.r < 0.84 && b.base.r > 0.83, "{}", b.base);
        assert!(b.lowlight.r < b.base.r);
    }

    #[test]
    fn a_mid_grey_keeps_its_base() {
        let b = bevel(Rgb::grey(0.5));
        assert_eq!(b.base, Rgb::grey(0.5));
        assert!(b.highlight.r > 0.5 && b.lowlight.r < 0.5);
    }

    #[test]
    fn black_and_white_bases_move() {
        assert_eq!(bevel(Rgb::BLACK).lowlight, Rgb::BLACK);
        assert!(bevel(Rgb::BLACK).base.r > 0.0);
        assert_eq!(bevel(Rgb::WHITE).highlight, Rgb::WHITE);
        assert!(bevel(Rgb::WHITE).base.r < 1.0);
    }

    #[test]
    fn eight_bit_rounds() {
        assert_eq!(Rgb::grey(0.9).to_rgb8(), [230, 230, 230]);
        assert_eq!(Rgb::new(1.0, 0.0, 2.0).to_rgb8(), [255, 0, 255]);
    }
}
