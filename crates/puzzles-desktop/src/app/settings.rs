//! The app's settings, `settings.toml` in the app's folder (themes
//! decision in `docs/porting-plan.md`): the theme choices and the game
//! last open. The window's place is eframe's, in `app.ron`. Unknown keys are ignored and missing
//! ones take their defaults, so older and newer files both load.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::paths::write_atomically;

/// What the user chose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Follow the system's light or dark appearance.
    pub follow_system: bool,
    /// The theme when not following the system.
    pub theme: String,
    /// The theme for a light system appearance.
    pub light_theme: String,
    /// The theme for a dark system appearance.
    pub dark_theme: String,
    /// The game open when the app last ran, reopened at launch; `None`
    /// for the game list.
    pub last_game: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            follow_system: true,
            theme: "Light".to_owned(),
            light_theme: "Light".to_owned(),
            dark_theme: "Dark".to_owned(),
            last_game: None,
        }
    }
}

impl Settings {
    /// The theme to use, given whether the system is dark (`None`: the
    /// system doesn't say, taken as light).
    #[must_use]
    pub fn theme_for(&self, system_dark: Option<bool>) -> &str {
        match (self.follow_system, system_dark) {
            (false, _) => &self.theme,
            (true, Some(true)) => &self.dark_theme,
            (true, _) => &self.light_theme,
        }
    }

    /// Reads the file, or the defaults if it is missing. A broken file
    /// gives the defaults and the reason.
    #[must_use]
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(settings) => (settings, None),
                Err(e) => (
                    Self::default(),
                    Some(format!("{}: {}", path.display(), e.message())),
                ),
            },
            Err(_) => (Self::default(), None),
        }
    }

    /// Writes the file.
    ///
    /// # Errors
    /// Writing it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        write_atomically(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_system_picks_by_appearance() {
        let s = Settings::default();
        assert_eq!(s.theme_for(Some(true)), "Dark");
        assert_eq!(s.theme_for(Some(false)), "Light");
        assert_eq!(s.theme_for(None), "Light");
        let fixed = Settings {
            follow_system: false,
            theme: "Classic".to_owned(),
            ..Settings::default()
        };
        assert_eq!(fixed.theme_for(Some(true)), "Classic");
    }

    #[test]
    fn files_round_trip_and_tolerate_change() {
        let dir = std::env::temp_dir().join(format!("puzzles-settings-{}", std::process::id()));
        let path = dir.join("settings.toml");
        let mine = Settings {
            follow_system: false,
            theme: "Classic".to_owned(),
            last_game: Some("Flip".to_owned()),
            ..Settings::default()
        };
        mine.save(&path).unwrap();
        assert_eq!(Settings::load(&path), (mine, None));
        let library = Settings::default();
        library.save(&path).unwrap();
        assert_eq!(Settings::load(&path).0.last_game, None);

        // A later version's keys are ignored, missing ones defaulted.
        std::fs::write(
            &path,
            "theme = \"Dark\"\nfollow_system = false\nwindow = 3\n",
        )
        .unwrap();
        let (read, error) = Settings::load(&path);
        assert_eq!(
            (read.theme.as_str(), read.dark_theme.as_str(), error),
            ("Dark", "Dark", None)
        );

        std::fs::write(&path, "theme = 3").unwrap();
        let (read, error) = Settings::load(&path);
        assert_eq!(read, Settings::default());
        assert!(error.is_some_and(|e| e.contains("settings.toml")));
        assert_eq!(
            Settings::load(&dir.join("none.toml")),
            (Settings::default(), None)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
