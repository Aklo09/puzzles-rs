//! A theme as data, and its TOML form.
//!
//! ```toml
//! name = "Classic"
//! extends = "..."            # optional: a theme to take missing values from
//! dark = false               # optional
//!
//! [roles]                    # every Role, by snake_case name
//! background = [0.9, 0.9, 0.9]
//! ink = "#000000"            # any CSS colour, or a float triple 0..1
//!
//! [series]                   # each Series as a list of colours
//! pieces = ["#4e79a7", "#f28e2b"]
//!
//! [game.flip]                # a game's Specific keys (inherited)
//! right = [1.0, 1.0, 1.0]
//!
//! [game.flip.overrides]      # entries replaced in this theme only
//! grid = [0.6, 0.6, 0.6]     # (palette entry names, snake_case)
//! ```
//!
//! Games are keyed by their name in lower case without spaces (`flip`,
//! `nullgame`). Float triples keep C's exact `f32` values, which Classic
//! needs; CSS colours are for people writing themes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use puzzles_core::palette::{Role, Series};
use serde::Deserialize;

use crate::colour::Rgb;

/// Every role, in declaration order.
pub const ROLES: [Role; 16] = [
    Role::Background,
    Role::Highlight,
    Role::Lowlight,
    Role::Ink,
    Role::Grid,
    Role::Cursor,
    Role::Error,
    Role::Flash,
    Role::Clue,
    Role::UserEntry,
    Role::Pencil,
    Role::Done,
    Role::Hint,
    Role::Selected,
    Role::Faint,
    Role::Filled,
];

/// Every series, in declaration order.
pub const SERIES: [Series; 4] = [
    Series::Pieces,
    Series::Numbers,
    Series::Binary,
    Series::Chains,
];

/// A role's key in theme files.
#[must_use]
pub fn role_key(role: Role) -> &'static str {
    match role {
        Role::Background => "background",
        Role::Highlight => "highlight",
        Role::Lowlight => "lowlight",
        Role::Ink => "ink",
        Role::Grid => "grid",
        Role::Cursor => "cursor",
        Role::Error => "error",
        Role::Flash => "flash",
        Role::Clue => "clue",
        Role::UserEntry => "user_entry",
        Role::Pencil => "pencil",
        Role::Done => "done",
        Role::Hint => "hint",
        Role::Selected => "selected",
        Role::Faint => "faint",
        Role::Filled => "filled",
    }
}

/// A series' key in theme files.
#[must_use]
pub fn series_key(series: Series) -> &'static str {
    match series {
        Series::Pieces => "pieces",
        Series::Numbers => "numbers",
        Series::Binary => "binary",
        Series::Chains => "chains",
    }
}

/// A game's key in theme files: its name in lower case, without spaces.
#[must_use]
pub fn game_key(game_name: &str) -> String {
    game_name
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A theme's colours for one game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameColours {
    /// Values of the game's `Specific` keys; inherited by themes that
    /// extend this one.
    pub specific: BTreeMap<String, Rgb>,
    /// Palette entries replaced in this theme only, by entry key.
    pub overrides: BTreeMap<String, Rgb>,
}

/// A theme: values for roles, series and game-specific colours.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Theme {
    /// The name, as shown and as `extends` refers to it.
    pub name: String,
    /// A theme to take missing values from.
    pub extends: Option<String>,
    /// Whether this is a dark theme.
    pub dark: bool,
    /// Role values.
    pub roles: BTreeMap<String, Rgb>,
    /// Series values.
    pub series: BTreeMap<String, Vec<Rgb>>,
    /// Per-game colours, by game key.
    pub games: BTreeMap<String, GameColours>,
    /// The app's own colours (`[ui]`: `panel`, `text`, `accent`); those
    /// missing come from the roles.
    pub ui: BTreeMap<String, Rgb>,
}

/// The keys a theme's `[ui]` section may set.
pub const UI_KEYS: [&str; 3] = ["panel", "text", "accent"];

