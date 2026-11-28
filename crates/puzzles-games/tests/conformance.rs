//! Conformance of every game with the `Game` trait's contract, beyond
//! what the compiler checks: each `CAN_*` const agrees with its method,
//! parameters round-trip through their text form, generated puzzles
//! validate, the solver's move survives the move codec and executes,
//! and the palette is well formed.

use puzzles_core::game::{Game, GameCtx, GameError};
use puzzles_core::palette::{ColourRef, ColourSpec, Role};
use puzzles_core::random::RandomState;

/// The parameter sets to try: the defaults and every preset.
fn param_sets<G: Game>() -> Vec<G::Params> {
    let mut sets = vec![G::default_params()];
    sets.extend(G::presets().presets().map(|(_, p)| p.clone()));
    sets
}

fn encode_move<G: Game>(mv: &G::Move) -> String {
    let mut s = String::new();
    G::encode_move(mv, &mut s);
    s
}

fn check<G: Game>() {
    let name = G::NAME;

    // Parameters.
    for params in param_sets::<G>() {
        G::validate_params(&params, true)
            .unwrap_or_else(|e| panic!("{name}: preset {params:?} invalid: {e}"));
        let mut decoded = G::default_params();
        G::decode_params(&mut decoded, &G::encode_params(&params, true));
        assert_eq!(decoded, params, "{name}: full encoding round trip");
    }
    for (title, _) in G::presets().presets() {
        assert!(!title.is_empty(), "{name}: preset without a title");
    }

    // Custom dialog.
    let defaults = G::default_params();
    if let Some(config) = G::configure(&defaults) {
        assert!(G::CAN_CONFIGURE, "{name}: configure without CAN_CONFIGURE");
        assert_eq!(
            G::custom_params(&config),
            Ok(defaults.clone()),
            "{name}: the dialog round-trips the parameters"
        );
    } else {
        assert!(!G::CAN_CONFIGURE, "{name}: CAN_CONFIGURE without configure");
        assert_eq!(G::custom_params(&[]), Err(GameError::unsupported()));
    }

    // Generation, states, solving, text.
    for (i, params) in param_sets::<G>().iter().enumerate() {
        let seed = format!("conformance {i}");
        let new = G::new_desc(params, &mut RandomState::new(seed.as_bytes()), false);
        G::validate_desc(params, &new.desc)
            .unwrap_or_else(|e| panic!("{name}: generated {:?} invalid: {e}", new.desc));
        let state = G::new_game(&mut GameCtx::new(), params, &new.desc);

        let solved = G::solve(&state, &state, new.aux.as_deref());
        if G::CAN_SOLVE {
            assert_ne!(
                solved,
                Err(GameError::unsupported()),
                "{name}: CAN_SOLVE without solve"
            );
        } else {
            assert_eq!(
                solved,
                Err(GameError::unsupported()),
                "{name}: solve without CAN_SOLVE"
            );
        }
        if let Ok(mv) = solved {
            let text = encode_move::<G>(&mv);
            assert_eq!(
                G::decode_move(&state, &text).as_ref(),
                Ok(&mv),
                "{name}: {text}"
            );
            assert!(
                G::execute_move(&mut GameCtx::new(), &state, &mv).is_some(),
                "{name}: the solver's move {text} doesn't execute"
            );
        }

        let text = G::text_format(&state);
        if G::can_format_as_text_now(params) {
            assert!(G::CAN_FORMAT_AS_TEXT_EVER, "{name}: text now but never");
            assert!(text.is_some(), "{name}: can format as text, but doesn't");
        }
        if !G::CAN_FORMAT_AS_TEXT_EVER {
            assert_eq!(
                text, None,
                "{name}: text_format without CAN_FORMAT_AS_TEXT_EVER"
            );
        }

        let ui = G::new_ui(Some(&state));
        let (w, h) = G::compute_size(params, G::PREFERRED_TILESIZE, &ui);
        assert!(w > 0 && h > 0, "{name}: empty puzzle area");
    }

    // Palette.
    assert_eq!(
        G::PALETTE.first().map(|e| e.spec),
        Some(ColourSpec::Role(Role::Background)),
        "{name}: colour 0 is the background, as in C"
    );
    // Classic has every colour the game needs (its C colours, checked
    // against C by c-oracle's theme_parity).
    if let Err(e) = puzzles_theme::ThemeSet::builtin().resolve("Classic", name, G::PALETTE) {
        panic!("{name}: Classic can't colour it: {e}");
    }
    let mut keys: Vec<String> = G::PALETTE
        .iter()
        .map(puzzles_core::palette::PaletteEntry::key)
        .collect();
    keys.sort();
    keys.dedup();
    assert_eq!(
        keys.len(),
        G::PALETTE.len(),
        "{name}: palette names are distinct"
    );
    for entry in G::PALETTE {
        let spec = &entry.spec;
        if let ColourSpec::Derived { base, op } = spec {
            let in_range = |ColourRef(i): ColourRef| i < G::PALETTE.len();
            assert!(in_range(*base), "{name}: {spec:?} derives from nothing");
            if let puzzles_core::palette::DeriveOp::Mix { with, .. } = op {
                assert!(in_range(*with), "{name}: {spec:?} mixes with nothing");
            }
        }
    }
}

