//! Settings C's mid-end reads from the environment (`<GAME>_DEFAULT`,
//! `<GAME>_PRESETS`, `<GAME>_TILESIZE`), passed in instead (mid-end
//! decision (b)). C's `<GAME>_COLOUR_<n>` is replaced by themes (D7).

use crate::game::Game;
use crate::parse::scan_int;
use crate::preset::PresetMenu;

/// Options for a [`Midend`](super::Midend), which never reads the
/// environment itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidendOptions {
    /// Parameters decoded over the game's defaults for New Game (C's
    /// `<GAME>_DEFAULT`, such as `NET_DEFAULT=25x25w`). Game IDs with a
    /// seed ignore it, as in C, so they mean the same everywhere.
    pub default_params: Option<String>,
    /// Extra presets (C's `<GAME>_PRESETS`): `title:params` pairs joined
    /// by colons; `title:#` opens a submenu and `:#` closes it.
    /// Parameters that don't validate are dropped.
    pub extra_presets: Option<String>,
    /// The preferred tile size instead of the game's (C's
    /// `<GAME>_TILESIZE`).
    pub tilesize: Option<i32>,
    /// The front end is stylus-based, so games with stylus support see
    /// `MOD_STYLUS` on pointer events (C's `STYLUS_BASED` build flag).
    pub stylus_based: bool,
}

impl MidendOptions {
    /// The options C would read for `game_name` (a game's `NAME`),
    /// looking variables up with `lookup`, such as
    /// `|name| std::env::var(name).ok()`. Names are C's: the game name
    /// without spaces, in capitals, then `_DEFAULT`, `_PRESETS` or
    /// `_TILESIZE` (`NULLGAME_TILESIZE`).
    #[must_use]
    pub fn from_lookup(game_name: &str, lookup: impl Fn(&str) -> Option<String>) -> Self {
        let prefix: String = game_name
            .bytes()
            .filter(|b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
            .map(|b| char::from(b.to_ascii_uppercase()))
            .collect();
        let var = |suffix: &str| lookup(&format!("{prefix}_{suffix}"));
        Self {
            default_params: var("DEFAULT"),
            extra_presets: var("PRESETS"),
            // C: sscanf(e, "%d", &ts) == 1 && ts > 0.
            tilesize: var("TILESIZE")
                .and_then(|v| scan_int(v.as_bytes()).map(|(ts, _)| ts))
                .filter(|&ts| ts > 0),
            stylus_based: false,
        }
    }
}

/// Adds the presets of a `<GAME>_PRESETS` string to `menu` (C's
/// `preset_menu_add_from_user_env`).
pub(super) fn add_presets_from_text<G: Game>(menu: &mut PresetMenu<G::Params>, text: &str) {
    add_from::<G>(menu, text, 0, true);
}

fn add_from<G: Game>(
    menu: &mut PresetMenu<G::Params>,
    text: &str,
    mut pos: usize,
    top_level: bool,
) -> usize {
    let field = |pos: &mut usize| -> &str {
        let rest = &text[*pos..];
        let end = rest.find(':').unwrap_or(rest.len());
        *pos += end + usize::from(end < rest.len());
        &rest[..end]
    };
    while pos < text.len() {
        let name = field(&mut pos);
        let value = field(&mut pos);
        if value == "#" {
            if !name.is_empty() {
                let submenu = menu.add_submenu(name);
                pos = add_from::<G>(submenu, text, pos, false);
            } else if !top_level {
                return pos;
            }
            // A close at the top level is quietly ignored.
            continue;
        }
        let mut params = G::default_params();
        G::decode_params(&mut params, value);
        if G::validate_params(&params, true).is_ok() {
            menu.add_preset(name, params);
        }
    }
    pos
}
