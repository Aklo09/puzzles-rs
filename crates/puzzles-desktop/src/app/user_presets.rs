//! The user's presets file, `presets.toml` (user presets decision in
//! `docs/porting-plan.md`): per game, an array of presets, each a title
//! and a C parameter string, or a title and a submenu of presets.
//!
//! ```toml
//! [[flip]]
//! title = "Big"
//! params = "6x6c"
//!
//! [[flip]]
//! title = "Large"
//! presets = [
//!   { title = "8x8 random", params = "8x8r" },
//! ]
//! ```
//!
//! Games are keyed like preference files: the name in lower case without
//! spaces. Mistakes never stop the rest from loading: each is reported
//! with its line, and the entry is skipped.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use puzzles_core::preset::PresetMenu;
use serde::Deserialize;
use toml::Spanned;

/// The key a game has in the presets file and in file names (C's
/// `make_prefs_path`): its name in lower case, without spaces.
#[must_use]
pub fn game_key(name: &str) -> String {
    name.chars()
        .filter(|&c| c != ' ')
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    title: String,
    params: Option<String>,
    presets: Option<Vec<Spanned<Entry>>>,
}

/// A parsed presets file.
#[derive(Debug, Default)]
pub struct UserPresets {
    text: String,
    games: BTreeMap<String, Vec<Spanned<Entry>>>,
    /// Problems with the file as a whole.
    errors: Vec<String>,
}

impl UserPresets {
    /// Parses the file's text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        match toml::from_str::<BTreeMap<String, Vec<Spanned<Entry>>>>(text) {
            Ok(games) => Self {
                text: text.to_owned(),
                games,
                errors: Vec::new(),
            },
            Err(e) => Self {
                errors: vec![one_line(&format!("presets.toml: {e}"))],
                ..Self::default()
            },
        }
    }

    fn line(&self, offset: usize) -> usize {
        self.text[..offset.min(self.text.len())]
            .matches('\n')
            .count()
            + 1
    }

    /// The presets for `game` (its name), each parameter string checked
    /// with `check`, and the problems found: the file's own, entries
    /// that are neither a preset nor a submenu, and parameters `check`
    /// rejects. Games the file names that `known` says don't exist are
    /// reported too.
    pub fn menu_for(
        &self,
        game: &str,
        known: impl Fn(&str) -> bool,
        check: impl Fn(&str) -> Result<(), String>,
    ) -> (PresetMenu<String>, Vec<String>) {
        let mut errors = self.errors.clone();
        errors.extend(
            self.games
                .keys()
                .filter(|key| !known(key))
                .map(|key| format!("presets.toml: no game called \"{key}\"")),
        );
        let mut menu = PresetMenu::new();
        if let Some(entries) = self.games.get(&game_key(game)) {
            self.add(&mut menu, entries, &check, &mut errors);
        }
        (menu, errors)
    }

    fn add(
        &self,
        menu: &mut PresetMenu<String>,
        entries: &[Spanned<Entry>],
        check: &impl Fn(&str) -> Result<(), String>,
        errors: &mut Vec<String>,
    ) {
        for spanned in entries {
            let line = self.line(spanned.span().start);
            let entry = spanned.get_ref();
            let mut error = |message: String| {
                errors.push(format!(
                    "presets.toml line {line}: \"{}\": {message}",
                    entry.title
                ));
            };
            match (&entry.params, &entry.presets) {
                (Some(params), None) => match check(params) {
                    Ok(()) => menu.add_preset(entry.title.clone(), params.clone()),
                    Err(e) => error(format!("params \"{params}\": {e}")),
                },
                (None, Some(sub)) => {
                    let submenu = menu.add_submenu(entry.title.clone());
                    self.add(submenu, sub, check, errors);
                }
                _ => error("give either params or presets".to_owned()),
            }
        }
    }
}

