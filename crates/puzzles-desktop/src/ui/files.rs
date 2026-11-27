//! The Save and Load dialogs, drawn by `egui-file-dialog` so they look
//! like the rest of the app and UI tests can drive them (persistence
//! decision in `docs/porting-plan.md`). They list `.sav` files; the
//! folders last used, pinned folders and hidden-file choice are kept in
//! eframe's `app.ron`.

use std::path::PathBuf;

use eframe::egui;
use egui_file_dialog::{DialogState, FileDialogStorage};

use crate::app::save_file;

const FILTER: &str = "Saved games";

/// A Save or Load dialog.
pub struct FileDialog {
    inner: egui_file_dialog::FileDialog,
    save: bool,
}

impl std::fmt::Debug for FileDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileDialog")
            .field("save", &self.save)
            .finish_non_exhaustive()
    }
}

/// What the user did with the dialog this frame.
#[derive(Debug, PartialEq, Eq)]
pub enum FileAction {
    /// Still open.
    Open,
    /// Cancelled.
    Close,
    /// Save the game here.
    Save(PathBuf),
    /// Load the game saved here.
    Load(PathBuf),
}

/// The folder to start in before the user has picked one: Documents,
/// else home. (The working directory, the crate's default, is `/` for
/// an app started from the desktop.)
fn first_folder() -> Option<PathBuf> {
    let dirs = directories::UserDirs::new()?;
    Some(
        dirs.document_dir()
            .unwrap_or_else(|| dirs.home_dir())
            .to_path_buf(),
    )
}

impl FileDialog {
    fn new(storage: FileDialogStorage, title: &str) -> Self {
        let mut inner = egui_file_dialog::FileDialog::new()
            .id(egui::Id::new("file-dialog"))
            .title(title)
            .storage(storage)
            .add_file_filter_extensions(FILTER, vec!["sav"])
            .default_file_filter(FILTER)
            .add_save_extension("Saved game", "sav")
            .default_save_extension("Saved game")
            .default_size([620.0, 400.0])
            // Its search field is egui's near-black "dark canvas" in
            // every theme, so typed text is unreadable in light ones.
            .show_search(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO);
        let labels = inner.labels_mut();
        "Save".clone_into(&mut labels.save_button);
        "Open".clone_into(&mut labels.open_button);
        if let Some(folder) = first_folder() {
            inner = inner.initial_directory(folder);
        }
        Self { inner, save: false }
    }

    /// The Save dialog, suggesting a name for `game_name`'s file.
    /// Escape first leaves the name field, then closes it (the crate's
    /// rule: Escape doesn't close it while a widget has focus).
    #[must_use]
    pub fn save(storage: FileDialogStorage, game_name: &str) -> Self {
        let mut dialog = Self::new(storage, "Save game");
        dialog.inner = dialog
            .inner
            .default_file_name(&save_file::default_name(game_name));
        dialog.save = true;
        dialog.inner.save_file();
        dialog
    }

    /// The Load dialog.
    #[must_use]
    pub fn load(storage: FileDialogStorage) -> Self {
        let mut dialog = Self::new(storage, "Load game");
        dialog.inner.pick_file();
        dialog
    }

    /// Draws the dialog.
    pub fn show(&mut self, ctx: &egui::Context) -> FileAction {
        self.inner.update(ctx);
        match self.inner.state() {
            DialogState::Open => FileAction::Open,
            DialogState::Picked(path) if self.save => FileAction::Save(path.clone()),
            DialogState::Picked(path) => FileAction::Load(path.clone()),
            DialogState::PickedMultiple(_) | DialogState::Closed | DialogState::Cancelled => {
                FileAction::Close
            }
        }
    }

    /// What the dialog remembers (folders), to keep for the next one.
    pub fn storage(&mut self) -> FileDialogStorage {
        self.inner.storage_mut().clone()
    }
}
