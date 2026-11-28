//! Immersive mode (`docs/porting-plan.md` §4.3 and the full-screen
//! decision): the toolbar and status bar are hidden, and the toolbar
//! comes back over the puzzle while the pointer is near the top edge or
//! over it, after Escape the game didn't use, or the immersive shortcut,
//! then hides again [`HIDE_AFTER`] later.

use std::time::{Duration, Instant};

/// How long the toolbar stays after the last reason to show it.
pub const HIDE_AFTER: Duration = Duration::from_millis(1500);

/// How close to the top edge, in points, the pointer brings the
/// toolbar back.
pub const EDGE: f32 = 24.0;

/// The title bar macOS slides over the top of a full-screen window when
/// the pointer reaches the top of the screen, in points (with a little
/// to spare). The pointer is over it, not the app, there.
pub const MAC_TITLE_BAR: f32 = 32.0;

/// Where the toolbar shows, and the band that brings it back, in points
/// from the window's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The toolbar's top.
    pub top: f32,
    /// The pointer above this brings the toolbar back.
    pub edge: f32,
}

impl Band {
    /// The band for a window; `mac_full_screen` puts both below the
    /// title bar macOS shows there.
    #[must_use]
    pub fn new(mac_full_screen: bool) -> Self {
        let top = if mac_full_screen { MAC_TITLE_BAR } else { 0.0 };
        Self {
            top,
            edge: top + EDGE,
        }
    }

    /// True if a pointer at height `y` (points from the window's top)
    /// brings the toolbar back.
    #[must_use]
    pub fn near_top(self, y: f32) -> bool {
        y <= self.edge
    }
}

/// Whether the hidden toolbar is showing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reveal {
    until: Option<Instant>,
}

impl Reveal {
    /// Shows the toolbar until [`HIDE_AFTER`] from `now` (and keeps it
    /// while called again, say each frame the pointer is over it).
    pub fn show(&mut self, now: Instant) {
        self.until = Some(now + HIDE_AFTER);
    }

    /// Hides it now.
    pub fn hide(&mut self) {
        self.until = None;
    }

    /// True while it shows.
    #[must_use]
    pub fn visible(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// How long until it hides, while it shows, so the UI repaints then.
    #[must_use]
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.until
            .filter(|&until| now < until)
            .map(|until| until - now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolbar_shows_for_a_while_then_hides() {
        let t0 = Instant::now();
        let mut reveal = Reveal::default();
        assert!(!reveal.visible(t0) && reveal.remaining(t0).is_none());
        reveal.show(t0);
        assert!(reveal.visible(t0 + HIDE_AFTER / 2));
        assert_eq!(reveal.remaining(t0 + HIDE_AFTER / 2), Some(HIDE_AFTER / 2));
        assert!(!reveal.visible(t0 + HIDE_AFTER));
        // Kept while the reason lasts.
        reveal.show(t0 + HIDE_AFTER);
        assert!(reveal.visible(t0 + HIDE_AFTER * 3 / 2));
        reveal.hide();
        assert!(!reveal.visible(t0 + HIDE_AFTER * 3 / 2));
    }

    #[test]
    fn only_the_top_edge_brings_it_back() {
        let band = Band::new(false);
        assert_eq!(band.top, 0.0);
        assert!(band.near_top(0.0) && band.near_top(EDGE));
        assert!(!band.near_top(EDGE + 1.0));
    }

    #[test]
    fn in_macos_full_screen_it_clears_the_system_title_bar() {
        let band = Band::new(true);
        assert_eq!(band.top, MAC_TITLE_BAR);
        assert!(band.near_top(MAC_TITLE_BAR + EDGE / 2.0), "just below it");
        assert!(!band.near_top(MAC_TITLE_BAR + EDGE + 1.0));
    }
}
