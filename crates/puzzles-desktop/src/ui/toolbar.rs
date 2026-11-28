//! The toolbar over a game: the game list, the Type and Game menus and
//! Settings, New and Restart, Undo, Redo and Solve, then zoom, full
//! screen and immersive mode. Drawn as a top panel, or over the puzzle
//! in immersive mode.

use eframe::egui;
use egui::RichText;
use egui_phosphor::regular as icon;

use super::fonts;
use super::menus::{self, MenuAction};
use crate::app::command::Command;
use crate::app::session::Session;
use crate::app::user_presets::PresetsFile;

/// What the toolbar asked for.
pub enum ToolbarAction {
    Command(Command),
    Menu(MenuAction),
}

/// One place on the toolbar.
#[derive(Clone, Copy)]
enum Tool {
    Command(Command),
    Separator,
    /// The Type and Game menus and Settings: what to play.
    PlayMenus,
    /// The zoom menu.
    Zoom,
}

const TOOLS: [Tool; 14] = [
    Tool::Command(Command::Games),
    Tool::Separator,
    Tool::PlayMenus,
    Tool::Separator,
    Tool::Command(Command::NewGame),
    Tool::Command(Command::Restart),
    Tool::Separator,
    Tool::Command(Command::Undo),
    Tool::Command(Command::Redo),
    Tool::Command(Command::Solve),
    Tool::Separator,
    Tool::Zoom,
    Tool::Command(Command::FullScreen),
    Tool::Command(Command::Immersive),
];

/// What the toolbar shows besides the game.
pub struct ToolbarState {
    /// How shortcuts are written.
    pub mac: bool,
    /// The zoom applied, over fit to window, and whether it can grow.
    pub zoom: (f32, bool),
    /// True in full screen.
    pub full_screen: bool,
    /// True in immersive mode.
    pub immersive: bool,
}

/// An icon button. The icon is a private-use character, so the button
/// is named `label` for screen readers (and UI tests).
pub fn icon_button(
    ui: &mut egui::Ui,
    glyph: &str,
    label: &str,
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    let glyph = RichText::new(glyph).size(20.0).family(fonts::icon_family());
    let response = ui
        .add_enabled(enabled, egui::Button::new(glyph).frame(false))
        .on_hover_text(tooltip)
        .on_disabled_hover_text(tooltip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response
}

/// The gear that opens Settings.
pub fn settings_button(ui: &mut egui::Ui) -> egui::Response {
    icon_button(ui, icon::GEAR_SIX, "Settings", "Settings", true)
}

/// The toolbar's buttons and menus, in a row.
pub fn contents(
    ui: &mut egui::Ui,
    session: &mut Session,
    presets_file: &mut Option<PresetsFile>,
    state: &ToolbarState,
) -> Option<ToolbarAction> {
    let mac = state.mac;
    let mut clicked = None;
    ui.horizontal(|ui| {
        for tool in TOOLS {
            match tool {
                Tool::Separator => {
                    ui.separator();
                }
                Tool::PlayMenus => {
                    let opened = |session: &mut Session| {
                        if let Some(file) = presets_file.as_mut()
                            && file.refresh()
                        {
                            session.use_presets_file(file);
                        }
                    };
                    if let Some(a) = menus::type_menu(ui, session, opened) {
                        clicked = Some(ToolbarAction::Menu(a));
                    }
                    if let Some(a) = menus::game_menu(ui, session, mac) {
                        clicked = Some(ToolbarAction::Menu(a));
                    }
                    if settings_button(ui).clicked() {
                        clicked = Some(ToolbarAction::Menu(MenuAction::Settings));
                    }
                }
                Tool::Zoom => {
                    if let Some(c) = menus::zoom_menu(ui, state.zoom.0, state.zoom.1, mac) {
                        clicked = Some(ToolbarAction::Command(c));
                    }
                }
                Tool::Command(command) => {
                    let glyph = match command {
                        Command::Games => icon::SQUARES_FOUR,
                        Command::NewGame => icon::DICE_FIVE,
                        Command::Restart => icon::ARROW_COUNTER_CLOCKWISE,
                        Command::Undo => icon::ARROW_U_UP_LEFT,
                        Command::Redo => icon::ARROW_U_UP_RIGHT,
                        Command::Solve => icon::LIGHTBULB,
                        Command::FullScreen if state.full_screen => icon::CORNERS_IN,
                        Command::FullScreen => icon::CORNERS_OUT,
                        Command::Immersive if state.immersive => icon::ARROWS_IN,
                        Command::Immersive => icon::ARROWS_OUT,
                        _ => continue,
                    };
                    let enabled = session.can(command);
                    let tooltip = command.tooltip(mac);
                    if icon_button(ui, glyph, command.label(), &tooltip, enabled).clicked() {
                        clicked = Some(ToolbarAction::Command(command));
                    }
                }
            }
        }
    });
    clicked
}