/// Why a theme file couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    /// Not valid TOML, or not shaped like a theme.
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    /// A colour that isn't one.
    #[error("{key}: {message}")]
    Colour {
        /// Where.
        key: String,
        /// What's wrong.
        message: String,
    },
    /// A key that isn't a role, series or UI colour.
    #[error("unknown {kind} {key:?}")]
    UnknownKey {
        /// "role", "series" or "ui".
        kind: &'static str,
        /// The key.
        key: String,
    },
}

/// A colour as written: CSS text or a float triple.
#[derive(Deserialize)]
#[serde(untagged)]
enum ColourText {
    Css(String),
    Floats([f32; 3]),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    extends: Option<String>,
    #[serde(default)]
    dark: bool,
    #[serde(default)]
    roles: BTreeMap<String, ColourText>,
    #[serde(default)]
    series: BTreeMap<String, Vec<ColourText>>,
    #[serde(default)]
    game: BTreeMap<String, GameFile>,
    #[serde(default)]
    ui: BTreeMap<String, ColourText>,
}

#[derive(Deserialize)]
struct GameFile {
    #[serde(default)]
    overrides: BTreeMap<String, ColourText>,
    #[serde(flatten)]
    specific: BTreeMap<String, ColourText>,
}

fn parse_colour(key: &str, text: ColourText) -> Result<Rgb, ThemeError> {
    let bad = |message: String| ThemeError::Colour {
        key: key.to_owned(),
        message,
    };
    match text {
        ColourText::Floats(c) => {
            if c.iter().all(|v| (0.0..=1.0).contains(v)) {
                Ok(Rgb::new(c[0], c[1], c[2]))
            } else {
                Err(bad("components must be from 0 to 1".to_owned()))
            }
        }
        ColourText::Css(s) => {
            let c = csscolorparser::parse(&s).map_err(|e| bad(e.to_string()))?;
            if c.a < 1.0 {
                return Err(bad("colours can't be transparent".to_owned()));
            }
            Ok(Rgb::new(c.r, c.g, c.b))
        }
    }
}

impl Theme {
    /// Reads a theme file.
    ///
    /// # Errors
    /// Invalid TOML, an unknown key, or a value that isn't a colour.
    pub fn from_toml(text: &str) -> Result<Self, ThemeError> {
        let file: ThemeFile = toml::from_str(text)?;
        let mut theme = Self {
            name: file.name,
            extends: file.extends,
            dark: file.dark,
            ..Self::default()
        };
        for (key, value) in file.roles {
            if !ROLES.iter().any(|&r| role_key(r) == key) {
                return Err(ThemeError::UnknownKey { kind: "role", key });
            }
            let colour = parse_colour(&format!("roles.{key}"), value)?;
            theme.roles.insert(key, colour);
        }
        for (key, values) in file.series {
            if !SERIES.iter().any(|&s| series_key(s) == key) {
                return Err(ThemeError::UnknownKey {
                    kind: "series",
                    key,
                });
            }
            let colours = values
                .into_iter()
                .enumerate()
                .map(|(i, v)| parse_colour(&format!("series.{key}[{i}]"), v))
                .collect::<Result<_, _>>()?;
            theme.series.insert(key, colours);
        }
        for (key, value) in file.ui {
            if !UI_KEYS.contains(&key.as_str()) {
                return Err(ThemeError::UnknownKey { kind: "ui", key });
            }
            let colour = parse_colour(&format!("ui.{key}"), value)?;
            theme.ui.insert(key, colour);
        }
        for (game, section) in file.game {
            let mut colours = GameColours::default();
            for (key, value) in section.specific {
                let colour = parse_colour(&format!("game.{game}.{key}"), value)?;
                colours.specific.insert(key, colour);
            }
            for (key, value) in section.overrides {
                let colour = parse_colour(&format!("game.{game}.overrides.{key}"), value)?;
                colours.overrides.insert(key, colour);
            }
            theme.games.insert(game, colours);
        }
        Ok(theme)
    }

