//! The egui side of the app: the game library, the toolbar with its
//! Type and Game menus, the puzzle view, the status bar, configuration,
//! Settings and file dialogs, themes, saved games dropped on the window,
//! session restore, and the "Generating…" overlay (desktop shell,
//! dialogs and presets, themes, and persistence decisions in
//! `docs/porting-plan.md`).

use eframe::egui;
mod dialogs;
mod files;
mod fonts;
mod input;
mod menus;
mod puzzle_view;
mod settings;
mod shortcuts;
mod toolbar;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Frame, Margin, RichText};
use egui_file_dialog::FileDialogStorage;
use egui_phosphor::regular as icon;
use puzzles_core::midend::ConfigKind;
use puzzles_games::{GAMES, GameInfo};

use crate::app::command::Command;
use crate::app::immersive::{self, Reveal};
use crate::app::paths::AppDirs;
use crate::app::save_file::SaveFile;
use crate::app::session::{Outcome, Session};
use crate::app::settings::Settings;
use crate::app::themes::Themes;
use crate::app::user_presets::PresetsFile;
use crate::app::watch::FolderWatch;
use dialogs::{Dialog, DialogAction};
use files::{FileAction, FileDialog};
use menus::MenuAction;
use puzzle_view::PuzzleView;
use settings::{SettingsAction, SettingsDialog};
use toolbar::{ToolbarAction, ToolbarState};

/// The theme used when the chosen one can't be found.
const FALLBACK_THEME: &str = "Light";

/// The file dialog's folders in eframe's storage.
const FILE_DIALOG_KEY: &str = "file_dialog";

/// An open dialog.
#[derive(Debug)]
enum OpenDialog {
    Config(Dialog),
    Settings(SettingsDialog),
    File(Box<FileDialog>),
}

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
    themes: Themes,
    settings: Settings,
    /// The theme in use, once applied.
    applied_theme: Option<String>,
    theme_watch: Option<FolderWatch>,
    mac: bool,
    title: String,
    error: Option<String>,
    dirs: Option<AppDirs>,
    presets_file: Option<PresetsFile>,
    dialog: Option<OpenDialog>,
    /// What the file dialogs remember.
    file_storage: FileDialogStorage,
    /// False until the first frame has reopened the last game.
    started: bool,
    /// A saved game to open at launch (from the command line).
    launch_file: Option<PathBuf>,
    /// The toolbar showing in immersive mode.
    reveal: Reveal,
}