/// The file's first error line only, for a menu.
fn one_line(message: &str) -> String {
    message
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the presets file was read, and its modification time then
/// (`None`: it didn't exist).
#[derive(Debug, PartialEq)]
enum Read {
    Never,
    At(Option<SystemTime>),
}

/// The presets file on disk, read again when it changes.
#[derive(Debug)]
pub struct PresetsFile {
    path: PathBuf,
    read: Read,
    presets: UserPresets,
}

impl PresetsFile {
    /// The file at `path`, not read yet.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            read: Read::Never,
            presets: UserPresets::default(),
        }
    }

    /// Reads the file again if it changed (or appeared, or went away)
    /// since the last call. Returns whether it changed.
    pub fn refresh(&mut self) -> bool {
        let modified = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        if self.read == Read::At(modified) {
            return false;
        }
        self.read = Read::At(modified);
        self.presets = match std::fs::read_to_string(&self.path) {
            Ok(text) => UserPresets::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => UserPresets::default(),
            Err(e) => UserPresets {
                errors: vec![format!("presets.toml: {e}")],
                ..UserPresets::default()
            },
        };
        true
    }

    /// The parsed presets.
    #[must_use]
    pub fn presets(&self) -> &UserPresets {
        &self.presets
    }
}

#[cfg(test)]
mod tests {
    use puzzles_core::preset::PresetEntry;

    use super::*;

    const FILE: &str = r#"
[[flip]]
title = "Big: 6 by 6"
params = "6x6c"

[[flip]]
title = "Odd"
presets = [
  { title = "Broken", params = "0x0" },
  { title = "9x9", params = "9x9" },
]

[[flip]]
title = "Neither"

[[flop]]
title = "Lost"
params = "3x3"

[[fifteen]]
title = "Wide"
params = "5x3"
"#;

    fn check(p: &str) -> Result<(), String> {
        (p != "0x0")
            .then_some(())
            .ok_or_else(|| "too small".to_owned())
    }

    fn known(key: &str) -> bool {
        key != "flop"
    }

    #[test]
    fn keys_are_lower_case_names_without_spaces() {
        assert_eq!(game_key("Null Game"), "nullgame");
        assert_eq!(game_key("Flip"), "flip");
    }

    #[test]
    fn presets_and_submenus_load_with_errors_by_line() {
        let file = UserPresets::parse(FILE);
        let (menu, errors) = file.menu_for("Flip", known, check);
        let titles: Vec<_> = menu
            .entries
            .iter()
            .map(|e| match e {
                PresetEntry::Preset { title, params } => format!("{title}={params}"),
                PresetEntry::Submenu { title, menu } => format!("{title}[{}]", menu.entries.len()),
            })
            .collect();
        assert_eq!(titles, ["Big: 6 by 6=6x6c", "Odd[1]"]);
        assert_eq!(
            errors,
            [
                "presets.toml: no game called \"flop\"",
                "presets.toml line 9: \"Broken\": params \"0x0\": too small",
                "presets.toml line 13: \"Neither\": give either params or presets",
            ]
        );
        let (other, _) = file.menu_for("Fifteen", known, check);
        assert_eq!(other.entries.len(), 1);
        assert!(file.menu_for("Guess", known, check).0.is_empty());
    }

    #[test]
    fn a_broken_file_reports_one_error_and_loads_nothing() {
        let file = UserPresets::parse("[[flip]\ntitle = 3");
        let (menu, errors) = file.menu_for("Flip", known, check);
        assert!(menu.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("presets.toml: "), "{errors:?}");
        assert!(!errors[0].contains('\n'));
        let typo = UserPresets::parse("[[flip]]\ntitle = \"x\"\nparam = \"3x3\"\n");
        assert_eq!(
            typo.menu_for("Flip", known, check).1.len(),
            1,
            "unknown keys are errors"
        );
    }

    #[test]
    fn the_file_is_read_again_only_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("puzzles-presets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("presets.toml");
        let _ = std::fs::remove_file(&path);
        let mut file = PresetsFile::new(path.clone());
        file.refresh();
        assert!(
            file.presets().menu_for("Flip", known, check).0.is_empty(),
            "no file yet"
        );
        std::fs::write(&path, "[[flip]]\ntitle = \"A\"\nparams = \"3x3\"\n").unwrap();
        assert!(file.refresh());
        assert_eq!(
            file.presets()
                .menu_for("Flip", known, check)
                .0
                .entries
                .len(),
            1
        );
        assert!(!file.refresh(), "unchanged");
        std::fs::remove_file(&path).unwrap();
        assert!(file.refresh(), "gone");
        assert!(file.presets().menu_for("Flip", known, check).0.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