    /// The theme as a TOML file, colours as float triples (exact), roles
    /// and series in declaration order, games and keys sorted.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let quote = |s: &str| toml::Value::String(s.to_owned()).to_string();
        let mut out = format!("name = {}\n", quote(&self.name));
        if let Some(parent) = &self.extends {
            let _ = writeln!(out, "extends = {}", quote(parent));
        }
        if self.dark {
            out += "dark = true\n";
        }
        out += "\n[roles]\n";
        for role in ROLES {
            if let Some(c) = self.roles.get(role_key(role)) {
                let _ = writeln!(out, "{} = {c}", role_key(role));
            }
        }
        if !self.series.is_empty() {
            out += "\n[series]\n";
            for series in SERIES {
                if let Some(colours) = self.series.get(series_key(series)) {
                    let list: Vec<String> = colours.iter().map(ToString::to_string).collect();
                    let _ = writeln!(out, "{} = [{}]", series_key(series), list.join(", "));
                }
            }
        }
        for (game, colours) in &self.games {
            if !colours.specific.is_empty() || colours.overrides.is_empty() {
                let _ = writeln!(out, "\n[game.{game}]");
                for (key, c) in &colours.specific {
                    let _ = writeln!(out, "{key} = {c}");
                }
            }
            if !colours.overrides.is_empty() {
                let _ = writeln!(out, "\n[game.{game}.overrides]");
                for (key, c) in &colours.overrides {
                    let _ = writeln!(out, "{key} = {c}");
                }
            }
        }
        if !self.ui.is_empty() {
            out += "\n[ui]\n";
            for key in UI_KEYS {
                if let Some(c) = self.ui.get(key) {
                    let _ = writeln!(out, "{key} = {c}");
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
name = "Sample"
extends = "classic"
dark = true

[roles]
background = "#1e1f22"
ink = [0.9, 0.9, 0.9]

[series]
pieces = ["red", "rgb(0 128 255)"]

[game.flip]
right = "white"

[game.flip.overrides]
grid = [0.6, 0.6, 0.6]
"##;

    #[test]
    fn reads_css_and_float_colours() {
        let t = Theme::from_toml(SAMPLE).unwrap();
        assert_eq!(t.name, "Sample");
        assert_eq!(t.extends.as_deref(), Some("classic"));
        assert!(t.dark);
        assert_eq!(t.roles["ink"], Rgb::grey(0.9));
        assert_eq!(t.roles["background"].to_rgb8(), [0x1e, 0x1f, 0x22]);
        assert_eq!(t.series["pieces"][0], Rgb::new(1.0, 0.0, 0.0));
        assert_eq!(t.games["flip"].specific["right"], Rgb::WHITE);
        assert_eq!(t.games["flip"].overrides["grid"], Rgb::grey(0.6));
    }

    #[test]
    fn writes_what_it_reads_exactly() {
        let t = Theme::from_toml(SAMPLE).unwrap();
        let again = Theme::from_toml(&t.to_toml()).unwrap();
        assert_eq!(again, t);
        // Awkward floats survive.
        let mut t = Theme {
            name: "x".into(),
            ..Theme::default()
        };
        let odd = Rgb::new(0.9 / 3.0, 1.0 / 1.5 * 0.9, f32::MIN_POSITIVE);
        t.roles.insert("ink".into(), odd);
        assert_eq!(Theme::from_toml(&t.to_toml()).unwrap().roles["ink"], odd);
    }

    #[test]
    fn rejects_bad_themes() {
        for (text, message) in [
            (
                "name = \"x\"\n[roles]\nbackgrond = \"red\"",
                "unknown role \"backgrond\"",
            ),
            (
                "name = \"x\"\n[series]\nhues = []",
                "unknown series \"hues\"",
            ),
            ("name = \"x\"\n[roles]\nink = \"nope\"", "roles.ink:"),
            (
                "name = \"x\"\n[roles]\nink = [2.0, 0.0, 0.0]",
                "components must be from 0 to 1",
            ),
            ("name = \"x\"\n[roles]\nink = \"#0000\"", "transparent"),
        ] {
            let err = Theme::from_toml(text).unwrap_err().to_string();
            assert!(err.contains(message), "{err}");
        }
        assert!(Theme::from_toml("nam = \"x\"").is_err());
    }

    #[test]
    fn game_keys_drop_spaces() {
        assert_eq!(game_key("Null Game"), "nullgame");
        assert_eq!(game_key("Flip"), "flip");
    }
}
