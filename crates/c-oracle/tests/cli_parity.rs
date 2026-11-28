//! Differential tests: `puzzles-cli`'s commands against the same steps
//! on C's mid-end.
//!
//! * `roundtrip` against C's `fuzzpuzz` (load, size to `INT_MAX`,
//!   redraw, save): the same bytes or the same error, for upstream's
//!   saves (`fifteen.sav`, `flip.sav`, `guess.sav`, `mosaic.sav`, `unruly.sav`), files saved by C after random play, and damaged copies.
//! * `generate` against C's mid-end generating from the same seed IDs,
//!   non-interactively as C's batch mode does.

#![cfg(feature = "oracle")]

use c_oracle::{CGame, CMidend, c_identify_game};
use proptest::prelude::*;
use puzzles_core::game::Game;
use puzzles_games::flip::Flip;

/// C's `fuzzpuzz` on `data`, through the oracle's C mid-end.
fn c_fuzzpuzz(data: &[u8]) -> Result<Vec<u8>, String> {
    let game = match c_identify_game(data)?.as_str() {
        "Fifteen" => CGame::fifteen(),
        "Flip" => CGame::flip(),
        "Guess" => CGame::guess(),
        "Mosaic" => CGame::mosaic(),
        "Unruly" => CGame::unruly(),
        _ => return Err("Game not recognised".into()),
    };
    let mut c = CMidend::new(game, b"roundtrip", true);
    c.deserialise(data)?;
    c.size(i32::MAX, i32::MAX, false, 1.0);
    c.redraw();
    Ok(c.serialise())
}

fn rust(data: &[u8]) -> Result<Vec<u8>, String> {
    puzzles_cli::roundtrip(data).map_err(|e| e.0)
}

/// Upstream's save files (its icon sources) for the ported games: the
/// same output as `fuzzpuzz`, which is the file itself (line ends aside)
/// but for `mosaic.sav`. That file comes from an older Mosaic: C reads
/// its parameters `8x8a0h1` as `8x8` and writes them so, and drops its
/// `TIME` line, since Mosaic has no timer now.
#[test]
fn upstream_saves_round_trip_like_fuzzpuzz() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/puzzles-c/icons");
    for name in [
        "fifteen.sav",
        "flip.sav",
        "guess.sav",
        "mosaic.sav",
        "unruly.sav",
    ] {
        let data = std::fs::read(dir.join(name)).expect("an icon save file");
        let out = rust(&data);
        assert_eq!(out, c_fuzzpuzz(&data), "{name}");
        let mut lf = String::from_utf8(data).unwrap().replace("\r\n", "\n");
        if name == "mosaic.sav" {
            lf = lf
                .replace(":7:8x8a0h1\n", ":3:8x8\n")
                .replace("TIME    :7:214.795\n", "");
        }
        assert_eq!(out.map(|o| String::from_utf8(o).unwrap()), Ok(lf), "{name}");
    }
}

/// A save file C writes after some play.
fn c_save(seed: &[u8], clicks: &[(i32, i32)], solve: bool) -> Vec<u8> {
    let mut c = CMidend::new(CGame::flip(), seed, true);
    c.new_game();
    c.size(300, 300, false, 1.0);
    for &(x, y) in clicks {
        c.process_key(x, y, 0x200);
        c.process_key(x, y, 0x206);
        c.timer(1.0);
    }
    if solve {
        let _ = c.solve();
    }
    c.process_key(0, 0, i32::from(b'u'));
    c.serialise()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn saves_round_trip_like_fuzzpuzz(
        seed in prop::collection::vec(any::<u8>(), 0..6),
        clicks in prop::collection::vec((0..300_i32, 0..300_i32), 0..6),
        solve: bool,
        edits in prop::collection::vec(
            (any::<prop::sample::Index>(), 0..4_u8, prop::sample::select(b"0129:\n SMx".to_vec())),
            0..3,
        ),
    ) {
        let mut data = c_save(&seed, &clicks, solve);
        for (at, edit, byte) in edits {
            let i = at.index(data.len() + 1);
            match edit {
                0 => data.truncate(i),
                1 => data.insert(i, byte),
                2 if i < data.len() => data[i] = byte,
                _ => {}
            }
        }
        let (r, c) = (rust(&data), c_fuzzpuzz(&data));
        match (&r, &c) {
            // Loaded moves are written back canonically (D4); the game
            // must still be C's: C reads Rust's file back unchanged.
            (Ok(r), Ok(c)) if r != c => prop_assert_eq!(&c_fuzzpuzz(r), &Ok(r.clone())),
            _ => prop_assert_eq!(r, c),
        }
    }

    #[test]
    fn generate_matches_c(
        preset in 0..6_usize,
        seed in "[0-9a-z]{1,10}",
    ) {
        let (_, params) = Flip::presets().presets().nth(preset).map(|(t, p)| (t.to_owned(), *p)).unwrap();
        let id = format!("{}#{seed}", Flip::encode_params(&params, true));
        let flip = puzzles_cli::find_game("flip").unwrap();
        let r = puzzles_cli::generate(flip, Some(&id), 1, b"x").unwrap();
        let mut c = CMidend::new(CGame::flip(), b"x", false);
        c.game_id(&id).unwrap();
        c.new_game();
        prop_assert_eq!(r, vec![c.get_game_id().unwrap()]);
    }
}
