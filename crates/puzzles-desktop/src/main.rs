//! Desktop app for the puzzle collection (Windows, macOS, Linux).
//!
//! Built on `eframe`/`egui` with a `tiny-skia` puzzle canvas; see
//! `docs/porting-plan.md` (decision D6 and the desktop shell decision).
//! `app/` holds the logic, independent of egui; `ui/` draws it.

use eframe::egui;
mod app;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Puzzles")
            .with_inner_size([720.0, 640.0])
            .with_min_inner_size([320.0, 240.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Puzzles",
        options,
        Box::new(|cc| Ok(Box::new(ui::PuzzlesApp::new(cc)))),
    )
}
