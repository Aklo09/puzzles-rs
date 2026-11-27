//! The commands of `puzzles-cli`, as functions (the binary parses
//! arguments and prints). See the CLI decisions in
//! `docs/porting-plan.md`.
//!
//! * [`generate`]: descriptive game IDs from a non-interactive mid-end.
//! * [`solve`]: the solved position as text, or the solve move.
//! * [`roundtrip`]: C's `fuzzpuzz`: load a save file, size, redraw, save.

use std::fmt;

use puzzles_core::drawing::NullDrawing;
use puzzles_core::erased::Puzzle;
use puzzles_core::midend::{MidendOptions, identify_game};
use puzzles_games::{GAMES, GameInfo};

/// A command's failure, with the message to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn error(message: impl Into<String>) -> Error {
    Error(message.into())
}

/// The game called `name`: its name or its key (`flip`, `Null Game` or
/// `nullgame`), in any case.
///
/// # Errors
/// An unknown name; the message lists the games.
pub fn find_game(name: &str) -> Result<&'static GameInfo, Error> {
    let key = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    GAMES
        .iter()
        .find(|g| key(g.name) == key(name))
        .ok_or_else(|| {
            let names: Vec<&str> = GAMES.iter().map(|g| g.name).collect();
            error(format!(
                "unknown game {name:?}; games: {}",
                names.join(", ")
            ))
        })
}

/// A mid-end for `game` with the options C reads from the environment
/// (`<GAME>_DEFAULT`, ...).
fn midend(game: &GameInfo, seed: &[u8], interactive: bool) -> Box<dyn Puzzle> {
    let options = MidendOptions::from_lookup(game.name, |name| std::env::var(name).ok());
    game.new_midend(seed, interactive, options)
}

/// Sets a game ID, with the command's error message.
fn set_id(me: &mut dyn Puzzle, id: &str) -> Result<(), Error> {
    me.game_id(id)
        .map_err(|e| error(format!("{}: invalid game ID {id:?}: {e}", me.name())))
}

/// `n` descriptive game IDs (`params:desc`). Without `id`, or with bare
/// parameters, each game comes from a fresh random seed, drawn from
/// `seed`; a seed ID (`params#seed`) or descriptive ID gives exactly
/// one game.
///
/// # Errors
/// An invalid ID, or more than one game asked of a seed or description.
///
/// # Panics
/// Never: a mid-end has a game ID after New Game.
pub fn generate(
    game: &GameInfo,
    id: Option<&str>,
    n: usize,
    seed: &[u8],
) -> Result<Vec<String>, Error> {
    let mut me = midend(game, seed, false);
    if let Some(id) = id {
        if n != 1 && id.contains([':', '#']) {
            return Err(error(format!(
                "{id:?} names one game; -n needs bare parameters or no ID"
            )));
        }
        set_id(me.as_mut(), id)?;
    }
    Ok((0..n)
        .map(|_| {
            me.new_game();
            me.get_game_id().expect("a game exists after New Game")
        })
        .collect())
}

/// Solves the game `id` names. Returns the solved position as text if
/// the game formats positions as text (and `move_only` is false),
/// otherwise the solve move, as in save files.
///
/// # Errors
/// An invalid ID; a game without a solver; no solution.
///
/// # Panics
/// Never: a successful solve is a move.
pub fn solve(game: &GameInfo, id: &str, move_only: bool) -> Result<String, Error> {
    let mut me = midend(game, b"solve", false);
    set_id(me.as_mut(), id)?;
    me.new_game();
    me.solve()
        .map_err(|e| error(format!("{}: {id}: {e}", game.name)))?;
    let text = if move_only { None } else { me.text_format() };
    Ok(text.unwrap_or_else(|| {
        let mv = me.current_move().expect("solving made a move");
        format!("{mv}\n")
    }))
}

/// C's `fuzzpuzz`: loads a save file of any game, sizes it as large as
/// it goes, redraws it with a null drawing, and writes it back.
///
/// # Errors
/// C's messages: not a save file, a game not built in ("Game not
/// recognised"), or the mid-end's loading errors.
pub fn roundtrip(input: &[u8]) -> Result<Vec<u8>, Error> {
    let name = identify_game(&mut &input[..]).map_err(|e| error(e.to_string()))?;
    let game = GAMES
        .iter()
        .find(|g| g.name == name)
        .ok_or_else(|| error("Game not recognised"))?;
    let mut me = midend(game, b"roundtrip", true);
    me.deserialise(&mut &input[..])
        .map_err(|e| error(e.to_string()))?;
    me.size(i32::MAX, i32::MAX, false, 1.0);
    me.redraw(&mut NullDrawing);
    let mut out = Vec::new();
    me.serialise(&mut out).map_err(|e| error(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_games_by_name_or_key() {
        assert_eq!(find_game("FLIP").unwrap().name, "Flip");
        let err = find_game("flop").unwrap_err().0;
        assert!(err.contains("games: ") && err.contains("Flip"), "{err}");
    }

    #[test]
    fn generates_known_ids() {
        let flip = find_game("flip").unwrap();
        assert_eq!(
            generate(flip, Some("3x3c#42"), 1, b"x").unwrap(),
            ["3x3:d074191345d1644c17058,f80"]
        );
        let ids = generate(flip, Some("4x4r"), 3, b"x").unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|id| id.starts_with("4x4:")));
        assert_ne!(ids[0], ids[1]);
        assert!(generate(flip, Some("3x3#1"), 2, b"x").is_err());
        assert_eq!(
            generate(flip, Some("0x3"), 1, b"x").unwrap_err().0,
            "Flip: invalid game ID \"0x3\": Width and height must both be greater than zero"
        );
    }

    #[test]
    fn solves_to_text_or_move() {
        let flip = find_game("flip").unwrap();
        let id = "3x3:d074191345d1644c17058,f80";
        assert_eq!(solve(flip, id, true).unwrap(), "S011011001\n");
        assert!(solve(flip, id, false).unwrap().starts_with("+---+"));
        assert!(
            solve(flip, "2x1:a,4", true)
                .unwrap_err()
                .0
                .contains("No solution exists")
        );
    }

    #[test]
    fn roundtrips_save_files() {
        let flip = find_game("flip").unwrap();
        let mut me = flip.new_midend(b"rt", true, MidendOptions::default());
        me.new_game();
        me.solve().unwrap();
        let mut save = Vec::new();
        me.serialise(&mut save).unwrap();
        assert_eq!(roundtrip(&save).unwrap(), save);
        assert_eq!(
            roundtrip(b"nonsense").unwrap_err().0,
            "Data does not appear to be a saved game file"
        );
        let other = String::from_utf8(save)
            .unwrap()
            .replace("GAME    :4:Flip", "GAME    :4:Flop");
        assert_eq!(
            roundtrip(other.as_bytes()).unwrap_err().0,
            "Game not recognised"
        );
    }
}
