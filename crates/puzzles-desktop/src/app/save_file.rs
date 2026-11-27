//! Saved games opened by the user: from the Load dialog, dropped on the
//! window or named on the command line. A save file is C's format
//! (`midend_serialise`); its `GAME` line says which game opens it.

use std::io::Read;
use std::path::Path;

use puzzles_core::midend::identify_game;
use puzzles_games::GameInfo;

use super::user_presets::game_key;

/// The largest file read as a saved game. Real ones are a few
/// kilobytes; this stops a stray drop of a huge file from filling memory.
pub const MAX_LEN: u64 = 16 << 20;

/// A saved game and the game it is for.
#[derive(Debug)]
pub struct SaveFile {
    /// The game that opens it.
    pub game: &'static GameInfo,
    /// The file's contents.
    pub data: Vec<u8>,
}

impl SaveFile {
    /// Reads a saved game from `path`.
    ///
    /// # Errors
    /// A message for the user: the file can't be read, is too large, isn't
    /// a saved game, or is for a game the app doesn't have.
    pub fn read(path: &Path) -> Result<Self, String> {
        let cant = |e: std::io::Error| format!("Can't read {}: {e}", path.display());
        let file = std::fs::File::open(path).map_err(cant)?;
        let mut data = Vec::new();
        file.take(MAX_LEN + 1)
            .read_to_end(&mut data)
            .map_err(cant)?;
        if data.len() as u64 > MAX_LEN {
            return Err(format!(
                "{} is too large to be a saved game.",
                path.display()
            ));
        }
        Self::parse(data).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// A saved game from its contents.
    ///
    /// # Errors
    /// C's message if it isn't a saved game; a message if the app doesn't
    /// have its game.
    pub fn parse(data: Vec<u8>) -> Result<Self, String> {
        if data.len() as u64 > MAX_LEN {
            return Err("Too large to be a saved game.".to_owned());
        }
        let name = identify_game(&mut data.as_slice()).map_err(|e| e.to_string())?;
        let game = puzzles_games::find(&name)
            .ok_or_else(|| format!("This is a saved game of {name}, which isn't here."))?;
        Ok(Self { game, data })
    }
}

/// The file name the Save dialog suggests for a game.
#[must_use]
pub fn default_name(game_name: &str) -> String {
    format!("{}.sav", game_key(game_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLIP: &str = "SAVEFILE:41:Simon Tatham's Portable Puzzle Collection\n\
                        VERSION :1:1\nGAME    :4:Flip\n";

    #[test]
    fn the_game_line_picks_the_game() {
        let file = SaveFile::parse(FLIP.as_bytes().to_vec()).unwrap();
        assert_eq!(file.game.name, "Flip");
        assert_eq!(default_name("Flip"), "flip.sav");
    }

    #[test]
    fn other_files_are_refused_with_a_reason() {
        let not_a_save = SaveFile::parse(b"hello".to_vec()).unwrap_err();
        assert!(not_a_save.contains("saved game"), "{not_a_save}");
        let other = FLIP.replace("GAME    :4:Flip", "GAME    :5:Pearl");
        assert_eq!(
            SaveFile::parse(other.into_bytes()).unwrap_err(),
            "This is a saved game of Pearl, which isn't here."
        );
        assert!(SaveFile::parse(Vec::new()).is_err());
    }

    #[test]
    fn files_are_read_from_disk_with_a_size_limit() {
        let dir = std::env::temp_dir().join(format!("puzzles-savefile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flip.sav");
        std::fs::write(&path, FLIP).unwrap();
        assert_eq!(SaveFile::read(&path).unwrap().game.name, "Flip");

        let big = dir.join("big.sav");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_LEN + 1).unwrap();
        assert!(SaveFile::read(&big).unwrap_err().contains("too large"));
        assert!(
            SaveFile::read(&dir.join("none.sav"))
                .unwrap_err()
                .starts_with("Can't read")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
