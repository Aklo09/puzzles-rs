//! Save files (C's `midend_serialise`, `midend_deserialise` and
//! `identify_game`). The format is C's, byte for byte: lines of an
//! 8-character key padded with spaces, `:`, the value's length in bytes,
//! `:`, the value and a newline. Loading checks the same things in the
//! same order as C and fails with C's messages.
//!
//! C reads values as C strings, so a NUL byte ends a value early; this
//! port does the same.

use std::io::{self, Read, Write};

use super::{Entry, Midend, Snapshot, Step};
use crate::game::{Game, GameCtx};
use crate::misc::{bytes_to_hex, deobfuscate_bitmap, hex_to_bytes, obfuscate_bitmap};
use crate::parse::{atof, atoi, format_g};

const MAGIC: &str = "Simon Tatham's Portable Puzzle Collection";
const VERSION: &str = "1";

const NOT_A_SAVE: &str = "Data does not appear to be a saved game file";
const ENDED: &str = "Saved data ended unexpectedly";
const BADLY_FORMATTED: &str = "Data was incorrectly formatted for a saved game file";

/// Why a save file (or preferences file) couldn't be loaded.
#[derive(Debug, thiserror::Error)]
pub enum DeserialiseError {
    /// The data is wrong; the message is C's.
    #[error("{0}")]
    Invalid(&'static str),
    /// Reading failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn write_line(w: &mut impl Write, key: &str, value: &[u8]) -> io::Result<()> {
    write!(w, "{key:<8}:{}:", value.len())?;
    w.write_all(value)?;
    w.write_all(b"\n")
}

/// A value as C sees it: up to the first NUL byte.
pub(super) fn c_str(mut value: Vec<u8>) -> Vec<u8> {
    if let Some(nul) = value.iter().position(|&b| b == 0) {
        value.truncate(nul);
    }
    value
}

fn printable(value: &[u8]) -> bool {
    value.iter().all(|&b| (32..127).contains(&b))
}

/// Reads one byte, or `None` at the end.
pub(super) fn read_byte(r: &mut impl Read) -> io::Result<Option<u8>> {
    let mut b = [0];
    loop {
        match r.read(&mut b) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(b[0])),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Reads one `KEY:len:value` record. `fail` is the message for a file
/// that ends early (or for any problem before the first record, which
/// C reports the same way); after the first record a malformed line
/// has a message of its own.
fn read_record(
    r: &mut impl Read,
    started: bool,
    fail: &'static str,
) -> Result<(String, Vec<u8>), DeserialiseError> {
    let eof = || DeserialiseError::Invalid(fail);
    let malformed = || DeserialiseError::Invalid(if started { BADLY_FORMATTED } else { fail });

    let mut key = [0_u8; 9];
    loop {
        key[0] = read_byte(r)?.ok_or_else(eof)?;
        if key[0] != b'\r' && key[0] != b'\n' {
            break;
        }
    }
    for b in &mut key[1..] {
        *b = read_byte(r)?.ok_or_else(eof)?;
    }
    if key[8] != b':' {
        return Err(malformed());
    }
    let key_len = key
        .iter()
        .position(|&b| matches!(b, b':' | b' ' | 0))
        .unwrap_or(8);
    let key = String::from_utf8_lossy(&key[..key_len]).into_owned();

    let mut len: i32 = 0;
    loop {
        let c = read_byte(r)?.ok_or_else(eof)?;
        if c == b':' {
            break;
        } else if c.is_ascii_digit() && len < (i32::MAX - 10) / 10 {
            len = len * 10 + i32::from(c - b'0');
        } else {
            return Err(malformed());
        }
    }

    let mut value = Vec::new();
    r.take(u64::try_from(len).expect("non-negative"))
        .read_to_end(&mut value)?;
    if value.len() < len as usize {
        return Err(eof());
    }
    Ok((key, c_str(value)))
}

/// Which game a save file is for (C's `identify_game`): its `GAME`
/// value.
///
/// # Errors
/// C's message if the data isn't a save file; reading errors.
pub fn identify_game(r: &mut impl Read) -> Result<String, DeserialiseError> {
    let mut started = false;
    let mut fail = NOT_A_SAVE;
    loop {
        let (key, value) = read_record(r, started, fail)?;
        if started {
            if key == "VERSION" && value != VERSION.as_bytes() {
                return Err(DeserialiseError::Invalid(
                    "Cannot handle this version of the saved game file format",
                ));
            } else if key == "GAME" {
                return Ok(String::from_utf8_lossy(&value).into_owned());
            }
        } else {
            if key != "SAVEFILE" || value != MAGIC.as_bytes() {
                return Err(DeserialiseError::Invalid(fail));
            }
            fail = ENDED;
            started = true;
        }
    }
}

/// A move line of a save file, replayed once the whole file is read.
enum SavedStep {
    Move(String),
    Solve(String),
    Restart(String),
}

impl<G: Game> Midend<G> {
    /// Writes the game as a save file (C's `midend_serialise`).
    ///
    /// # Errors
    /// Writing errors.
    ///
    /// # Panics
    /// If there is no game yet.
    pub fn serialise(&self, w: &mut impl Write) -> io::Result<()> {
        assert!(
            (1..=self.states.len()).contains(&self.statepos),
            "serialise needs a game"
        );
        write_line(w, "SAVEFILE", MAGIC.as_bytes())?;
        write_line(w, "VERSION", VERSION.as_bytes())?;
        write_line(w, "GAME", G::NAME.as_bytes())?;
        write_line(w, "PARAMS", G::encode_params(&self.params, true).as_bytes())?;
        if let Some(curparams) = &self.curparams {
            write_line(w, "CPARAMS", G::encode_params(curparams, true).as_bytes())?;
        }
        if let Some(seed) = &self.seed {
            // Seeds needn't be printable; hex-encode those that aren't.
            if printable(seed) {
                write_line(w, "SEED", seed)?;
            } else {
                write_line(w, "HEXSEED", bytes_to_hex(seed).as_bytes())?;
            }
        }
        if let Some(desc) = &self.desc {
            write_line(w, "DESC", desc.as_bytes())?;
        }
        if let Some(privdesc) = &self.privdesc {
            write_line(w, "PRIVDESC", privdesc.as_bytes())?;
        }
        if let Some(aux) = &self.aux {
            // Obfuscated, so a glance at the file doesn't spoil the
            // solution.
            let mut bytes = aux.as_bytes().to_vec();
            obfuscate_bitmap(&mut bytes, aux.len() * 8);
            write_line(w, "AUXINFO", bytes_to_hex(&bytes).as_bytes())?;
        }
        if let Some(ui) = self.ui.as_ref().and_then(G::encode_ui) {
            write_line(w, "UI", ui.as_bytes())?;
        }
        if G::IS_TIMED {
            write_line(w, "TIME", format_g(f64::from(self.elapsed)).as_bytes())?;
        }
        write_line(w, "NSTATES", self.states.len().to_string().as_bytes())?;
        write_line(w, "STATEPOS", self.statepos.to_string().as_bytes())?;
        for entry in &self.states[1..] {
            let mut text = String::new();
            let key = match &entry.step {
                Step::Move(mv) => {
                    G::encode_move(mv, &mut text);
                    "MOVE"
                }
                Step::Solve(mv) => {
                    G::encode_move(mv, &mut text);
                    "SOLVE"
                }
                Step::Restart(desc) => {
                    text.clone_from(desc);
                    "RESTART"
                }
                Step::NewGame => unreachable!("only the first entry starts a game"),
            };
            write_line(w, key, text.as_bytes())?;
        }
        Ok(())
    }

    /// Replaces the game with a save file's (C's `midend_deserialise`).
    /// Nothing changes if the file is rejected.
    ///
    /// Requests from the game while replaying (Mines superseding its
    /// description) are ignored: a saved game replays from its private
    /// description, which never needs one.
    ///
    /// # Errors
    /// C's message for the first problem C would find; reading errors.
    ///
    /// # Panics
    /// Never on any input: values are checked before they are used.
    pub fn deserialise(&mut self, r: &mut impl Read) -> Result<(), DeserialiseError> {
        let invalid = DeserialiseError::Invalid;
        let mut started = false;
        let mut fail = NOT_A_SAVE;

        let (mut seed, mut parstr, mut cparstr) = (None, None, None);
        let (mut desc, mut privdesc, mut aux, mut uistr) = (None, None, None, None);
        let mut elapsed = 0.0_f32;
        let mut nstates: i32 = 0;
        let mut statepos: i32 = -1;
        let mut steps: Vec<SavedStep> = Vec::new();
        let mut have_states = false;

        let text = |v: Vec<u8>| String::from_utf8(v).expect("checked printable");
        while nstates <= 0 || statepos < 0 || (steps.len() as i64) < i64::from(nstates) - 1 {
            let (key, value) = read_record(r, started, fail)?;
            if key != "SEED" && !printable(&value) {
                return Err(invalid("Forbidden characters in saved game file"));
            }
            if !started {
                if key != "SAVEFILE" || value != MAGIC.as_bytes() {
                    return Err(invalid(fail));
                }
                fail = ENDED;
                started = true;
                continue;
            }
            match key.as_str() {
                "VERSION" if value != VERSION.as_bytes() => {
                    return Err(invalid(
                        "Cannot handle this version of the saved game file format",
                    ));
                }
                "GAME" if value != G::NAME.as_bytes() => {
                    return Err(invalid("Save file is from a different game"));
                }
                "PARAMS" => parstr = Some(text(value)),
                "CPARAMS" => cparstr = Some(text(value)),
                "HEXSEED" => {
                    let bytes =
                        hex_to_bytes(&text(value.clone()), value.len() / 2).unwrap_or_default();
                    seed = Some(c_str(bytes));
                }
                "SEED" => seed = Some(value),
                "DESC" => desc = Some(text(value)),
                "PRIVDESC" => privdesc = Some(text(value)),
                "AUXINFO" => {
                    let len = value.len() / 2;
                    let mut bytes = hex_to_bytes(&text(value), len).unwrap_or_default();
                    deobfuscate_bitmap(&mut bytes, len * 8);
                    aux = Some(String::from_utf8_lossy(&c_str(bytes)).into_owned());
                }
                "UI" => uistr = Some(text(value)),
                "TIME" => elapsed = atof(&value) as f32,
                "NSTATES" => {
                    if have_states {
                        return Err(invalid("Two state counts provided in save file"));
                    }
                    nstates = atoi(&value);
                    if nstates <= 0 {
                        return Err(invalid("Number of states in save file was negative"));
                    }
                    have_states = true;
                }
                "STATEPOS" => statepos = atoi(&value),
                "MOVE" | "SOLVE" | "RESTART" => {
                    if !have_states {
                        return Err(invalid("No state count provided in save file"));
                    }
                    if statepos < 0 {
                        return Err(invalid("No game position provided in save file"));
                    }
                    let value = text(value);
                    steps.push(match key.as_str() {
                        "MOVE" => SavedStep::Move(value),
                        "SOLVE" => SavedStep::Solve(value),
                        _ => SavedStep::Restart(value),
                    });
                }
                _ => {}
            }
        }

        let mut params = G::default_params();
        let parstr = parstr.ok_or(invalid("Long-term parameters in save file are missing"))?;
        G::decode_params(&mut params, &parstr);
        if G::validate_params(&params, true).is_err() {
            return Err(invalid("Long-term parameters in save file are invalid"));
        }
        let mut cparams = G::default_params();
        let cparstr = cparstr.ok_or(invalid("Short-term parameters in save file are missing"))?;
        G::decode_params(&mut cparams, &cparstr);
        if G::validate_params(&cparams, false).is_err() {
            return Err(invalid("Short-term parameters in save file are invalid"));
        }
        if seed.is_some() && G::validate_params(&cparams, true).is_err() {
            // The seed is no use with these parameters, but the rest is.
            seed = None;
        }
        let desc = desc.ok_or(invalid("Game description in save file is missing"))?;
        if G::validate_desc(&cparams, &desc).is_err() {
            return Err(invalid("Game description in save file is invalid"));
        }
        if let Some(privdesc) = &privdesc
            && G::validate_desc(&cparams, privdesc).is_err()
        {
            return Err(invalid("Game private description in save file is invalid"));
        }
        if statepos < 1 || statepos > nstates {
            return Err(invalid("Game position in save file is out of range"));
        }

        let mut ctx = GameCtx::new();
        let mut first = G::new_game(&mut ctx, &cparams, privdesc.as_deref().unwrap_or(&desc));
        if privdesc.is_some() {
            G::set_public_desc(&mut first, &desc);
        }
        let mut states = vec![Entry::<G> {
            state: first,
            step: Step::NewGame,
        }];
        for saved in steps {
            let prev = &states.last().expect("the first state").state;
            let entry = match saved {
                SavedStep::Move(text) => replay_move::<G>(&mut ctx, prev, &text, Step::Move)?,
                SavedStep::Solve(text) => replay_move::<G>(&mut ctx, prev, &text, Step::Solve)?,
                SavedStep::Restart(text) => {
                    if G::validate_desc(&cparams, &text).is_err() {
                        return Err(invalid("Save file contained an invalid restart move"));
                    }
                    let state = G::new_game(&mut ctx, &cparams, &text);
                    Entry {
                        state,
                        step: Step::Restart(text),
                    }
                }
            };
            states.push(entry);
        }

        let mut ui = G::new_ui(Some(&states[0].state));
        self.apply_prefs(&mut ui);
        if let Some(uistr) = &uistr {
            G::decode_ui(&mut ui, uistr, &states[statepos as usize - 1].state);
        }

        self.install(Snapshot {
            params,
            curparams: Some(cparams),
            seed,
            desc: Some(desc),
            privdesc,
            aux,
            states,
            statepos: statepos as usize,
            ui: Some(ui),
            elapsed,
        });
        Ok(())
    }
}

/// Replays a `MOVE` or `SOLVE` line on `prev`.
fn replay_move<G: Game>(
    ctx: &mut GameCtx,
    prev: &G::State,
    text: &str,
    step: fn(G::Move) -> Step<G::Move>,
) -> Result<Entry<G>, DeserialiseError> {
    let bad = || DeserialiseError::Invalid("Save file contained an invalid move");
    let mv = G::decode_move(prev, text).map_err(|_| bad())?;
    let state = G::execute_move(ctx, prev, &mv).ok_or_else(bad)?;
    Ok(Entry {
        state,
        step: step(mv),
    })
}
