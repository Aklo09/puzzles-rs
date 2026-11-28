//! The Classic theme against C's colours.
//!
//! * Classic is generated here from the C code (D7c): role values from
//!   C's conventions with the default background (0.9 grey) and
//!   `game_mkhighlight` of it, each ported game's `Specific` colours from
//!   its C `colours()`, and an override for every entry where Classic
//!   would otherwise differ. The checked-in
//!   `puzzles-theme/themes/classic.toml` must equal the generated text;
//!   regenerate it with `PUZZLES_UPDATE_CLASSIC=1 cargo test -p c-oracle
//!   --features oracle --test theme_parity`.
//! * Palette parity (D7d): every ported game's palette resolved with the
//!   built-in Classic equals its C `colours()` within float epsilon.
//! * The bevel arithmetic equals C's `game_mkhighlight_specific` exactly.

#![cfg(feature = "oracle")]

use std::collections::BTreeMap;

use c_oracle::{CGame, c_mkhighlight};
use proptest::prelude::*;
use puzzles_core::game::Game;
use puzzles_core::palette::{ColourSpec, PaletteEntry};
use puzzles_theme::{Rgb, Theme, ThemeSet, bevel, game_key, series_key};

/// Largest component difference between Classic and C: C's `bg / 3`
/// against the port's `bg * (1/3)` differ in the last bit.
const EPSILON: f32 = 1e-6;

/// A ported game: its name, palette, C original, and C's palette index
/// for each of its entries, empty when they are the same. A port can
/// give a C alias an entry of its own (Mosaic's clue text colours, C's
/// `COL_TEXT_DARK = COL_MARKED`), which must equal the aliased colour.
struct Ported {
    name: &'static str,
    palette: &'static [PaletteEntry],
    c_game: CGame,
    c_colours: &'static [usize],
}

impl Ported {
    /// C's colour for each of the port's entries.
    fn c_colours(&self) -> Vec<Rgb> {
        let c: Vec<Rgb> = self.c_game.colours().into_iter().map(rgb).collect();
        if self.c_colours.is_empty() {
            return c;
        }
        assert_eq!(self.c_colours.len(), self.palette.len(), "{}", self.name);
        self.c_colours.iter().map(|&i| c[i]).collect()
    }
}

/// The ported games, with their C originals. Add each game as it is
/// ported, then regenerate Classic.
fn ported() -> Vec<Ported> {
    let game = |name, palette, c_game| Ported {
        name,
        palette,
        c_game,
        c_colours: &[],
    };
    vec![
        game(
            puzzles_games::fifteen::Fifteen::NAME,
            puzzles_games::fifteen::Fifteen::PALETTE,
            CGame::fifteen(),
        ),
        game(
            puzzles_games::flip::Flip::NAME,
            puzzles_games::flip::Flip::PALETTE,
            CGame::flip(),
        ),
        game(
            puzzles_games::guess::Guess::NAME,
            puzzles_games::guess::Guess::PALETTE,
            CGame::guess(),
        ),
        Ported {
            c_colours: &[0, 1, 2, 3, 4, 5, 6, 7, 3, 4],
            // Mosaic's clue text colours (also in mosaic_parity.rs).
            ..game(
                puzzles_games::mosaic::Mosaic::NAME,
                puzzles_games::mosaic::Mosaic::PALETTE,
                CGame::mosaic(),
            )
        },
        game(
            puzzles_games::nullgame::NullGame::NAME,
            puzzles_games::nullgame::NullGame::PALETTE,
            CGame::nullgame(),
        ),
        game(
            puzzles_games::unruly::Unruly::NAME,
            puzzles_games::unruly::Unruly::PALETTE,
            CGame::unruly(),
        ),
    ]
}

fn rgb(c: [f32; 3]) -> Rgb {
    Rgb::new(c[0], c[1], c[2])
}

