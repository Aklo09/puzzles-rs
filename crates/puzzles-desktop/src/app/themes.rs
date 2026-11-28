//! The built-in themes plus the user's, `themes/*.toml` in the app's
//! folder (themes decision in `docs/porting-plan.md`; how to write one:
//! `docs/themes.md`). A broken file is skipped and reported; a file can't
//! replace a built-in theme. Keys in a file's game sections that colour
//! nothing (an unknown game or colour) are reported, and the theme is
//! used without them.

use std::path::{Path, PathBuf};

use puzzles_core::palette::PaletteEntry;
use puzzles_theme::{Theme, ThemeSet, unused_game_keys};

/// All themes, and problems with the user's files.
#[derive(Debug, Clone)]
pub struct Themes {
    set: ThemeSet,
    errors: Vec<String>,
}

/// Names of the built-in themes, in menu order.
pub const BUILT_IN: [&str; 3] = ["Classic", "Light", "Dark"];

impl Themes {
    /// The built-in themes and those in `dir` (if any).
    #[must_use]
    pub fn load(dir: Option<&Path>) -> Self {
        let mut set = ThemeSet::builtin();
        let mut errors = Vec::new();
        let mut files: Vec<PathBuf> = dir
            .and_then(|d| std::fs::read_dir(d).ok())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        for path in files {
            let file = path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let theme = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| Theme::from_toml(&text).map_err(|e| one_line(&e.to_string())));
            match theme {
                Ok(theme) if BUILT_IN.iter().any(|b| b.eq_ignore_ascii_case(&theme.name)) => {
                    errors.push(format!(
                        "{file}: \"{}\" is a built-in theme's name",
                        theme.name
                    ));
                }
                Ok(theme) => {
                    let games: Vec<(&str, &[PaletteEntry])> = puzzles_games::GAMES
                        .iter()
                        .map(|g| (g.name, g.palette))
                        .collect();
                    for unused in unused_game_keys(&theme, &games) {
                        errors.push(format!("{file}: {unused}"));
                    }
                    set.insert(theme);
                }
                Err(e) => errors.push(format!("{file}: {e}")),
            }
        }
        Self { set, errors }
    }

    /// The themes.
    #[must_use]
    pub fn set(&self) -> &ThemeSet {
        &self.set
    }

    /// Problems with the user's theme files: files that weren't loaded,
    /// and keys in loaded ones that colour nothing.
    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Theme names for a menu: the built-in ones, then the user's
    /// sorted; only light or only dark ones if `dark` says so.
    #[must_use]
    pub fn names(&self, dark: Option<bool>) -> Vec<String> {
        let mut user: Vec<String> = self
            .set
            .names()
            .into_iter()
            .filter(|n| !BUILT_IN.iter().any(|b| b.eq_ignore_ascii_case(n)))
            .collect();
        user.sort_by_key(|n| n.to_lowercase());
        BUILT_IN
            .iter()
            .map(|&n| n.to_owned())
            .chain(user)
            .filter(|n| dark.is_none_or(|d| self.set.is_dark(n) == d))
            .collect()
    }
}

/// An error's first line or two, for a dialog.
fn one_line(message: &str) -> String {
    message
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_themes_load_beside_the_built_in_ones() {
        let dir = std::env::temp_dir().join(format!("puzzles-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("b.toml"),
            "name = \"Sepia\"\nextends = \"Light\"\n[roles]\nbackground = \"#f4ecd8\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("a.toml"),
            "name = \"Night\"\nextends = \"Dark\"\ndark = true\n",
        )
        .unwrap();
        std::fs::write(dir.join("c.toml"), "name = \"Dark\"\n").unwrap();
        std::fs::write(
            dir.join("d.toml"),
            "name = \"Bad\"\n[roles]\nnope = \"red\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("e.toml"),
            "name = \"Typo\"\n[game.flip.overrides]\ndone = \"red\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "not a theme").unwrap();
        let themes = Themes::load(Some(&dir));
        assert_eq!(
            themes.names(None),
            ["Classic", "Light", "Dark", "Night", "Sepia", "Typo"]
        );
        assert_eq!(themes.names(Some(true)), ["Dark", "Night"]);
        assert_eq!(
            themes.names(Some(false)),
            ["Classic", "Light", "Sepia", "Typo"]
        );
        assert_eq!(themes.errors().len(), 3, "{:?}", themes.errors());
        assert!(themes.errors()[0].starts_with("c.toml: \"Dark\" is a built-in"));
        assert!(themes.errors()[1].starts_with("d.toml: "));
        // Loaded, with the key that colours nothing reported.
        assert!(
            themes.errors()[2]
                .starts_with("e.toml: game.flip.overrides.done: Flip has no colour called that"),
            "{:?}",
            themes.errors()
        );
        assert_eq!(Themes::load(None).errors(), &[] as &[String]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