#[cfg(feature = "fifteen")]
#[test]
fn fifteen() {
    check::<puzzles_games::fifteen::Fifteen>();
}

#[cfg(feature = "flip")]
#[test]
fn flip() {
    check::<puzzles_games::flip::Flip>();
}

#[cfg(feature = "guess")]
#[test]
fn guess() {
    check::<puzzles_games::guess::Guess>();
}

#[cfg(feature = "mosaic")]
#[test]
fn mosaic() {
    check::<puzzles_games::mosaic::Mosaic>();
}

#[cfg(feature = "nullgame")]
#[test]
fn nullgame() {
    check::<puzzles_games::nullgame::NullGame>();
}

#[cfg(feature = "unruly")]
#[test]
fn unruly() {
    check::<puzzles_games::unruly::Unruly>();
}

/// The built-in themes colour only what exists: every key in their game
/// sections names a game in the registry and one of its colours.
#[test]
fn built_in_themes_have_no_unused_game_keys() {
    let games: Vec<(&str, &[puzzles_core::palette::PaletteEntry])> = puzzles_games::GAMES
        .iter()
        .map(|g| (g.name, g.palette))
        .collect();
    let set = puzzles_theme::ThemeSet::builtin();
    for name in set.names() {
        let theme = set.get(&name).expect("listed");
        let unused = puzzles_theme::unused_game_keys(theme, &games);
        assert!(unused.is_empty(), "{name}: {unused:?}");
    }
}

/// A game's colour table in `docs/themes.md`: each palette entry's key
/// and where its colour comes from.
fn theme_doc_table(name: &str, palette: &[puzzles_core::palette::PaletteEntry]) -> String {
    use std::fmt::Write as _;

    use puzzles_core::palette::{ColourSpec as S, DeriveOp as D};
    let key = |i: usize| format!("`{}`", palette[i].key());
    let mut out = format!("### {name}\n\n| Colour | Default |\n|---|---|\n");
    for entry in palette {
        let source = match entry.spec {
            S::Role(role) => format!("role `{}`", puzzles_theme::role_key(role)),
            S::Series(series, n) => format!(
                "series `{}`, colour {}",
                puzzles_theme::series_key(series),
                n + 1
            ),
            S::Specific(_) => format!("its own, under `[game.{}]`", puzzles_theme::game_key(name)),
            S::Derived { base, op } => match op {
                D::Shade(f) => format!("{} × {f}", key(base.0)),
                D::Mix { with, t } => format!("{} mixed with {} ({t})", key(base.0), key(with.0)),
                D::Highlight => format!("bevel highlight of {}", key(base.0)),
                D::Lowlight => format!("bevel lowlight of {}", key(base.0)),
            },
        };
        let _ = writeln!(out, "| `{}` | {source} |", entry.key());
    }
    out
}

/// `docs/themes.md` lists every game's colours as they are.
#[test]
fn theme_doc_lists_every_games_colours() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/themes.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    let mut missing = String::new();
    for g in puzzles_games::GAMES {
        let table = theme_doc_table(g.name, g.palette);
        if !doc.contains(&table) {
            missing += &table;
            missing += "\n";
        }
    }
    assert!(
        missing.is_empty(),
        "docs/themes.md is missing or has outdated game tables; it should contain:\n\n{missing}"
    );
}
