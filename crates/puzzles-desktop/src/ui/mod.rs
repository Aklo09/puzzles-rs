//! The egui side of the app: the game library, the toolbar, the puzzle
//! view, the status bar and the "Generating…" overlay (desktop shell
//! decision in `docs/porting-plan.md`).

use eframe::egui;
mod fonts;
mod input;
mod puzzle_view;
mod shortcuts;

use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Frame, Margin, RichText};
use egui_phosphor::regular as icon;
use puzzles_games::{GAMES, GameInfo};
use puzzles_theme::ThemeSet;

use crate::app::command::Command;
use crate::app::session::{Outcome, Session};
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
}

impl PuzzlesApp {
    /// Sets up fonts and style.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::install(&cc.egui_ctx);
        // Classic is a light theme; themed app colours come with themes.
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        Self {
            screen: Screen::Library,
            themes: ThemeSet::builtin(),
            mac: cfg!(target_os = "macos"),
            title: String::new(),
            error: None,
        }
    }

    fn open(&mut self, ctx: &egui::Context, info: &'static GameInfo) {
        let repaint = ctx.clone();
        let waker = Arc::new(move || repaint.request_repaint());
        match Session::new(info, &self.themes, THEME, waker) {
            Ok(session) => {
                self.error = None;
                self.screen = Screen::Game {
                    session: Box::new(session),
                    view: PuzzleView::default(),
                };
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn run(&mut self, ctx: &egui::Context, command: Command) {
        match (command, &mut self.screen) {
            (Command::Games, _) => self.screen = Screen::Library,
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

    fn toolbar(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        let Screen::Game { session, .. } = &self.screen else {
            return None;
        };
        let mut clicked = None;
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                for entry in Command::TOOLBAR {
                    let Some(command) = entry else {
                        ui.separator();
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
                        .on_hover_text(command.tooltip(self.mac))
                        .on_disabled_hover_text(command.tooltip(self.mac));
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
                        clicked = Some(command);
                    }
                }
            });
        });
        clicked
    }

    fn game(&mut self, ui: &mut egui::Ui) -> Outcome {
        let mac = self.mac;
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
            .show(ui, |ui| view.show(ui, session, mac))
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
        ctx.input_mut(|i| {
            for (shortcut, command) in shortcuts::shortcuts(self.mac) {
                if i.consume_shortcut(&shortcut) {
                    commands.push(command);
                }
            }
        });

        if let Screen::Game { session, .. } = &mut self.screen {
            session.poll();
            session.tick(Instant::now());
        }
        if let Some(command) = self.toolbar(ui) {
            commands.push(command);
        }
        for command in commands {
            self.run(&ctx, command);
        }

        let mut outcome = Outcome::Continue;
        match &self.screen {
            Screen::Library => self.library(ui),
            Screen::Game { .. } => outcome = self.game(ui),
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
