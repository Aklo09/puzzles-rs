//! Where the app keeps its files (dialogs and presets decision in
//! `docs/porting-plan.md`): one folder per OS, `puzzles-rs` in the
//! system's configuration folder.
//!
//! * macOS: `~/Library/Application Support/puzzles-rs/`
//! * Linux: `~/.config/puzzles-rs/` (or `$XDG_CONFIG_HOME`)
//! * Windows: `%APPDATA%\puzzles-rs\`
//!
//! Inside: `settings.toml`, `presets.toml` and `themes/*.toml`
//! (hand-edited), and `prefs/<game>.conf` (C's preference file format,
//! named as C names them).

use std::io::Write;
use std::path::{Path, PathBuf};

use super::user_presets::game_key;

/// The app's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppDirs {
    config: PathBuf,
}

impl AppDirs {
    /// The system's folder for this user, if the OS reports one.
    #[must_use]
    pub fn detect() -> Option<Self> {
        directories::BaseDirs::new().map(|d| Self::at(d.config_dir().join("puzzles-rs")))
    }

    /// A folder chosen by the caller (tests).
    #[must_use]
    pub fn at(config: PathBuf) -> Self {
        Self { config }
    }

    /// The user's presets file.
    #[must_use]
    pub fn presets(&self) -> PathBuf {
        self.config.join("presets.toml")
    }

    /// The app's settings.
    #[must_use]
    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    /// The folder of the user's themes.
    #[must_use]
    pub fn themes(&self) -> PathBuf {
        self.config.join("themes")
    }

    /// A game's preference file (C's `make_prefs_path` naming).
    #[must_use]
    pub fn prefs(&self, game_name: &str) -> PathBuf {
        self.config
            .join("prefs")
            .join(format!("{}.conf", game_key(game_name)))
    }
}

/// Writes `data` to `path` through a temporary file and a rename, so a
/// crash never leaves half a file (as C's GTK front end does), creating
/// the folder if needed.
///
/// # Errors
/// Creating the folder, writing or renaming.
pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_named_like_c() {
        let dirs = AppDirs::at(PathBuf::from("base"));
        assert_eq!(dirs.presets(), Path::new("base").join("presets.toml"));
        assert_eq!(
            dirs.prefs("Null Game"),
            Path::new("base").join("prefs").join("nullgame.conf")
        );
    }

    #[test]
    fn writes_replace_the_file_whole() {
        let dir = std::env::temp_dir().join(format!("puzzles-paths-{}", std::process::id()));
        let path = dir.join("prefs").join("flip.conf");
        write_atomically(&path, b"one").unwrap();
        write_atomically(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!dir.join("prefs").join("flip.conf.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
