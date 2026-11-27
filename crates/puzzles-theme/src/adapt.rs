//! Adapting a light theme's colour to a dark theme (themes decision in
//! `docs/porting-plan.md`): its lightness is mirrored around the
//! background in `OKLab`, keeping hue and chroma, so a colour far darker
//! than the light background becomes far lighter than the dark one.
//!
//! Lightness maps piecewise linearly through (0 → 1), (light background
//! → dark background) and (1 → 0); the result is clamped to sRGB.

use palette::{FromColor, Oklab, Srgb};

use crate::colour::Rgb;

/// `OKLab` lightness of a colour, from 0 (black) to 1 (white).
#[must_use]
pub fn lightness(c: Rgb) -> f32 {
    Oklab::from_color(Srgb::new(c.r, c.g, c.b)).l
}

/// The mapping between a light and a dark background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mirror {
    light: f32,
    dark: f32,
}

impl Mirror {
    /// Mirrors around `light_background` onto `dark_background`.
    #[must_use]
    pub fn new(light_background: Rgb, dark_background: Rgb) -> Self {
        // Keep both strictly inside (0, 1) so the two segments exist.
        let clampl = |l: f32| l.clamp(0.001, 0.999);
        Self {
            light: clampl(lightness(light_background)),
            dark: clampl(lightness(dark_background)),
        }
    }

    fn map_lightness(self, l: f32) -> f32 {
        let l = l.clamp(0.0, 1.0);
        if l <= self.light {
            1.0 + (self.dark - 1.0) * (l / self.light)
        } else {
            self.dark * (1.0 - (l - self.light) / (1.0 - self.light))
        }
    }

    /// The colour adapted to the dark theme.
    #[must_use]
    pub fn apply(self, c: Rgb) -> Rgb {
        let mut lab = Oklab::from_color(Srgb::new(c.r, c.g, c.b));
        lab.l = self.map_lightness(lab.l);
        let out = Srgb::from_color(lab);
        Rgb::new(out.red, out.green, out.blue).map(|v| v.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIGHT_BG: Rgb = Rgb::grey(0.9);
    const DARK_BG: Rgb = Rgb::grey(0.12);

    fn close(a: Rgb, b: Rgb) -> bool {
        a.max_difference(b) < 0.01
    }

    #[test]
    fn the_background_black_and_white_swap_ends() {
        let m = Mirror::new(LIGHT_BG, DARK_BG);
        assert!(close(m.apply(LIGHT_BG), DARK_BG));
        assert!(close(m.apply(Rgb::BLACK), Rgb::WHITE));
        assert!(close(m.apply(Rgb::WHITE), Rgb::BLACK));
    }

    #[test]
    fn order_by_lightness_reverses_and_hue_stays() {
        let m = Mirror::new(LIGHT_BG, DARK_BG);
        let greys = [0.0, 0.3, 0.6, 0.9, 1.0]
            .map(Rgb::grey)
            .map(|c| lightness(m.apply(c)));
        assert!(greys.windows(2).all(|w| w[0] > w[1]), "{greys:?}");
        // A dark red stays red, and becomes lighter than the dark
        // background.
        let red = m.apply(Rgb::new(0.6, 0.0, 0.0));
        assert!(red.r > red.g && red.r > red.b, "{red:?}");
        assert!(lightness(red) > lightness(DARK_BG));
    }

    #[test]
    fn flips_wrong_tiles_stay_visible() {
        // Flip's "wrong" tile is the light background × 1/3: in a dark
        // theme it must stand well clear of the background.
        let m = Mirror::new(LIGHT_BG, DARK_BG);
        let wrong = m.apply(LIGHT_BG.map(|c| c / 3.0));
        assert!(lightness(wrong) - lightness(DARK_BG) > 0.3, "{wrong:?}");
    }
}
