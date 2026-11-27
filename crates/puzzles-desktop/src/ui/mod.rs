//! The egui side of the app: the game library, the toolbar with its
//! Type and Game menus, the puzzle view, the status bar, configuration
//! dialogs and the "Generating…" overlay (desktop shell, and dialogs and
//! presets decisions in `docs/porting-plan.md`).

use eframe::egui;
mod dialogs;
mod fonts;
mod input;
mod menus;
mod puzzle_view;
mod shortcuts;

use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Frame, Margin, RichText};
use egui_phosphor::regular as icon;
use puzzles_games::{GAMES, GameInfo};
use puzzles_theme::ThemeSet;

use crate::app::command::Command;
use crate::app::paths::AppDirs;
use crate::app::session::{Outcome, Session};
use crate::app::user_presets::PresetsFile;
use dialogs::{Dialog, DialogAction};
use menus::MenuAction;
use puzzle_view::PuzzleView;

/// The theme until themes can be chosen (Phase 3, `ui/themes`).
const THEME: &str = "classic";

/// What the window shows.
enum Screen {
    Library,
    Game {
        session: Box<Session>,
        view: PuzzleView,
    },
}

/// The app.
pub struct PuzzlesApp {
    screen: Screen,
    themes: ThemeSet,
    mac: bool,
    title: String,
    error: Option<String>,
    dirs: Option<AppDirs>,
    presets_file: Option<PresetsFile>,
    dialog: Option<Dialog>,
}

/// What the toolbar asked for.
enum ToolbarAction {
    Command(Command),
    Menu(MenuAction),
}

