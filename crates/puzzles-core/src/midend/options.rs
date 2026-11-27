//! Settings C's mid-end reads from the environment (`<GAME>_DEFAULT`,
//! `<GAME>_PRESETS`, `<GAME>_TILESIZE`), passed in instead (mid-end
//! decision (b)). C's `<GAME>_COLOUR_<n>` is replaced by themes (D7).

use crate::game::Game;
use crate::parse::scan_int;
use crate::preset::{PresetEntry, PresetMenu};

/// Options for a [`Midend`](super::Midend), which never reads the
/// environment itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidendOptions {
    /// Parameters decoded over the game's defaults for New Game (C's
    /// `<GAME>_DEFAULT`, such as `NET_DEFAULT=25x25w`). Game IDs with a
    /// seed ignore it, as in C, so they mean the same everywhere.
    pub default_params: Option<String>,
    /// Presets listed after the game's own, each a parameter string
    /// decoded over the game's defaults; those that don't validate are
    /// dropped. Front ends fill it from the user's presets file and from
    /// C's `<GAME>_PRESETS` ([`parse_env_presets`]), in that order.
    pub extra_presets: PresetMenu<String>,
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
            extra_presets: var("PRESETS").map_or_default(|text| parse_env_presets(&text)),
            // C: sscanf(e, "%d", &ts) == 1 && ts > 0.
            tilesize: var("TILESIZE")
                .and_then(|v| scan_int(v.as_bytes()).map(|(ts, _)| ts))
                .filter(|&ts| ts > 0),
            stylus_based: false,
        }
    }
}

/// Parses a `<GAME>_PRESETS` string into a menu of parameter strings
/// (the parsing half of C's `preset_menu_add_from_user_env`).
/// `title:params` pairs are joined by colons; `title:#` opens a
/// submenu and `:#` closes it. A close at the top level is ignored, and
/// an unclosed submenu runs to the end of the text.
#[doc(alias = "preset_menu_add_from_user_env")]
#[must_use]
pub fn parse_env_presets(text: &str) -> PresetMenu<String> {
    let mut menu = PresetMenu::new();
    parse_from(&mut menu, text, 0, true);
    menu
}

fn parse_from(menu: &mut PresetMenu<String>, text: &str, mut pos: usize, top_level: bool) -> usize {
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
                pos = parse_from(submenu, text, pos, false);
            } else if !top_level {
                return pos;
            }
            continue;
        }
        menu.add_preset(name, value.to_owned());
    }
    pos
}

/// Appends `extra` to `menu`, decoding each parameter string over the
/// game's defaults (the adding half of C's
/// `preset_menu_add_from_user_env`). Presets whose parameters don't
/// validate are dropped; submenus are kept even if left empty, as in C.
pub(super) fn add_extra_presets<G: Game>(
    menu: &mut PresetMenu<G::Params>,
    extra: &PresetMenu<String>,
) {
    for entry in &extra.entries {
        match entry {
            PresetEntry::Preset {
                title,
                params: text,
            } => {
                let mut params = G::default_params();
                G::decode_params(&mut params, text);
                if G::validate_params(&params, true).is_ok() {
                    menu.add_preset(title.clone(), params);
                }
            }
            PresetEntry::Submenu { title, menu: sub } => {
                add_extra_presets::<G>(menu.add_submenu(title.clone()), sub);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(title: &str, params: &str) -> PresetEntry<String> {
        PresetEntry::Preset {
            title: title.into(),
            params: params.into(),
        }
    }

    #[test]
    fn env_presets_parse_like_c() {
        let menu = parse_env_presets("Big:6x6c:Sub:#:Tiny:2x2:Bad:0x0::#:After:3x3");
        let mut sub = PresetMenu::new();
        sub.entries
            .extend([preset("Tiny", "2x2"), preset("Bad", "0x0")]);
        assert_eq!(
            menu.entries,
            [
                preset("Big", "6x6c"),
                PresetEntry::Submenu {
                    title: "Sub".into(),
                    menu: sub,
                },
                preset("After", "3x3"),
            ]
        );
    }

    #[test]
    fn env_preset_edge_cases() {
        // A top-level close is ignored, an empty title is kept, a
        // missing value is an empty string, and an unclosed submenu
        // takes the rest.
        let menu = parse_env_presets(":#::4x4:Open:#:Last");
        assert_eq!(menu.entries.len(), 2);
        assert_eq!(menu.entries[0], preset("", "4x4"));
        let PresetEntry::Submenu { title, menu: sub } = &menu.entries[1] else {
            panic!("a submenu");
        };
        assert_eq!(title, "Open");
        assert_eq!(sub.entries, [preset("Last", "")]);
        assert!(parse_env_presets("").is_empty());
        // Non-ASCII titles split only at colons.
        assert_eq!(
            parse_env_presets("Grand é:5x5").entries,
            [preset("Grand é", "5x5")]
        );
    }
}
