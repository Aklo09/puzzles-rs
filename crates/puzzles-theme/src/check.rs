//! Keys in a theme's `[game.<name>]` sections that colour nothing.
//!
//! A theme file can't know which games exist or what their palettes
//! hold, so reading it accepts any key there. This check, given the
//! games, finds the keys that match nothing: a game that doesn't exist,
//! an override naming no palette entry, or a game colour the game doesn't
//! have. Such keys are ignored when resolving, which silently leaves the
//! colour as it was; front ends report them instead.

use std::fmt::{self, Write as _};

use puzzles_core::palette::{ColourSpec, PaletteEntry};

use crate::model::{Theme, game_key};

/// A key in a theme that colours nothing, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnusedKey {
    /// Where, as written in the file: `game.mosaic.overrides.done`.
    pub path: String,
    /// Why it colours nothing, and what would.
    pub reason: String,
}

impl fmt::Display for UnusedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

/// The keys in `theme`'s own `[game.*]` sections (not those of the
/// themes it extends) that match nothing in `games`, given as each game's
/// name (`Game::NAME`) and palette. Sections without keys are ignored.
#[must_use]
pub fn unused_game_keys(theme: &Theme, games: &[(&str, &[PaletteEntry])]) -> Vec<UnusedKey> {
    let mut unused = Vec::new();
    for (key, colours) in &theme.games {
        if colours.specific.is_empty() && colours.overrides.is_empty() {
            continue;
        }
        let Some(&(name, palette)) = games.iter().find(|(name, _)| game_key(name) == *key) else {
            let mut known: Vec<String> = games.iter().map(|(name, _)| game_key(name)).collect();
            known.sort();
            unused.push(UnusedKey {
                path: format!("game.{key}"),
                reason: format!("no game is called that (games: {})", known.join(", ")),
            });
            continue;
        };
        let entries: Vec<String> = palette.iter().map(PaletteEntry::key).collect();
        let own: Vec<&str> = palette
            .iter()
            .filter_map(|e| match e.spec {
                ColourSpec::Specific(k) => Some(k),
                _ => None,
            })
            .collect();

        for colour in colours.specific.keys() {
            if own.contains(&colour.as_str()) {
                continue;
            }
            let mut reason = if own.is_empty() {
                format!("{name} has no colours of its own")
            } else {
                format!(
                    "{name} has no colour of its own called that (its own: {})",
                    own.join(", ")
                )
            };
            if entries.contains(colour) {
                let _ = write!(
                    reason,
                    "; to replace that colour, put it under [game.{key}.overrides]"
                );
            }
            unused.push(UnusedKey {
                path: format!("game.{key}.{colour}"),
                reason,
            });
        }

        for colour in colours.overrides.keys() {
            if !entries.contains(colour) {
                unused.push(UnusedKey {
                    path: format!("game.{key}.overrides.{colour}"),
                    reason: format!(
                        "{name} has no colour called that (its colours: {})",
                        entries.join(", ")
                    ),
                });
            }
        }
    }
    unused
}

#[cfg(test)]
mod tests {
    use super::*;

    puzzles_core::palette! {
        enum Col {
            Background => Role(Background),
            TextSolved => Role(Done),
            Unmarked => Specific("unmarked"),
        }
    }

    const GAMES: &[(&str, &[PaletteEntry])] = &[("Mosaic", Col::PALETTE), ("Flip", &[])];

    fn check(toml: &str) -> Vec<String> {
        let theme = Theme::from_toml(toml).expect("a theme");
        unused_game_keys(&theme, GAMES)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn keys_that_match_are_fine() {
        assert_eq!(
            check(
                "name = \"T\"\n[game.mosaic]\nunmarked = \"red\"\n\
                 [game.mosaic.overrides]\ntext_solved = \"red\"\nbackground = \"blue\"\n\
                 [game.nothing]\n"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_role_name_as_override_is_reported_with_the_entries() {
        assert_eq!(
            check("name = \"T\"\n[game.mosaic.overrides]\ndone = \"yellow\"\n"),
            [
                "game.mosaic.overrides.done: Mosaic has no colour called that \
                 (its colours: background, text_solved, unmarked)"
            ]
        );
    }

    #[test]
    fn an_entry_outside_overrides_is_pointed_there() {
        assert_eq!(
            check("name = \"T\"\n[game.mosaic]\ntext_solved = \"yellow\"\n"),
            [
                "game.mosaic.text_solved: Mosaic has no colour of its own called that \
                 (its own: unmarked); to replace that colour, put it under \
                 [game.mosaic.overrides]"
            ]
        );
        assert_eq!(
            check("name = \"T\"\n[game.flip]\nright = \"white\"\n"),
            ["game.flip.right: Flip has no colours of its own"]
        );
    }

    #[test]
    fn unknown_games_are_reported() {
        assert_eq!(
            check("name = \"T\"\n[game.mosiac]\nunmarked = \"red\"\n"),
            ["game.mosiac: no game is called that (games: flip, mosaic)"]
        );
    }
}