/// Classic's role values: the colours most C games use for each role
/// (a survey of the 40 `colours()` functions), computed as C computes
/// them from the default background. Roles the games disagree on have
/// provisional values; ported games that differ get overrides.
fn classic_roles() -> BTreeMap<String, Rgb> {
    // The null game's only colour is the front end's default.
    let bg = rgb(CGame::nullgame().colours()[0]);
    let [_, highlight, lowlight] = c_mkhighlight(bg.to_array());
    let roles = [
        ("background", bg),
        ("highlight", rgb(highlight)),
        ("lowlight", rgb(lowlight)),
        ("ink", Rgb::BLACK),
        ("grid", Rgb::BLACK),
        ("cursor", Rgb::new(1.0, 0.0, 0.0)),
        ("error", Rgb::new(1.0, 0.0, 0.0)),
        ("flash", Rgb::WHITE),
        ("clue", Rgb::BLACK),
        ("user_entry", Rgb::new(0.0, 0.6 * bg.g, 0.0)),
        ("pencil", Rgb::new(0.5 * bg.r, 0.5 * bg.g, bg.b)),
        // Provisional: no common C value.
        ("done", Rgb::new(0.0, 0.5, 0.0)),
        ("hint", Rgb::new(1.0, 0.0, 0.0)),
        ("selected", Rgb::new(0.5 * bg.r, 0.5 * bg.g, bg.b)),
        ("faint", Rgb::grey(0.5 * bg.r)),
        ("filled", Rgb::BLACK),
    ];
    roles.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn resolve(theme: &Theme, name: &str, palette: &[PaletteEntry]) -> Vec<Rgb> {
    let mut set = ThemeSet::default();
    set.insert(theme.clone());
    set.resolve(&theme.name, name, palette)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn generate() -> Theme {
    let mut theme = Theme {
        name: "Classic".into(),
        roles: classic_roles(),
        ..Theme::default()
    };
    for game in ported() {
        let (name, palette) = (game.name, game.palette);
        let c = game.c_colours();
        assert_eq!(c.len(), palette.len(), "{name}: palette size");
        let key = game_key(name);
        let mut colours = puzzles_theme::GameColours::default();
        for (entry, &colour) in palette.iter().zip(&c) {
            match entry.spec {
                ColourSpec::Specific(k) => {
                    colours.specific.insert(k.to_owned(), colour);
                }
                // The first game using a series colour supplies it; later
                // games that differ get overrides.
                ColourSpec::Series(series, n) => {
                    let list = theme
                        .series
                        .entry(series_key(series).to_owned())
                        .or_default();
                    match usize::from(n).cmp(&list.len()) {
                        std::cmp::Ordering::Equal => list.push(colour),
                        std::cmp::Ordering::Less => {}
                        std::cmp::Ordering::Greater => {
                            panic!("{name}: series {series:?} skips colours")
                        }
                    }
                }
                _ => {}
            }
        }
        theme.games.insert(key.clone(), colours);
        // Override entries until every one matches; overriding a base
        // also fixes the entries derived from it.
        loop {
            let resolved = resolve(&theme, name, palette);
            let Some(i) = (0..palette.len()).find(|&i| resolved[i].max_difference(c[i]) > EPSILON)
            else {
                break;
            };
            theme
                .games
                .get_mut(&key)
                .expect("inserted above")
                .overrides
                .insert(palette[i].key(), c[i]);
        }
    }
    theme
}

#[test]
fn classic_theme_is_generated_from_c() {
    let text = format!(
        "# The Classic theme: the C colours, with the default background 0.9 grey.\n\
         # Generated by c-oracle/tests/theme_parity.rs; do not edit.\n\
         # Regenerate: PUZZLES_UPDATE_CLASSIC=1 cargo test -p c-oracle --features oracle --test theme_parity\n\
         {}",
        generate().to_toml()
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../puzzles-theme/themes/classic.toml");
    if std::env::var_os("PUZZLES_UPDATE_CLASSIC").is_some() {
        std::fs::write(&path, &text).expect("write classic.toml");
    }
    let file = std::fs::read_to_string(&path).expect("classic.toml");
    assert_eq!(
        file.replace("\r\n", "\n"),
        text,
        "classic.toml is out of date; regenerate it (see this file's header)"
    );
}

#[test]
fn palettes_match_c_under_classic() {
    let set = ThemeSet::builtin();
    for game in ported() {
        let (name, palette) = (game.name, game.palette);
        let resolved = set.resolve("Classic", name, palette).expect("resolves");
        let c = game.c_colours();
        assert_eq!(resolved.len(), c.len(), "{name}: palette size");
        for (i, (r, &c)) in resolved.iter().zip(&c).enumerate() {
            assert!(
                r.max_difference(c) <= EPSILON,
                "{name} entry {i} ({}): {r} vs C {:?}",
                palette[i].name,
                c
            );
        }
    }
}

proptest! {
    #[test]
    fn bevel_matches_c(r in 0.0_f32..=1.0, g in 0.0_f32..=1.0, b in 0.0_f32..=1.0) {
        let ours = bevel(Rgb::new(r, g, b));
        let [base, hi, lo] = c_mkhighlight([r, g, b]);
        prop_assert_eq!(ours.base.to_array().map(f32::to_bits), base.map(f32::to_bits));
        prop_assert_eq!(ours.highlight.to_array().map(f32::to_bits), hi.map(f32::to_bits));
        prop_assert_eq!(ours.lowlight.to_array().map(f32::to_bits), lo.map(f32::to_bits));
    }
}

#[test]
fn bevel_matches_c_at_the_edges() {
    for c in [
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.9, 0.9, 0.9],
        [0.1, 0.1, 0.1],
        [1.0, 0.0, 0.0],
        [0.5, 0.5, 0.5],
    ] {
        let ours = bevel(rgb(c));
        let [base, hi, lo] = c_mkhighlight(c);
        assert_eq!(
            [ours.base, ours.highlight, ours.lowlight].map(Rgb::to_array),
            [base, hi, lo],
            "{c:?}"
        );
    }
}
