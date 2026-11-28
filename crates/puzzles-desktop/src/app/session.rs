//! One open game: its mid-end, its canvas, and New Game running on a
//! worker thread (threading and desktop shell decisions in
//! `docs/porting-plan.md`). Independent of the UI toolkit.
//!
//! Each New Game starts its own thread. Cancelling drops the result;
//! the thread can't be stopped (the ported generators have no cancel
//! points), so a generation that never ends keeps running until the app
//! quits.
//!
//! Session restore: each game's position is kept in `sessions/<game>.sav`
//! (a C save file), written about a second after the game changes, and
//! when the app leaves the game or quits. Opening the game resumes it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use puzzles_canvas::Canvas;
use puzzles_core::button::{Button, ButtonKind, UiAction};
use puzzles_core::config::ConfigItem;
use puzzles_core::erased::{Capabilities, Puzzle};
use puzzles_core::midend::{Config, ConfigKind, GeneratedGame, KeyResult, MidendOptions};
use puzzles_core::preset::PresetMenu;
use puzzles_games::GameInfo;
use puzzles_theme::{Rgb, ThemeSet};

use super::command::Command;
use super::paths::{AppDirs, write_atomically};
use super::user_presets::{PresetsFile, game_key};

/// Wakes the UI when a generation finishes (from the worker thread).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How long a changed game waits before it is saved, so a burst of
/// moves is written once. A crash loses at most this much play.
pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

/// What the app should do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Carry on.
    Continue,
    /// The user asked to quit (C's `q`).
    Quit,
    /// The game didn't use the key (C's `PKR_UNUSED`): the app may.
    Unused,
}

/// A New Game running on its thread.
struct Generation {
    result: Receiver<GeneratedGame>,
}

/// The layout last applied: the area in pixels and the pixel ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LayoutKey {
    area: (u32, u32),
    pixels_per_point: f32,
}

/// An open game.
pub struct Session {
    info: &'static GameInfo,
    puzzle: Box<dyn Puzzle>,
    colours: Vec<Rgb>,
    canvas: Option<Canvas>,
    /// Bumped whenever the canvas is replaced or resized, so the screen
    /// copy is uploaded in full.
    canvas_version: u64,
    generation: Option<Generation>,
    has_game: bool,
    layout: Option<LayoutKey>,
    last_tick: Option<Instant>,
    message: Option<String>,
    waker: Waker,
    /// Presets from `<GAME>_PRESETS`, listed after the file's.
    env_presets: PresetMenu<String>,
    /// Problems with the presets file, for the presets menu.
    preset_errors: Vec<String>,
    /// Where this game's preferences are saved, if anywhere.
    prefs_path: Option<PathBuf>,
    /// Where this game's session is kept, if anywhere.
    saved_path: Option<PathBuf>,
    /// When the game first changed since the session was last written.
    unsaved_since: Option<Instant>,
}

impl Session {
    /// Opens `info`'s game in the theme `theme`, with its preferences.
    /// There is no puzzle yet: [`resume`](Self::resume) or
    /// [`load`](Self::load) one.
    ///
    /// # Errors
    /// The theme can't colour this game.
    pub fn new(
        info: &'static GameInfo,
        themes: &ThemeSet,
        theme: &str,
        waker: Waker,
        dirs: Option<&AppDirs>,
    ) -> Result<Self, String> {
        let mut options = MidendOptions::from_lookup(info.name, |name| std::env::var(name).ok());
        options.defer_new_game = true;
        let env_presets = options.extra_presets.clone();
        let seed: [u8; 16] = rand::random();
        let mut puzzle = info.new_midend(&seed, true, options);
        let prefs_path = dirs.map(|d| d.prefs(info.name));
        let saved_path = dirs.map(|d| d.session(info.name));
        let mut message = None;
        if let Some(path) = &prefs_path
            && let Ok(data) = std::fs::read(path)
            && let Err(e) = puzzle.load_prefs(&mut data.as_slice())
        {
            message = Some(format!("{}: {e}", path.display()));
        }
        let colours = themes
            .resolve(theme, info.name, puzzle.palette())
            .map_err(|e| format!("{}: {e}", info.name))?;
        Ok(Self {
            info,
            puzzle,
            colours,
            canvas: None,
            canvas_version: 0,
            generation: None,
            has_game: false,
            layout: None,
            last_tick: None,
            message,
            waker,
            env_presets,
            preset_errors: Vec::new(),
            prefs_path,
            saved_path,
            unsaved_since: None,
        })
    }