impl PuzzlesApp {
    /// Sets up fonts and style, with the user's files in the system's
    /// folder for them.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_dirs(cc, AppDirs::detect())
    }

    /// [`new`](Self::new), keeping the user's files in `dirs` (or
    /// nowhere).
    pub fn with_dirs(cc: &eframe::CreationContext<'_>, dirs: Option<AppDirs>) -> Self {
        fonts::install(&cc.egui_ctx);
        // Classic is a light theme; themed app colours come with themes.
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let presets_file = dirs.as_ref().map(|d| PresetsFile::new(d.presets()));
        Self {
            screen: Screen::Library,
            themes: ThemeSet::builtin(),
            mac: cfg!(target_os = "macos"),
            title: String::new(),
            error: None,
            dirs,
            presets_file,
            dialog: None,
        }
    }

    fn open(&mut self, ctx: &egui::Context, info: &'static GameInfo) {
        let repaint = ctx.clone();
        let waker = Arc::new(move || repaint.request_repaint());
        match Session::new(info, &self.themes, THEME, waker, self.dirs.as_ref()) {
            Ok(mut session) => {
                if let Some(file) = &mut self.presets_file {
                    file.refresh();
                    session.use_presets_file(file);
                }
                self.dialog = None;
                self.error = None;
                self.screen = Screen::Game {
                    session: Box::new(session),
                    view: PuzzleView::default(),
                };
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn menu_action(&mut self, ctx: &egui::Context, action: MenuAction) {
        let Screen::Game { session, .. } = &mut self.screen else {
            return;
        };
        match action {
            MenuAction::Preset(id) => session.choose_preset(id),
            MenuAction::Dialog(kind) => {
                self.dialog = session.config(kind).map(|c| Dialog::new(kind, c));
            }
            MenuAction::CopyGameId => {
                if let Some(id) = session.game_id() {
                    ctx.copy_text(id);
                    session.notify("Game ID copied.");
                }
            }
            MenuAction::CopySeed => {
                if let Some(seed) = session.random_seed() {
                    ctx.copy_text(seed);
                    session.notify("Random seed copied.");
                }
            }
        }
    }

    /// Draws the open dialog and applies it on OK.
    fn dialog(&mut self, ctx: &egui::Context) {
        let (Some(dialog), Screen::Game { session, .. }) = (&mut self.dialog, &mut self.screen)
        else {
            self.dialog = None;
            return;
        };
        match dialog.show(ctx) {
            DialogAction::Open => {}
            DialogAction::Close => self.dialog = None,
            DialogAction::Apply(kind, items) => match session.apply_config(kind, &items) {
                Ok(()) => self.dialog = None,
                Err(e) => dialog.set_error(e),
            },
        }
    }

    fn run(&mut self, ctx: &egui::Context, command: Command) {
        match (command, &mut self.screen) {
            (Command::Games, _) => {
                self.dialog = None;
                self.screen = Screen::Library;
            }
            (Command::Quit, _) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            (_, Screen::Game { session, .. }) => {
                if session.command(command) == Outcome::Quit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            (_, Screen::Library) => {}
        }
    }

    fn set_title(&mut self, ctx: &egui::Context) {
        let title = match &self.screen {
            Screen::Library => "Puzzles".to_owned(),
            Screen::Game { session, .. } => session.title(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn library(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(520.0);
                ui.add_space(24.0);
                ui.heading(RichText::new("Puzzles").size(28.0));
                ui.add_space(16.0);
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for info in GAMES {
                        if Self::library_row(ui, info).clicked() {
                            chosen = Some(info);
                        }
                        ui.add_space(6.0);
                    }
                });
            });
        });
        if let Some(info) = chosen {
            self.open(ui.ctx(), info);
        }
    }

    /// One game in the library: its name, and what kind of puzzle it is
    /// underneath; the objective as a tooltip.
    fn library_row(ui: &mut egui::Ui, info: &GameInfo) -> egui::Response {
        let row = ui
            .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                // Selectable labels would take the clicks meant for the row.
                ui.style_mut().interaction.selectable_labels = false;
                let fill = ui.style().interact(&ui.response()).weak_bg_fill;
                Frame::new()
                    .fill(fill)
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.label(RichText::new(info.name).size(16.0).strong());
                            ui.label(RichText::new(info.description).weak());
                        });
                    });
            })
            .response;
        row.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, info.name));
        row.on_hover_text(info.objective)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) -> Option<ToolbarAction> {
        let Screen::Game { session, .. } = &mut self.screen else {
            return None;
        };
        let presets_file = &mut self.presets_file;
        let mac = self.mac;
        let mut clicked = None;
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, entry) in Command::TOOLBAR.into_iter().enumerate() {
                    let Some(command) = entry else {
                        ui.separator();
                        if i == 1 {
                            // After Games: what to play.
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
                            if let Some(a) = menus::game_menu(ui, session) {
                                clicked = Some(ToolbarAction::Menu(a));
                            }
                            ui.separator();
                        }
                        continue;
                    };
                    let glyph = match command {
                        Command::Games => icon::SQUARES_FOUR,
                        Command::NewGame => icon::DICE_FIVE,
                        Command::Restart => icon::ARROW_COUNTER_CLOCKWISE,
                        Command::Undo => icon::ARROW_U_UP_LEFT,
                        Command::Redo => icon::ARROW_U_UP_RIGHT,
                        Command::Solve => icon::LIGHTBULB,
                        Command::Quit => continue,
                    };
                    let enabled = session.can(command);
                    let glyph = RichText::new(glyph).size(20.0).family(fonts::icon_family());
                    let button = egui::Button::new(glyph).frame(false);
                    let response = ui
                        .add_enabled(enabled, button)
                        .on_hover_text(command.tooltip(mac))
                        .on_disabled_hover_text(command.tooltip(mac));
                    // The icon is a private-use character: name the button
                    // for screen readers (and UI tests).
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            enabled,
                            command.label(),
                        )
                    });
                    if response.clicked() {
                        clicked = Some(ToolbarAction::Command(command));
                    }
                }
            });
        });
        clicked
    }

    fn game(&mut self, ui: &mut egui::Ui) -> Outcome {
        let mac = self.mac;
        let input = self.dialog.is_none();
        let Screen::Game { session, view } = &mut self.screen else {
            return Outcome::Continue;
        };
        let status = session.status_line();
        let wants_bar = session.capabilities().wants_statusbar || session.capabilities().is_timed;
        if wants_bar || !status.is_empty() {
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.label(if status.is_empty() {
                    " ".to_owned()
                } else {
                    status
                });
            });
        }
        let background = puzzle_view::colour(session.background());
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(background).inner_margin(Margin::same(8)))
            .show(ui, |ui| view.show(ui, session, mac, input))
            .inner
    }

    /// The overlay while a New Game generates; returns true on Cancel.
    fn generating(ui: &egui::Ui) -> bool {
        let mut cancel = false;
        egui::Area::new(egui::Id::new("generating"))
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label("Generating puzzle…");
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            });
        cancel
    }
}

impl eframe::App for PuzzlesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut commands = Vec::new();
        if self.dialog.is_none() {
            ctx.input_mut(|i| {
                for (shortcut, command) in shortcuts::shortcuts(self.mac) {
                    if i.consume_shortcut(&shortcut) {
                        commands.push(command);
                    }
                }
            });
        }

        if let Screen::Game { session, .. } = &mut self.screen {
            session.poll();
            session.tick(Instant::now());
        }
        match self.toolbar(ui) {
            Some(ToolbarAction::Command(command)) => commands.push(command),
            Some(ToolbarAction::Menu(action)) => self.menu_action(&ctx, action),
            None => {}
        }
        for command in commands {
            self.run(&ctx, command);
        }

        let mut outcome = Outcome::Continue;
        match &self.screen {
            Screen::Library => self.library(ui),
            Screen::Game { .. } => outcome = self.game(ui),
        }
        if self.dialog.is_some() {
            self.dialog(&ctx);
        }
        if let Screen::Game { session, .. } = &mut self.screen {
            if session.is_generating() && Self::generating(ui) && !session.cancel_generation() {
                self.screen = Screen::Library;
            } else if session.wants_timer() || session.needs_redraw() {
                // Input after this frame's redraw left changes to draw.
                ctx.request_repaint();
            }
        }
        if outcome == Outcome::Quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.set_title(&ctx);
    }
}

#[cfg(test)]
mod tests;
