//! The app's fonts: Inter and `JetBrains` Mono, the families the puzzles
//! use (D6), then egui's own as fallbacks, and the Phosphor icons.

use eframe::egui;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

const INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");
const MONO: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMonoNL-Regular.ttf");

/// Installs the fonts.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, data, family) in [
        ("inter", INTER, FontFamily::Proportional),
        ("jetbrains-mono", MONO, FontFamily::Monospace),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(data)));
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, name.to_owned());
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // egui-phosphor's own family puts the text font first, and Inter has
    // glyphs at some of the private-use code points the icons use; this
    // family is Phosphor alone.
    fonts
        .families
        .insert(icon_family(), vec!["phosphor".to_owned()]);
    ctx.set_fonts(fonts);
}

/// The family to draw Phosphor icons in.
pub fn icon_family() -> FontFamily {
    FontFamily::Name("icons".into())
}
