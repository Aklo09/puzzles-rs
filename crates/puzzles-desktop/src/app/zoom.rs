//! Zooming the puzzle (`docs/porting-plan.md` §4.1 and the full-screen
//! decision): a factor over "fit to window", from 1× to [`MAX`]. A
//! zoomed puzzle is laid out again in a larger area, so the game redraws
//! it at a larger size and it stays crisp; the area never asks for more
//! than [`MAX_PIXELS`], nor a side over the GPU's largest texture (which
//! the front end passes).

/// The largest zoom, over fit to window.
pub const MAX: f32 = 4.0;

/// One zoom step (keyboard, menu).
pub const STEP: f32 = 1.25;

/// The largest area zoom asks for, in pixels: 128 MiB as the canvas,
/// and again as the texture. About 2.5× a maximised window on a Retina
/// laptop; the full [`MAX`] in smaller windows. (A cap per side instead,
/// 4096 pixels, allowed only 137% there: the play test.)
pub const MAX_PIXELS: u64 = 32 << 20;

/// How far the puzzle is zoomed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    factor: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Self { factor: 1.0 }
    }
}

impl Zoom {
    /// The zoom asked for (the one applied may be capped; see
    /// [`applied`](Self::applied)).
    #[must_use]
    pub fn factor(self) -> f32 {
        self.factor
    }

    /// Multiplies the zoom by `by`, within 1× and [`MAX`]. Returns
    /// whether it changed.
    pub fn scale(&mut self, by: f32) -> bool {
        let old = self.factor;
        let mut factor = (self.factor * by).clamp(1.0, MAX);
        // Steps there and back land on fit exactly.
        if (factor - 1.0).abs() < 0.01 {
            factor = 1.0;
        }
        self.factor = factor;
        (factor - old).abs() > f32::EPSILON
    }

    /// One step closer. Returns whether it changed.
    pub fn zoom_in(&mut self) -> bool {
        self.scale(STEP)
    }

    /// One step further. Returns whether it changed.
    pub fn zoom_out(&mut self) -> bool {
        self.scale(STEP.recip())
    }

    /// Back to fit to window. Returns whether it changed.
    pub fn fit(&mut self) -> bool {
        std::mem::replace(&mut self.factor, 1.0) > 1.0
    }

    /// The largest zoom in a `viewport` (pixels) within [`MAX_PIXELS`]
    /// and canvas sides of at most `max_side` pixels: never below fit,
    /// even if the viewport itself is larger than the caps.
    #[must_use]
    pub fn ceiling(viewport: (u32, u32), max_side: u32) -> f32 {
        let (w, h) = (viewport.0.max(1) as f32, viewport.1.max(1) as f32);
        let by_side = max_side as f32 / w.max(h);
        let by_area = (MAX_PIXELS as f32 / (w * h)).sqrt();
        MAX.min(by_side).min(by_area).max(1.0)
    }

    /// The zoom applied in a `viewport`, capped as in
    /// [`ceiling`](Self::ceiling).
    #[must_use]
    pub fn applied(self, viewport: (u32, u32), max_side: u32) -> f32 {
        self.factor.min(Self::ceiling(viewport, max_side))
    }

    /// The area to lay the puzzle out in, in pixels: the viewport scaled
    /// by the applied zoom.
    #[must_use]
    pub fn area(self, viewport: (u32, u32), max_side: u32) -> (u32, u32) {
        let f = self.applied(viewport, max_side);
        let scale = |n: u32| ((n as f32 * f).floor() as u32).max(n);
        (scale(viewport.0), scale(viewport.1))
    }

    /// The zoom as the toolbar shows it, such as "125%".
    #[must_use]
    pub fn percent(applied: f32) -> String {
        format!("{:.0}%", applied * 100.0)
    }
}

