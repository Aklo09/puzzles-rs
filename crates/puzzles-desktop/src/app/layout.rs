//! Placing the puzzle in its area (`docs/porting-plan.md` §4.1): the
//! canvas is sized in physical pixels, centred, and put on whole pixels
//! so it is never resampled.

/// The area available to the puzzle, in whole physical pixels, for an
/// area of `points` at `pixels_per_point`. `None` if it is empty.
#[must_use]
pub fn available_pixels(points: (f32, f32), pixels_per_point: f32) -> Option<(u32, u32)> {
    let px = |p: f32| (p * pixels_per_point).floor();
    let (w, h) = (px(points.0), px(points.1));
    (w >= 1.0 && h >= 1.0).then_some((w as u32, h as u32))
}

/// The top-left pixel of a `size` canvas centred in an `area`, both in
/// physical pixels, relative to the area's corner. A canvas larger than
/// the area starts at its corner.
#[must_use]
pub fn centred(area: (u32, u32), size: (u32, u32)) -> (u32, u32) {
    (
        area.0.saturating_sub(size.0) / 2,
        area.1.saturating_sub(size.1) / 2,
    )
}

/// The puzzle pixel under a pointer at `offset` points from the canvas's
/// top-left corner.
#[must_use]
pub fn to_canvas(offset: (f32, f32), pixels_per_point: f32) -> (i32, i32) {
    let px = |p: f32| (p * pixels_per_point).floor() as i32;
    (px(offset.0), px(offset.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_round_down_to_whole_pixels() {
        assert_eq!(available_pixels((100.4, 50.9), 1.0), Some((100, 50)));
        assert_eq!(available_pixels((100.4, 50.9), 2.0), Some((200, 101)));
        assert_eq!(available_pixels((0.4, 50.0), 2.0), None);
        assert_eq!(available_pixels((-3.0, 50.0), 1.0), None);
    }

    #[test]
    fn canvases_are_centred_on_whole_pixels() {
        assert_eq!(centred((300, 200), (100, 100)), (100, 50));
        assert_eq!(centred((301, 200), (100, 101)), (100, 49));
        assert_eq!(centred((50, 50), (100, 40)), (0, 5));
    }

    #[test]
    fn pointers_map_to_puzzle_pixels() {
        assert_eq!(to_canvas((10.0, 20.6), 1.0), (10, 20));
        assert_eq!(to_canvas((10.25, 20.75), 2.0), (20, 41));
        assert_eq!(to_canvas((-0.5, 3.0), 1.0), (-1, 3));
    }
}
