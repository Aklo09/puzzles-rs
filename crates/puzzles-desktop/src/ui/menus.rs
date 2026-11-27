//! The toolbar's Type and Game menus (dialogs and presets decision in
//! `docs/porting-plan.md`): presets with the user's own and "Custom…";
//! game IDs, random seeds, copying them, and preferences.

use eframe::egui;
use egui::RichText;
use egui_phosphor::regular as icon;
use puzzles_core::midend::ConfigKind;
use puzzles_core::preset::{PresetEntry, PresetMenu};

use super::fonts;
use crate::app::command::Command;
use crate::app::session::Session;

/// What a menu asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// A new game with this preset.
    Preset(usize),
    /// Open a dialog.
    Dialog(ConfigKind),
    /// Copy the descriptive game ID.
    CopyGameId,
    /// Copy the random seed ID.
    CopySeed,
    /// Open Settings (with the game's preferences).
    Settings,
    /// Run an app command (Save, Load).
    Command(Command),
}

/// A toolbar menu button: an icon and a label.
fn menu_title(glyph: &str, label: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    RichText::new(glyph)
        .size(18.0)
        .family(fonts::icon_family())
        .append_to(
            &mut job,
            &egui::Style::default(),
            egui::FontSelection::Default,
            egui::Align::Center,
        );
    RichText::new(format!(" {label}")).append_to(
        &mut job,
        &egui::Style::default(),
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    job
}

fn presets(
    ui: &mut egui::Ui,
    menu: &PresetMenu<usize>,
    current: Option<usize>,
    action: &mut Option<MenuAction>,
) {
    for entry in &menu.entries {
        match entry {
            PresetEntry::Preset { title, params: id } => {
                if ui
                    .add(egui::Button::selectable(
                        current == Some(*id),
                        title.as_str(),
                    ))
                    .clicked()
                {
                    *action = Some(MenuAction::Preset(*id));
                    ui.close();
                }
            }
            PresetEntry::Submenu { title, menu } => {
                ui.menu_button(title.as_str(), |ui| presets(ui, menu, current, action));
            }
        }
    }
}

/// The Type menu. `opened` runs when it opens, to re-read the presets
/// file.
pub fn type_menu(
    ui: &mut egui::Ui,
    session: &mut Session,
    opened: impl FnOnce(&mut Session),
) -> Option<MenuAction> {
    let mut action = None;
    let response = ui.menu_button(menu_title(icon::SLIDERS_HORIZONTAL, "Type"), |ui| {
        ui.set_min_width(180.0);
        let current = session.current_preset();
        let can_configure = session.capabilities().can_configure;
        presets(ui, &session.presets().clone(), current, &mut action);
        if can_configure {
            ui.separator();
            let custom = current.is_none();
            if ui
                .add(egui::Button::selectable(custom, "Custom…"))
                .clicked()
            {
                action = Some(MenuAction::Dialog(ConfigKind::Settings));
                ui.close();
            }
        }
        if !session.preset_errors().is_empty() {
            ui.separator();
            for error in session.preset_errors() {
                ui.label(
                    RichText::new(error)
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
        }
    });
    if response.response.clicked() {
        opened(session);
    }
    action
}

/// The Game menu. `mac` picks how shortcuts are written.
pub fn game_menu(ui: &mut egui::Ui, session: &Session, mac: bool) -> Option<MenuAction> {
    let mut action = None;
    ui.menu_button(menu_title(icon::HASH, "Game"), |ui| {
        ui.set_min_width(180.0);
        for command in [Command::Save, Command::Load] {
            let button = egui::Button::new(command.label())
                .shortcut_text(command.shortcut_text(mac).unwrap_or_default());
            if ui.add_enabled(session.can(command), button).clicked() {
                action = Some(MenuAction::Command(command));
                ui.close();
            }
        }
        ui.separator();
        let mut item = |ui: &mut egui::Ui, enabled: bool, label: &str, what: MenuAction| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                action = Some(what);
                ui.close();
            }
        };
        let playing = !session.is_generating();
        item(
            ui,
            playing,
            "Enter game ID…",
            MenuAction::Dialog(ConfigKind::Desc),
        );
        item(
            ui,
            playing,
            "Enter random seed…",
            MenuAction::Dialog(ConfigKind::Seed),
        );
        ui.separator();
        item(
            ui,
            session.game_id().is_some(),
            "Copy game ID",
            MenuAction::CopyGameId,
        );
        item(
            ui,
            session.random_seed().is_some(),
            "Copy random seed",
            MenuAction::CopySeed,
        );
        let has_prefs = session
            .config(ConfigKind::Prefs)
            .is_some_and(|c| !c.items.is_empty());
        if has_prefs {
            ui.separator();
            item(
                ui,
                true,
                "Preferences…",
                MenuAction::Dialog(ConfigKind::Prefs),
            );
        }
    });
    action
}