impl PuzzlesApp {
    /// Sets up fonts and style, keeping the user's files in `dirs` (or
    /// nowhere). The first frame opens `launch_file`, a saved game, if
    /// given, else the game open when the app last ran.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        dirs: Option<AppDirs>,
        launch_file: Option<PathBuf>,
    ) -> Self {
        let mut app = Self::with_dirs(cc, dirs);
        app.launch_file = launch_file;
        app
    }

    /// [`new`](Self::new) without a file to open.
    pub fn with_dirs(cc: &eframe::CreationContext<'_>, dirs: Option<AppDirs>) -> Self {
        fonts::install(&cc.egui_ctx);
        // Cmd/Ctrl with +, - and 0 zoom the puzzle, not egui's UI.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let file_storage = cc
            .storage
            .and_then(|s| eframe::get_value(s, FILE_DIALOG_KEY))
            .unwrap_or_default();
        let presets_file = dirs.as_ref().map(|d| PresetsFile::new(d.presets()));
        let (settings, mut error) = dirs.as_ref().map_or((Settings::default(), None), |d| {
            Settings::load(&d.settings())
        });
        let themes = Themes::load(dirs.as_ref().map(AppDirs::themes).as_deref());
        let repaint = cc.egui_ctx.clone();
        let theme_watch = dirs.as_ref().and_then(|d| {
            FolderWatch::new(&d.themes(), Arc::new(move || repaint.request_repaint()))
                .map_err(|e| error = Some(format!("Watching themes: {e}")))
                .ok()
        });
        Self {
            screen: Screen::Library,
            themes,
            settings,
            applied_theme: None,
            theme_watch,
            mac: cfg!(target_os = "macos"),
            title: String::new(),
            error,
            dirs,
            presets_file,
            dialog: None,
            file_storage,
            started: false,
            launch_file: None,
            reveal: Reveal::default(),
        }
    }

    /// Opens the saved game named on the command line, or else the game
    /// open when the app last ran.
    fn start(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.launch_file.take() {
            match SaveFile::read(&path) {
                Ok(file) => self.open_save(ctx, &file),
                Err(e) => self.error = Some(e),
            }
        } else if let Some(info) = self
            .settings
            .last_game
            .as_deref()
            .and_then(puzzles_games::find)
        {
            self.open(ctx, info);
        }
    }

    /// Shows a message: in the status bar with a game open, else in the
    /// library.
    fn report(&mut self, message: String) {
        match &mut self.screen {
            Screen::Game { session, .. } => session.notify(message),
            Screen::Library => self.error = Some(message),
        }
    }

    /// Writes the open game's session, before leaving it.
    fn save_session(&mut self) -> Option<String> {
        match &mut self.screen {
            Screen::Game { session, .. } => session.save_session().err(),
            Screen::Library => None,
        }
    }

    /// Records the screen in the settings, so the next launch reopens it.
    fn remember_screen(&mut self) {
        let current = match &self.screen {
            Screen::Game { session, .. } => Some(session.name()),
            Screen::Library => None,
        };
        if self.settings.last_game.as_deref() == current {
            return;
        }
        self.settings.last_game = current.map(str::to_owned);
        if let Some(dirs) = &self.dirs
            && let Err(e) = self.settings.save(&dirs.settings())
        {
            self.report(format!("Saving settings: {e}"));
        }
    }

    /// The theme the settings and the system's appearance call for.
    fn wanted_theme(&self, ctx: &egui::Context) -> String {
        let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark);
        let wanted = self.settings.theme_for(system_dark);
        if self.themes.set().get(wanted).is_some() {
            wanted.to_owned()
        } else {
            FALLBACK_THEME.to_owned()
        }
    }

    /// Re-reads the user's themes if they changed, and applies the theme
    /// the settings call for if it isn't the one in use: egui's colours,
    /// and the open game's.
    fn update_theme(&mut self, ctx: &egui::Context) {
        if self
            .theme_watch
            .as_ref()
            .is_some_and(FolderWatch::take_changed)
        {
            self.themes = Themes::load(self.dirs.as_ref().map(AppDirs::themes).as_deref());
            self.applied_theme = None;
        }
        let wanted = self.wanted_theme(ctx);
        if self.applied_theme.as_deref() == Some(wanted.as_str()) {
            return;
        }
        let theme = match self.themes.set().ui_colours(&wanted) {
            Ok(ui) => {
                let egui_theme = if ui.dark {
                    egui::Theme::Dark
                } else {
                    egui::Theme::Light
                };
                ctx.set_theme(egui_theme);
                ctx.set_visuals_of(egui_theme, settings::visuals(ui));
                wanted
            }
            Err(e) => {
                self.error = Some(format!("Theme {wanted:?}: {e}"));
                FALLBACK_THEME.to_owned()
            }
        };
        if let Screen::Game { session, .. } = &mut self.screen
            && let Err(e) = session.set_theme(self.themes.set(), &theme)
        {
            session.notify(e);
        }
        self.applied_theme = Some(theme);
    }

    /// A session for `info`'s game, with its presets, but no puzzle yet.
    fn new_session(
        &mut self,
        ctx: &egui::Context,
        info: &'static GameInfo,
    ) -> Result<Session, String> {
        let repaint = ctx.clone();
        let waker = Arc::new(move || repaint.request_repaint());
        let theme = self.applied_theme.as_deref().unwrap_or(FALLBACK_THEME);
        let mut session = Session::new(info, self.themes.set(), theme, waker, self.dirs.as_ref())?;
        if let Some(file) = &mut self.presets_file {
            file.refresh();
            session.use_presets_file(file);
        }
        Ok(session)
    }

    /// Shows `session`, leaving the game open before (saving it).
    fn show(&mut self, mut session: Session) {
        if let Some(e) = self.save_session() {
            session.notify(e);
        }
        self.dialog = None;
        self.error = None;
        self.screen = Screen::Game {
            session: Box::new(session),
            view: PuzzleView::default(),
        };
    }

    /// Opens `info`'s game where it was left, or a new one.
    fn open(&mut self, ctx: &egui::Context, info: &'static GameInfo) {
        match self.new_session(ctx, info) {
            Ok(mut session) => {
                session.resume();
                self.show(session);
            }
            Err(e) => self.report(e),
        }
    }

    /// Opens a saved game, in its game (switching games if needed).
    fn open_save(&mut self, ctx: &egui::Context, file: &SaveFile) {
        if let Screen::Game { session, .. } = &mut self.screen
            && session.name() == file.game.name
        {
            if let Err(e) = session.load(&file.data) {
                session.notify(e);
            }
            return;
        }
        let loaded = self.new_session(ctx, file.game).and_then(|mut session| {
            session.load(&file.data)?;
            Ok(session)
        });
        match loaded {
            Ok(session) => self.show(session),
            Err(e) => self.report(e),
        }
    }

    /// Opens the saved game at `path`.
    fn open_path(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        match SaveFile::read(path) {
            Ok(file) => self.open_save(ctx, &file),
            Err(e) => self.report(e),
        }
    }

    /// Opens a saved game dropped on the window (the first, if several).
    fn dropped(&mut self, ctx: &egui::Context) {
        let path = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()));
        if let Some(path) = path {
            self.open_path(ctx, &path);
        }
    }

    /// While files are dragged over the window: says what dropping does.
    fn drop_hint(ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        egui::Area::new(egui::Id::new("drop-hint"))
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label("Drop a saved game to open it");
                });
            });
    }

    /// Opens Settings, with the open game's preferences.
    fn open_settings(&mut self) {
        let game = match &self.screen {
            Screen::Game { session, .. } => session
                .config(ConfigKind::Prefs)
                .filter(|c| !c.items.is_empty())
                .map(|c| (session.name(), c.items)),
            Screen::Library => None,
        };
        let dir = self.dirs.as_ref().map(|d| d.themes().display().to_string());
        self.dialog = Some(OpenDialog::Settings(SettingsDialog::new(
            self.settings.clone(),
            game,
            dir,
        )));
    }

    fn menu_action(&mut self, ctx: &egui::Context, action: MenuAction) {
        match action {
            MenuAction::Settings => return self.open_settings(),
            MenuAction::Command(command) => return self.run(ctx, command),
            _ => {}
        }
        let Screen::Game { session, .. } = &mut self.screen else {
            return;
        };
        match action {
            MenuAction::Settings | MenuAction::Command(_) => {}
            MenuAction::Preset(id) => session.choose_preset(id),
            MenuAction::Dialog(kind) => {
                self.dialog = session
                    .config(kind)
                    .map(|c| OpenDialog::Config(Dialog::new(kind, c)));
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
        match &mut self.dialog {
            Some(OpenDialog::Config(dialog)) => {
                let Screen::Game { session, .. } = &mut self.screen else {
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
            Some(OpenDialog::File(dialog)) => {
                let action = dialog.show(ctx);
                if action == FileAction::Open {
                    return;
                }
                self.file_storage = dialog.storage();
                self.dialog = None;
                match action {
                    FileAction::Open | FileAction::Close => {}
                    FileAction::Save(path) => {
                        if let Screen::Game { session, .. } = &mut self.screen {
                            match session.save_as(&path) {
                                Ok(()) => session.notify(format!("Saved to {}.", path.display())),
                                Err(e) => session.notify(e),
                            }
                        }
                    }
                    FileAction::Load(path) => self.open_path(ctx, &path),
                }
            }
            Some(OpenDialog::Settings(dialog)) => match dialog.show(ctx, &self.themes) {
                SettingsAction::Open => {}
                SettingsAction::Close => self.dialog = None,
                SettingsAction::Apply(mut settings, prefs) => {
                    settings.last_game.clone_from(&self.settings.last_game);
                    if let (Some(items), Screen::Game { session, .. }) = (prefs, &mut self.screen)
                        && let Err(e) = session.apply_config(ConfigKind::Prefs, &items)
                    {
                        dialog.set_error(e);
                        return;
                    }
                    if settings != self.settings {
                        if let Some(dirs) = &self.dirs
                            && let Err(e) = settings.save(&dirs.settings())
                        {
                            dialog.set_error(format!("Saving settings: {e}"));
                            return;
                        }
                        self.settings = settings;
                        self.applied_theme = None;
                    }
                    self.dialog = None;
                }
            },
            None => {}
        }
    }

    /// Shows a file dialog. The puzzle lets go of the keyboard first:
    /// the dialog ignores Escape while any widget has focus.
    fn open_file_dialog(&mut self, ctx: &egui::Context, dialog: FileDialog) {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        self.dialog = Some(OpenDialog::File(Box::new(dialog)));
    }

    fn run(&mut self, ctx: &egui::Context, command: Command) {
        match (command, &mut self.screen) {
            (Command::Games, _) => {
                self.dialog = None;
                let error = self.save_session();
                self.screen = Screen::Library;
                self.error = error;
            }
            (Command::Quit, _) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            (Command::FullScreen, _) => {
                let full = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            }
            (Command::Immersive, _) => self.toggle_immersive(),
            (Command::ZoomIn | Command::ZoomOut | Command::ZoomFit, Screen::Game { view, .. }) => {
                view.zoom_command(|zoom| match command {
                    Command::ZoomIn => zoom.zoom_in(),
                    Command::ZoomOut => zoom.zoom_out(),
                    _ => zoom.fit(),
                });
            }
            (Command::Load, _) => {
                let dialog = FileDialog::load(self.file_storage.clone());
                self.open_file_dialog(ctx, dialog);
            }
            (Command::Save, Screen::Game { session, .. }) => {
                if session.can(Command::Save) {
                    let dialog = FileDialog::save(self.file_storage.clone(), session.name());
                    self.open_file_dialog(ctx, dialog);
                }
            }
            (_, Screen::Game { session, .. }) => {
                if session.command(command) == Outcome::Quit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            (_, Screen::Library) => {}
        }
    }

    /// Enters or leaves immersive mode, remembering it. Entering shows
    /// the toolbar for a moment, so it is seen going.
    fn toggle_immersive(&mut self) {
        self.settings.immersive = !self.settings.immersive;
        if self.settings.immersive {
            self.reveal.show(Instant::now());
        } else {
            self.reveal.hide();
        }
        if let Some(dirs) = &self.dirs
            && let Err(e) = self.settings.save(&dirs.settings())
        {
            self.report(format!("Saving settings: {e}"));
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
        let mut settings = false;
        let mut load = false;
        let tooltip = Command::Load.tooltip(self.mac);
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Sides::new().show(
                ui,
                |_| {},
                |ui| {
                    settings = toolbar::settings_button(ui).clicked();
                    load = toolbar::icon_button(ui, icon::FOLDER_OPEN, "Load…", &tooltip, true)
                        .clicked();
                },
            );
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
        if settings {
            self.open_settings();
        } else if load {
            self.run(ui.ctx(), Command::Load);
        } else if let Some(info) = chosen {
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

    /// The toolbar: a panel at the top, or in immersive mode over the
    /// puzzle while revealed.
    fn toolbar(&mut self, ui: &mut egui::Ui, now: Instant) -> Option<ToolbarAction> {
        let ctx = ui.ctx().clone();
        let immersive = self.settings.immersive;
        let full_screen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
        let Screen::Game { session, view } = &mut self.screen else {
            return None;
        };
        let state = ToolbarState {
            mac: self.mac,
            zoom: view.applied_zoom(),
            full_screen,
            immersive,
        };
        let presets_file = &mut self.presets_file;
        if !immersive {
            return egui::Panel::top("toolbar")
                .show(ui, |ui| {
                    toolbar::contents(ui, session, presets_file, &state)
                })
                .inner;
        }
        let screen = ctx.content_rect();
        let band = immersive::Band::new(self.mac && full_screen);
        let near_top = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| band.near_top(p.y - screen.top()));
        if near_top || egui::Popup::is_any_open(&ctx) || self.dialog.is_some() {
            self.reveal.show(now);
        }
        if !self.reveal.visible(now) {
            return None;
        }
        if let Some(wait) = self.reveal.remaining(now) {
            ctx.request_repaint_after(wait);
        }
        let fill = ctx.global_style().visuals.panel_fill.gamma_multiply(0.94);
        let shown = egui::Area::new(egui::Id::new("toolbar-overlay"))
            .fixed_pos(screen.min + egui::vec2(0.0, band.top))
            .order(egui::Order::Foreground)
            .show(&ctx, |ui| {
                Frame::new()
                    .fill(fill)
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(screen.width() - 16.0);
                        toolbar::contents(ui, session, presets_file, &state)
                    })
                    .inner
            });
        if shown.response.contains_pointer() {
            self.reveal.show(now);
        }
        shown.inner
    }

    fn game(&mut self, ui: &mut egui::Ui) -> Outcome {
        let mac = self.mac;
        let input = self.dialog.is_none();
        let immersive = self.settings.immersive;
        let Screen::Game { session, view } = &mut self.screen else {
            return Outcome::Continue;
        };
        let status = session.status_line();
        let wants_bar = session.capabilities().wants_statusbar || session.capabilities().is_timed;
        if immersive {
            Self::status_overlay(ui.ctx(), &status);
        } else if wants_bar || !status.is_empty() {
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

    /// In immersive mode, the status line as a quiet label in the bottom
    /// left corner, which clicks pass through.
    fn status_overlay(ctx: &egui::Context, status: &str) {
        if status.is_empty() {
            return;
        }
        let visuals = &ctx.global_style().visuals;
        let fill = visuals.panel_fill.gamma_multiply(0.8);
        // An area's room is its size last frame, so a longer line would
        // wrap to the width of a shorter one: give it the window's width.
        let room = (ctx.content_rect().width() - 32.0).max(80.0);
        egui::Area::new(egui::Id::new("status-overlay"))
            .anchor(Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(room);
                Frame::new()
                    .fill(fill)
                    .corner_radius(4)
                    .inner_margin(Margin::symmetric(8, 3))
                    .show(ui, |ui| ui.label(RichText::new(status).small()));
            });
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
        self.update_theme(&ctx);
        if !self.started {
            self.started = true;
            self.start(&ctx);
        }
        if self.dialog.is_none() {
            self.dropped(&ctx);
        }
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

        let now = Instant::now();
        if let Screen::Game { session, .. } = &mut self.screen {
            session.poll();
            session.tick(now);
            if let Some(wait) = session.autosave(now) {
                ctx.request_repaint_after(wait);
            }
        }
        match self.toolbar(ui, now) {
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
        if let Screen::Game { session, view } = &mut self.screen {
            // Escape the game had no use for brings the toolbar back.
            if view.take_unused_escape() && self.settings.immersive {
                self.reveal.show(now);
                ctx.request_repaint();
            }
            if session.is_generating() && Self::generating(ui) && !session.cancel_generation() {
                self.screen = Screen::Library;
            } else if session.wants_timer() || session.needs_redraw() {
                // Input after this frame's redraw left changes to draw.
                ctx.request_repaint();
            }
        }
        Self::drop_hint(&ctx);
        if outcome == Outcome::Quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.set_title(&ctx);
        self.remember_screen();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, FILE_DIALOG_KEY, &self.file_storage);
    }

    fn on_exit(&mut self) {
        // Nowhere to show an error once the window is gone.
        let _ = self.save_session();
    }

    /// Only the window's place and the file dialog's folders are kept;
    /// egui's own memory (scroll positions, open menus) isn't.
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
