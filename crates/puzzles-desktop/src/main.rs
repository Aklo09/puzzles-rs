//! Desktop app for the puzzle collection (Windows, macOS, Linux).
//!
//! Built on `eframe`/`egui` with a `tiny-skia` puzzle canvas; see
//! `docs/porting-plan.md` (decision D6 and the desktop shell decision).
//! `app/` holds the logic, independent of egui; `ui/` draws it.

use std::path::PathBuf;

use eframe::egui;
mod app;
mod ui;

fn main() -> eframe::Result {
    let dirs = app::paths::AppDirs::detect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Puzzles")
            .with_inner_size([720.0, 640.0])
            .with_min_inner_size([ui::MIN_WINDOW_WIDTH, 240.0]),
        // The window's place, kept by eframe (persistence decision).
        persistence_path: dirs.as_ref().map(app::paths::AppDirs::app_state),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Puzzles",
        options,
        Box::new(|cc| Ok(Box::new(ui::PuzzlesApp::new(cc, dirs, launch_file())))),
    )
}

/// A saved game to open, named on the command line (`puzzles game.sav`,
/// as file managers start an app to open a file). Arguments starting
/// with `-` are skipped: older macOS passes `-psn_…` to apps opened from
/// the Finder.
fn launch_file() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .find(|a| !a.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
}