    /// Resumes the game's saved session, or starts generating a puzzle if
    /// there is none (or it can't be read, saying why).
    pub fn resume(&mut self) {
        let saved = self
            .saved_path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok());
        let Some(data) = saved else {
            self.start_new_game();
            return;
        };
        if let Err(e) = self.puzzle.deserialise(&mut data.as_slice()) {
            self.start_new_game();
            self.message = Some(format!(
                "Couldn't resume the last {} game: {e}",
                self.info.name
            ));
        } else {
            self.installed();
        }
    }

    /// Replaces the game with a saved one (a C save file of this game),
    /// dropping a New Game in progress. It becomes the game's session.
    ///
    /// # Errors
    /// C's message if the file is rejected; nothing changes then.
    pub fn load(&mut self, data: &[u8]) -> Result<(), String> {
        self.puzzle
            .deserialise(&mut &data[..])
            .map_err(|e| e.to_string())?;
        self.generation = None;
        self.message = None;
        self.installed();
        self.changed();
        Ok(())
    }

    /// A puzzle is in place: lay it out and draw it.
    fn installed(&mut self) {
        self.has_game = true;
        self.layout = None;
    }

    /// The game changed, so the session is written soon.
    fn changed(&mut self) {
        self.unsaved_since.get_or_insert_with(Instant::now);
    }

    /// The game as a save file, if there is one.
    fn serialise(&self) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        (self.has_game && self.puzzle.serialise(&mut data).is_ok()).then_some(data)
    }

    /// Writes the game to `path` as a save file.
    ///
    /// # Errors
    /// A message for the user: there's no game yet, or writing failed.
    pub fn save_as(&self, path: &Path) -> Result<(), String> {
        let data = self
            .serialise()
            .ok_or_else(|| "There is no game to save yet.".to_owned())?;
        write_atomically(path, &data).map_err(|e| format!("Saving {}: {e}", path.display()))
    }

    /// Writes the session now, if there is a game and a place for it.
    ///
    /// # Errors
    /// A message for the user if writing failed.
    pub fn save_session(&mut self) -> Result<(), String> {
        self.unsaved_since = None;
        match (&self.saved_path, self.serialise()) {
            (Some(path), Some(data)) => write_atomically(path, &data)
                .map_err(|e| format!("Saving the game to {}: {e}", path.display())),
            _ => Ok(()),
        }
    }

    /// Writes the session if the game changed at least
    /// [`AUTOSAVE_DELAY`] before `now`. Returns how long until the next
    /// write is due, if one is waiting.
    pub fn autosave(&mut self, now: Instant) -> Option<Duration> {
        let due = self.unsaved_since? + AUTOSAVE_DELAY;
        if now < due {
            return Some(due - now);
        }
        if let Err(e) = self.save_session() {
            self.message = Some(e);
        }
        None
    }

    /// Recolours the game in the theme `theme`, redrawing it in full.
    ///
    /// # Errors
    /// The theme can't colour this game; the colours stay as they were.
    pub fn set_theme(&mut self, themes: &ThemeSet, theme: &str) -> Result<(), String> {
        let colours = themes
            .resolve(theme, self.info.name, self.puzzle.palette())
            .map_err(|e| format!("{}: {e}", self.info.name))?;
        if let Some(canvas) = &mut self.canvas {
            canvas.set_colours(&colours);
            canvas.fill_background();
        }
        self.colours = colours;
        self.puzzle.force_redraw();
        Ok(())
    }

    /// The game's name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.info.name
    }

    /// What the game can do.
    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        self.puzzle.capabilities()
    }

    /// The background colour, for the area around the puzzle.
    #[must_use]
    pub fn background(&self) -> Rgb {
        self.colours
            .first()
            .copied()
            .unwrap_or(Rgb::new(1.0, 1.0, 1.0))
    }

    /// True while a New Game is generating.
    #[must_use]
    pub fn is_generating(&self) -> bool {
        self.generation.is_some()
    }

    /// Starts a New Game. A descriptive game ID finishes at once; a
    /// random puzzle generates on its own thread, and the current one
    /// stays until it is ready.
    pub fn start_new_game(&mut self) {
        self.message = None;
        let job = self.puzzle.begin_new_game();
        if !job.generates() {
            self.generation = None;
            let generated = job.run();
            self.finish(generated);
            return;
        }
        let (tx, rx) = mpsc::channel();
        let waker = Arc::clone(&self.waker);
        std::thread::spawn(move || {
            // A send fails only if the session was dropped or the job
            // cancelled; the result is unwanted then.
            let _ = tx.send(job.run());
            waker();
        });
        self.generation = Some(Generation { result: rx });
    }

    /// Drops the New Game in progress. Returns whether a game is left to
    /// show.
    pub fn cancel_generation(&mut self) -> bool {
        self.generation = None;
        self.has_game
    }

    /// Installs a finished generation, if one is ready.
    pub fn poll(&mut self) {
        let Some(generation) = &self.generation else {
            return;
        };
        match generation.result.try_recv() {
            Ok(generated) => {
                self.generation = None;
                self.finish(generated);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.generation = None;
                self.message = Some("Generating the puzzle failed.".to_owned());
            }
        }
    }

    fn finish(&mut self, generated: GeneratedGame) {
        if self.puzzle.finish_new_game(generated) {
            self.installed();
            self.changed();
        }
    }

    /// Sizes the puzzle to fit `area` physical pixels (at
    /// `pixels_per_point`), as large as fits, when either changed or the
    /// game did.
    pub fn layout(&mut self, area: (u32, u32), pixels_per_point: f32) {
        let key = LayoutKey {
            area,
            pixels_per_point,
        };
        if !self.has_game || self.layout == Some(key) {
            return;
        }
        let clamp = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
        let (w, h) = self.puzzle.size(
            clamp(area.0),
            clamp(area.1),
            true,
            f64::from(pixels_per_point),
        );
        let (Ok(w), Ok(h)) = (u32::try_from(w), u32::try_from(h)) else {
            return;
        };
        let resized = match &mut self.canvas {
            Some(canvas) => canvas.resize(w, h).is_ok(),
            None => match Canvas::new(w, h, &self.colours) {
                Ok(canvas) => {
                    self.canvas = Some(canvas);
                    true
                }
                Err(_) => false,
            },
        };
        if resized {
            self.canvas_version += 1;
            self.puzzle.force_redraw();
            self.layout = Some(key);
        }
    }

    /// Advances animations and the timer to `now`.
    pub fn tick(&mut self, now: Instant) {
        if let Some(last) = self.last_tick
            && self.has_game
            && self.puzzle.wants_timer()
        {
            self.puzzle.timer(now.duration_since(last).as_secs_f32());
        }
        self.last_tick = Some(now);
    }

    /// True while animations or the timer run, so the UI keeps
    /// repainting.
    #[must_use]
    pub fn wants_timer(&self) -> bool {
        self.has_game && self.puzzle.wants_timer()
    }

    /// True if the puzzle has changes not drawn yet.
    #[must_use]
    pub fn needs_redraw(&self) -> bool {
        self.has_game && self.puzzle.is_dirty()
    }

    /// Draws what changed onto the canvas. Returns whether it did.
    pub fn redraw(&mut self) -> bool {
        match &mut self.canvas {
            Some(canvas) if self.has_game && self.puzzle.is_dirty() => {
                self.puzzle.redraw(canvas);
                true
            }
            _ => false,
        }
    }

    /// The canvas and its version (see `canvas_version`).
    pub fn canvas_mut(&mut self) -> Option<(&mut Canvas, u64)> {
        let version = self.canvas_version;
        self.canvas.as_mut().map(|c| (c, version))
    }

    /// Sends a key or mouse event at puzzle pixel `(x, y)`. Ignored while
    /// generating.
    pub fn input(&mut self, x: i32, y: i32, button: Button) -> Outcome {
        if !self.has_game || self.is_generating() {
            return Outcome::Continue;
        }
        match self.puzzle.process_key(x, y, button) {
            KeyResult::NewGame => {
                self.start_new_game();
                Outcome::Continue
            }
            KeyResult::Quit => Outcome::Quit,
            KeyResult::SomeEffect => {
                self.changed();
                Outcome::Continue
            }
            KeyResult::NoEffect => Outcome::Continue,
            KeyResult::Unused => Outcome::Unused,
        }
    }

    /// True if the command can run now.
    #[must_use]
    pub fn can(&self, command: Command) -> bool {
        let playing = self.has_game && !self.is_generating();
        match command {
            Command::Games
            | Command::Load
            | Command::Quit
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomFit
            | Command::FullScreen
            | Command::Immersive => true,
            Command::Save => self.has_game,
            Command::NewGame => !self.is_generating(),
            Command::Restart => playing,
            Command::Undo => playing && self.puzzle.can_undo(),
            Command::Redo => playing && self.puzzle.can_redo(),
            Command::Solve => playing && self.capabilities().can_solve,
        }
    }

    /// Runs a game command. The app's own (game list, files, zoom,
    /// full screen, immersive mode) are the UI's.
    pub fn command(&mut self, command: Command) -> Outcome {
        if !self.can(command) {
            return Outcome::Continue;
        }
        self.message = None;
        match command {
            Command::Games
            | Command::Save
            | Command::Load
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomFit
            | Command::FullScreen
            | Command::Immersive => Outcome::Continue,
            Command::Quit => Outcome::Quit,
            Command::NewGame => {
                self.start_new_game();
                Outcome::Continue
            }
            Command::Restart => {
                self.puzzle.restart_game();
                self.changed();
                Outcome::Continue
            }
            Command::Undo => self.input(0, 0, Button::new(ButtonKind::Ui(UiAction::Undo))),
            Command::Redo => self.input(0, 0, Button::new(ButtonKind::Ui(UiAction::Redo))),
            Command::Solve => {
                match self.puzzle.solve() {
                    Ok(()) => self.changed(),
                    Err(e) => self.message = Some(e.to_string()),
                }
                Outcome::Continue
            }
        }
    }

    /// The status bar line: a message for the user if there is one, else
    /// the game's status (with the timer for timed games).
    #[must_use]
    pub fn status_line(&self) -> String {
        if let Some(message) = &self.message {
            return message.clone();
        }
        if self.has_game {
            self.puzzle.status_text()
        } else {
            String::new()
        }
    }

    // --------------------------------------------------------------
    // Presets

    /// Takes the presets file's entries for this game (after the game's
    /// own, before `<GAME>_PRESETS`) and remembers its problems.
    pub fn use_presets_file(&mut self, file: &PresetsFile) {
        let known = |key: &str| puzzles_games::GAMES.iter().any(|g| game_key(g.name) == key);
        let puzzle = &self.puzzle;
        let (mut menu, errors) = file.presets().menu_for(self.info.name, known, |p| {
            puzzle.check_params(p).map_err(|e| e.to_string())
        });
        menu.entries
            .extend(self.env_presets.entries.iter().cloned());
        self.puzzle.set_extra_presets(menu);
        self.preset_errors = errors;
    }

    /// The presets menu, each preset carrying its id.
    pub fn presets(&mut self) -> &PresetMenu<usize> {
        self.puzzle.presets()
    }

    /// The preset matching the current parameters.
    #[must_use]
    pub fn current_preset(&self) -> Option<usize> {
        self.puzzle.which_preset()
    }

    /// Problems with the presets file.
    #[must_use]
    pub fn preset_errors(&self) -> &[String] {
        &self.preset_errors
    }

    /// Starts a new game with a preset's parameters.
    pub fn choose_preset(&mut self, id: usize) {
        if !self.is_generating() && self.puzzle.set_preset(id) {
            self.start_new_game();
        }
    }

    // --------------------------------------------------------------
    // Dialogs

    /// What a dialog shows, or `None` if it doesn't apply now (no
    /// Custom dialog, or no game for an ID yet).
    #[must_use]
    pub fn config(&self, kind: ConfigKind) -> Option<Config> {
        self.puzzle.get_config(kind)
    }

    /// Applies a dialog's fields: new parameters or a game ID start a
    /// new game; preferences are saved.
    ///
    /// # Errors
    /// The game's message, for the dialog to show; nothing changes then.
    pub fn apply_config(&mut self, kind: ConfigKind, items: &[ConfigItem]) -> Result<(), String> {
        if self.is_generating() {
            return Err("A new game is still being generated.".to_owned());
        }
        self.puzzle
            .set_config(kind, items)
            .map_err(|e| e.to_string())?;
        match kind {
            ConfigKind::Settings | ConfigKind::Seed | ConfigKind::Desc => self.start_new_game(),
            ConfigKind::Prefs => {
                self.puzzle.force_redraw();
                self.save_prefs();
                // Preferences can change the UI, which the session keeps.
                self.changed();
            }
        }
        Ok(())
    }

    fn save_prefs(&mut self) {
        let Some(path) = &self.prefs_path else {
            return;
        };
        let mut data = Vec::new();
        let saved = self
            .puzzle
            .save_prefs(&mut data)
            .and_then(|()| write_atomically(path, &data));
        if let Err(e) = saved {
            self.message = Some(format!("Saving preferences to {}: {e}", path.display()));
        }
    }

    /// The current game's descriptive ID, for copying.
    #[must_use]
    pub fn game_id(&self) -> Option<String> {
        self.puzzle.get_game_id()
    }

    /// The current game's random seed ID, if it came from one.
    #[must_use]
    pub fn random_seed(&self) -> Option<String> {
        self.puzzle.get_random_seed()
    }

    /// Shows a line in the status bar until the next command.
    pub fn notify(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    /// The window title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("{} — Puzzles", self.info.name)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use puzzles_core::button::{MouseAction, MouseButton};

    use super::*;

    fn flip() -> &'static GameInfo {
        puzzles_games::find("Flip").expect("built in")
    }

    /// A Flip session keeping its files in `dirs`, with no puzzle yet.
    fn session_in(dirs: Option<&AppDirs>) -> Session {
        Session::new(
            flip(),
            &ThemeSet::builtin(),
            "classic",
            Arc::new(|| {}),
            dirs,
        )
        .unwrap()
    }

    fn open() -> Session {
        let mut s = session_in(None);
        s.resume();
        s
    }

    /// A fresh folder for one test's files.
    fn test_dirs(name: &str) -> AppDirs {
        let dir =
            std::env::temp_dir().join(format!("puzzles-session-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        AppDirs::at(dir)
    }

    /// Clicks the middle of the puzzle, a move in Flip.
    fn play(s: &mut Session) {
        let (w, h) = {
            let (c, _) = s.canvas_mut().unwrap();
            (c.width() as i32, c.height() as i32)
        };
        for action in [MouseAction::Press, MouseAction::Release] {
            s.input(w / 2, h / 2, Button::mouse(MouseButton::Left, action));
        }
    }

    /// A session with a generated game and one move, its files in `dirs`.
    fn played_in(dirs: &AppDirs) -> Session {
        let mut s = session_in(Some(dirs));
        s.resume();
        wait(&mut s);
        s.layout((400, 300), 1.0);
        play(&mut s);
        s
    }

    /// Waits for the generation in progress, as the UI does when woken.
    fn wait(session: &mut Session) {
        let generation = session.generation.take().expect("generating");
        let generated = generation
            .result
            .recv_timeout(Duration::from_secs(10))
            .expect("generates in time");
        session.finish(generated);
    }

    fn ready() -> Session {
        let mut s = open();
        wait(&mut s);
        s.layout((400, 300), 1.0);
        s
    }

    #[test]
    fn the_first_game_generates_off_the_ui_thread() {
        let mut s = open();
        assert!(s.is_generating() && !s.has_game);
        assert!(!s.redraw(), "nothing to draw yet");
        assert!(!s.can(Command::Undo) && s.can(Command::Games));
        wait(&mut s);
        assert!(s.has_game && !s.is_generating());
        s.layout((400, 300), 1.0);
        let (canvas, _) = s.canvas_mut().expect("a canvas");
        assert!(canvas.width() <= 400 && canvas.height() <= 300);
        assert!(canvas.height() > 250, "as large as fits");
        assert!(s.redraw());
        assert!(!s.redraw(), "nothing changed since");
    }

    #[test]
    fn layout_runs_again_only_when_something_changed() {
        let mut s = ready();
        let version = s.canvas_mut().unwrap().1;
        s.layout((400, 300), 1.0);
        assert_eq!(s.canvas_mut().unwrap().1, version);
        s.layout((800, 600), 2.0);
        assert_eq!(s.canvas_mut().unwrap().1, version + 1);
    }

    #[test]
    fn new_game_keeps_the_old_game_until_the_new_one_is_ready() {
        let mut s = ready();
        s.redraw();
        let before = s.puzzle.get_game_id();
        assert_eq!(s.input(0, 0, Button::key('n')), Outcome::Continue);
        assert!(s.is_generating(), "'n' starts a generation");
        assert_eq!(s.puzzle.get_game_id(), before);
        assert_eq!(
            s.input(10, 10, Button::mouse(MouseButton::Left, MouseAction::Press)),
            Outcome::Continue,
            "input waits"
        );
        assert!(s.cancel_generation(), "the old game is still there");
        assert_eq!(s.puzzle.get_game_id(), before);

        s.command(Command::NewGame);
        wait(&mut s);
        assert_ne!(s.puzzle.get_game_id(), before);
    }

    #[test]
    fn cancelling_the_first_game_leaves_nothing() {
        let mut s = open();
        assert!(!s.cancel_generation());
        s.poll();
        assert!(!s.has_game);
    }

    #[test]
    fn commands_reach_the_game() {
        let mut s = ready();
        assert!(!s.can(Command::Undo));
        let (w, h) = {
            let (c, _) = s.canvas_mut().unwrap();
            (c.width() as i32, c.height() as i32)
        };
        s.input(
            w / 2,
            h / 2,
            Button::mouse(MouseButton::Left, MouseAction::Press),
        );
        s.input(
            w / 2,
            h / 2,
            Button::mouse(MouseButton::Left, MouseAction::Release),
        );
        assert!(s.can(Command::Undo));
        s.command(Command::Undo);
        assert!(s.can(Command::Redo) && !s.can(Command::Undo));
        s.command(Command::Solve);
        assert!(s.can(Command::Undo), "solving is a move");
        assert_eq!(s.input(0, 0, Button::key('q')), Outcome::Quit);
        assert_eq!(s.command(Command::Quit), Outcome::Quit);
        assert_eq!(s.title(), "Flip — Puzzles");
    }

    #[test]
    fn a_saved_session_resumes_where_it_was_left() {
        let dirs = test_dirs("resume");
        let mut s = played_in(&dirs);
        let id = s.game_id();
        assert!(s.can(Command::Undo));
        s.save_session().unwrap();
        assert!(dirs.session("Flip").exists());

        let mut again = session_in(Some(&dirs));
        again.resume();
        assert!(again.has_game && !again.is_generating(), "no generation");
        assert_eq!(again.game_id(), id);
        assert!(again.can(Command::Undo), "with its moves");
        std::fs::remove_dir_all(dirs.session("Flip").parent().unwrap()).unwrap();
    }

    #[test]
    fn changes_are_saved_once_play_pauses() {
        let dirs = test_dirs("autosave");
        let mut s = session_in(Some(&dirs));
        s.resume();
        let now = Instant::now();
        assert_eq!(s.autosave(now), None, "nothing to save while generating");
        wait(&mut s);
        let since = s.unsaved_since.expect("a new game is a change");
        let wait = s.autosave(since).expect("waits");
        assert!(wait <= AUTOSAVE_DELAY && wait > Duration::ZERO);
        assert!(!dirs.session("Flip").exists());

        assert_eq!(s.autosave(since + AUTOSAVE_DELAY), None);
        let saved = std::fs::read(dirs.session("Flip")).expect("saved");
        assert_eq!(s.autosave(since + 2 * AUTOSAVE_DELAY), None, "nothing new");

        s.layout((400, 300), 1.0);
        play(&mut s);
        let since = s.unsaved_since.expect("a move is a change");
        s.autosave(since + AUTOSAVE_DELAY);
        assert_ne!(std::fs::read(dirs.session("Flip")).unwrap(), saved);
        std::fs::remove_dir_all(dirs.session("Flip").parent().unwrap()).unwrap();
    }

    #[test]
    fn a_broken_session_starts_a_new_game_and_says_so() {
        let dirs = test_dirs("broken");
        let path = dirs.session("Flip");
        write_atomically(&path, b"SAVEFILE:5:wrong").unwrap();
        let mut s = session_in(Some(&dirs));
        s.resume();
        assert!(s.is_generating());
        assert!(
            s.status_line()
                .starts_with("Couldn't resume the last Flip game"),
            "{}",
            s.status_line()
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn saved_games_load_and_replace_the_game() {
        let dirs = test_dirs("load");
        let s = played_in(&dirs);
        let file = dirs.session("Flip").with_file_name("mine.sav");
        s.save_as(&file).unwrap();
        let data = std::fs::read(&file).unwrap();

        let mut other = ready();
        assert_ne!(other.game_id(), s.game_id());
        other.command(Command::NewGame);
        assert!(other.is_generating());
        other.load(&data).unwrap();
        assert!(!other.is_generating(), "loading drops the New Game");
        assert_eq!(other.game_id(), s.game_id());
        assert!(other.can(Command::Undo));

        let id = other.game_id();
        assert_eq!(
            other.load(&with_game(&data, "Fifteen")).unwrap_err(),
            "Save file is from a different game"
        );
        assert_eq!(other.game_id(), id, "unchanged");
        assert!(session_in(None).save_as(&file).is_err(), "no game yet");
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    /// A Flip save file with its `GAME` line naming another game.
    fn with_game(data: &[u8], name: &str) -> Vec<u8> {
        String::from_utf8(data.to_vec())
            .unwrap()
            .replace(
                "GAME    :4:Flip",
                &format!("GAME    :{}:{name}", name.len()),
            )
            .into_bytes()
    }

    #[test]
    fn ticks_drive_animations() {
        let mut s = ready();
        let (w, h) = {
            let (c, _) = s.canvas_mut().unwrap();
            (c.width() as i32, c.height() as i32)
        };
        let t0 = Instant::now();
        s.tick(t0);
        s.input(
            w / 2,
            h / 2,
            Button::mouse(MouseButton::Left, MouseAction::Press),
        );
        s.input(
            w / 2,
            h / 2,
            Button::mouse(MouseButton::Left, MouseAction::Release),
        );
        assert!(s.wants_timer(), "Flip animates the flip");
        s.tick(t0 + Duration::from_secs(5));
        assert!(!s.wants_timer(), "and finishes");
    }
}