/// Where a `size` canvas starts in a `view` (one axis, any unit): centred
/// when smaller, at 0 when larger (then it scrolls).
#[must_use]
pub fn canvas_start(view: f32, size: f32) -> f32 {
    ((view - size) / 2.0).max(0.0)
}

/// The scroll offset (one axis) that keeps the point of the puzzle under
/// `pointer` (from the viewport's start) in place when the canvas goes
/// from `old` to `new` size in a `view`, scrolled by `offset` before.
/// Content smaller than the view doesn't scroll.
#[must_use]
pub fn anchored_offset(view: f32, old: f32, new: f32, offset: f32, pointer: f32) -> f32 {
    if old <= 0.0 {
        return 0.0;
    }
    let fraction = (offset + pointer - canvas_start(view, old)) / old;
    let target = canvas_start(view, new) + fraction * new - pointer;
    target.clamp(0.0, (new - view).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_between_fit_and_the_maximum() {
        let mut z = Zoom::default();
        assert_eq!(z.factor(), 1.0);
        assert!(!z.zoom_out(), "never below fit");
        assert!(z.zoom_in());
        assert!((z.factor() - 1.25).abs() < 1e-6);
        assert!(z.zoom_out());
        assert_eq!(z.factor(), 1.0, "and back exactly");
        while z.zoom_in() {}
        assert_eq!(z.factor(), MAX);
        assert!(z.fit());
        assert_eq!(z.factor(), 1.0);
        assert!(!z.fit());
        assert!(z.scale(1.1) && z.scale(0.5));
        assert_eq!(z.factor(), 1.0);
    }

    #[test]
    fn the_area_grows_by_the_zoom_up_to_the_caps() {
        const GPU: u32 = 8192;
        let mut z = Zoom::default();
        assert_eq!(z.area((800, 600), GPU), (800, 600));
        z.scale(2.0);
        assert_eq!(z.area((800, 600), GPU), (1600, 1200));
        z.scale(2.0);
        assert_eq!(Zoom::ceiling((800, 600), GPU), MAX);
        assert_eq!(z.area((800, 600), GPU), (3200, 2400));
        // The GPU's side limit.
        assert_eq!(z.applied((800, 600), 2400), 3.0);
        assert_eq!(z.area((800, 600), 2400), (2400, 1800));
        // The area budget: a maximised window on a Retina laptop.
        let retina = (3024, 1700);
        let top = Zoom::ceiling(retina, GPU);
        assert!((2.5..2.6).contains(&top), "{top}");
        let (w, h) = z.area(retina, GPU);
        assert!(u64::from(w) * u64::from(h) <= MAX_PIXELS);
        // Fit beats both caps.
        assert_eq!(Zoom::ceiling((9000, 6000), GPU), 1.0);
        assert_eq!(z.area((9000, 6000), GPU), (9000, 6000));
        assert_eq!(Zoom::percent(1.25), "125%");
    }

    #[test]
    fn small_canvases_are_centred_large_ones_scroll() {
        assert_eq!(canvas_start(100.0, 60.0), 20.0);
        assert_eq!(canvas_start(100.0, 160.0), 0.0);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        // Fit: 80 of 100 centred (starts at 10); pointer at 30 is 1/4 in.
        // Doubled to 160 it scrolls so that 1/4 in (40) is under the
        // pointer again: offset 10.
        assert_eq!(anchored_offset(100.0, 80.0, 160.0, 0.0, 30.0), 10.0);
        // From 160 scrolled by 10 to 320, same point: 80 - 30 = 50.
        assert_eq!(anchored_offset(100.0, 160.0, 320.0, 10.0, 30.0), 50.0);
        // Never past either end.
        assert_eq!(anchored_offset(100.0, 160.0, 320.0, 60.0, 100.0), 220.0);
        assert_eq!(anchored_offset(100.0, 160.0, 320.0, 0.0, 0.0), 0.0);
        assert_eq!(anchored_offset(100.0, 160.0, 80.0, 30.0, 50.0), 0.0);
    }
}
