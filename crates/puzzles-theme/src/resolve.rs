//! Turning a game's palette into colours with a theme.
//!
//! For each palette entry, in order:
//! 1. an override for the entry in the theme itself (not in the themes it
//!    extends: Classic decision (b));
//! 2. otherwise by its spec: a role, series or `Specific` value from the
//!    theme or, if it has none, the theme it extends, and so on; a derived
//!    entry is computed from its base entry (resolved the same way).
//!
//! Dark themes will adapt `Specific` values they inherit from a light
//! parent (`OKLab` lightness), with the dark themes in Phase 3.

use std::collections::BTreeMap;

use puzzles_core::palette::{ColourSpec, DeriveOp, PaletteEntry, Role, Series};

use crate::colour::{Rgb, bevel, mix};
use crate::model::{Theme, game_key, role_key, series_key};

/// Why a palette couldn't be resolved.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// No theme of that name.
    #[error("no theme named {0:?}")]
    UnknownTheme(String),
    /// A theme extends itself, directly or not.
    #[error("theme {0:?} extends itself")]
    ExtendsCycle(String),
    /// Neither the theme nor its parents define a role.
    #[error("theme {theme:?} has no {role:?} colour")]
    MissingRole {
        /// The theme.
        theme: String,
        /// The role.
        role: Role,
    },
    /// Neither the theme nor its parents have that many series colours.
    #[error("theme {theme:?} has no colour {index} in series {series:?}")]
    MissingSeries {
        /// The theme.
        theme: String,
        /// The series.
        series: Series,
        /// The colour asked for.
        index: u8,
    },
    /// Neither the theme nor its parents define a game's colour.
    #[error("theme {theme:?} has no colour {key:?} for {game}")]
    MissingSpecific {
        /// The theme.
        theme: String,
        /// The game key.
        game: String,
        /// The colour's key.
        key: String,
    },
    /// A palette entry derives from itself, or from no entry.
    #[error("palette entry {0} derives from itself or from nothing")]
    BadDerivation(usize),
}

/// Themes by name, for resolving `extends`.
#[derive(Clone, Debug, Default)]
pub struct ThemeSet {
    themes: BTreeMap<String, Theme>,
}

/// The themes built into the collection.
const BUILT_IN: &[&str] = &[include_str!("../themes/classic.toml")];

impl ThemeSet {
    /// The built-in themes: Classic, reproducing C's colours.
    ///
    /// # Panics
    /// If a built-in theme file is broken (tests check them).
    #[must_use]
    pub fn builtin() -> Self {
        let mut set = Self::default();
        for text in BUILT_IN {
            set.insert(Theme::from_toml(text).expect("built-in themes are valid"));
        }
        set
    }

    /// Adds a theme, replacing one of the same name (names compare
    /// without case).
    pub fn insert(&mut self, theme: Theme) {
        self.themes.insert(theme.name.to_lowercase(), theme);
    }

    /// The theme with this name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Theme> {
        self.themes.get(&name.to_lowercase())
    }

    /// The theme and the ones it extends, nearest first.
    fn chain(&self, name: &str) -> Result<Vec<&Theme>, ResolveError> {
        let mut chain: Vec<&Theme> = Vec::new();
        let mut next = Some(name);
        while let Some(name) = next {
            let theme = self
                .get(name)
                .ok_or_else(|| ResolveError::UnknownTheme(name.to_owned()))?;
            if chain
                .iter()
                .any(|t| t.name.eq_ignore_ascii_case(&theme.name))
            {
                return Err(ResolveError::ExtendsCycle(theme.name.clone()));
            }
            chain.push(theme);
            next = theme.extends.as_deref();
        }
        Ok(chain)
    }

    /// The colours of `palette` for `game_name` (a game's `NAME`) in the
    /// theme named `theme`, in palette order.
    ///
    /// # Errors
    /// An unknown theme or a cycle of `extends`; a colour the theme
    /// chain doesn't define; a palette entry deriving from itself.
    pub fn resolve(
        &self,
        theme: &str,
        game_name: &str,
        palette: &[PaletteEntry],
    ) -> Result<Vec<Rgb>, ResolveError> {
        let chain = self.chain(theme)?;
        let resolver = Resolver {
            chain: &chain,
            game: game_key(game_name),
            palette,
        };
        let mut done = vec![None; palette.len()];
        (0..palette.len())
            .map(|i| resolver.entry(i, &mut done, &mut Vec::new()))
            .collect()
    }
}

struct Resolver<'a> {
    chain: &'a [&'a Theme],
    game: String,
    palette: &'a [PaletteEntry],
}

impl Resolver<'_> {
    fn name(&self) -> String {
        self.chain[0].name.clone()
    }

    fn entry(
        &self,
        i: usize,
        done: &mut [Option<Rgb>],
        visiting: &mut Vec<usize>,
    ) -> Result<Rgb, ResolveError> {
        if let Some(c) = done.get(i).copied().flatten() {
            return Ok(c);
        }
        let entry = self.palette.get(i).ok_or(ResolveError::BadDerivation(i))?;
        if visiting.contains(&i) {
            return Err(ResolveError::BadDerivation(i));
        }
        visiting.push(i);

        let own = self.chain[0]
            .games
            .get(&self.game)
            .and_then(|g| g.overrides.get(&entry.key()));
        let colour = match (own, entry.spec) {
            (Some(&c), _) => c,
            (None, ColourSpec::Role(role)) => self.role(role)?,
            (None, ColourSpec::Series(series, n)) => self.series(series, n)?,
            (None, ColourSpec::Specific(key)) => self.specific(key)?,
            (None, ColourSpec::Derived { base, op }) => {
                let base = self.entry(base.0, done, visiting)?;
                match op {
                    DeriveOp::Shade(f) => base.map(|c| c * f),
                    DeriveOp::Mix { with, t } => mix(base, self.entry(with.0, done, visiting)?, t),
                    DeriveOp::Highlight => bevel(base).highlight,
                    DeriveOp::Lowlight => bevel(base).lowlight,
                }
            }
        };

        visiting.pop();
        done[i] = Some(colour);
        Ok(colour)
    }

    fn role(&self, role: Role) -> Result<Rgb, ResolveError> {
        self.chain
            .iter()
            .find_map(|t| t.roles.get(role_key(role)).copied())
            .ok_or(ResolveError::MissingRole {
                theme: self.name(),
                role,
            })
    }

    fn series(&self, series: Series, n: u8) -> Result<Rgb, ResolveError> {
        self.chain
            .iter()
            .find_map(|t| t.series.get(series_key(series)))
            .and_then(|colours| colours.get(usize::from(n)).copied())
            .ok_or(ResolveError::MissingSeries {
                theme: self.name(),
                series,
                index: n,
            })
    }

    fn specific(&self, key: &str) -> Result<Rgb, ResolveError> {
        self.chain
            .iter()
            .find_map(|t| t.games.get(&self.game)?.specific.get(key).copied())
            .ok_or_else(|| ResolveError::MissingSpecific {
                theme: self.name(),
                game: self.game.clone(),
                key: key.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    puzzles_core::palette! {
        enum Test {
            Background => Role(Background),
            Wire => Role(Ink),
            Powered => Specific("powered"),
            Border => Derived(Background, Shade(0.5)),
            Tint => Derived(Background, Mix(Powered, 0.25)),
            Piece => Series(Pieces, 1),
            Raised => Derived(Background, Highlight),
        }
    }

    fn set() -> ThemeSet {
        let mut set = ThemeSet::default();
        set.insert(
            Theme::from_toml(
                r#"
name = "Base"
[roles]
background = [0.8, 0.8, 0.8]
ink = "black"
[series]
pieces = ["red", "lime"]
[game.test]
powered = [0.0, 1.0, 1.0]
[game.test.overrides]
wire = [0.25, 0.25, 0.25]
"#,
            )
            .unwrap(),
        );
        set.insert(
            Theme::from_toml(
                r#"
name = "Child"
extends = "base"
[roles]
background = [0.2, 0.2, 0.2]
"#,
            )
            .unwrap(),
        );
        set
    }

    #[test]
    fn resolves_every_kind_of_entry() {
        let c = set().resolve("Base", "Test", Test::PALETTE).unwrap();
        assert_eq!(c[0], Rgb::grey(0.8));
        assert_eq!(c[1], Rgb::grey(0.25), "the override");
        assert_eq!(c[2], Rgb::new(0.0, 1.0, 1.0));
        assert_eq!(c[3], Rgb::grey(0.4));
        assert_eq!(c[4], mix(Rgb::grey(0.8), Rgb::new(0.0, 1.0, 1.0), 0.25));
        assert_eq!(c[5], Rgb::new(0.0, 1.0, 0.0));
        assert_eq!(c[6], bevel(Rgb::grey(0.8)).highlight);
    }

    #[test]
    fn children_inherit_values_but_not_overrides() {
        let c = set().resolve("child", "Test", Test::PALETTE).unwrap();
        assert_eq!(c[0], Rgb::grey(0.2), "own role");
        assert_eq!(c[1], Rgb::BLACK, "the parent's role, not its override");
        assert_eq!(c[2], Rgb::new(0.0, 1.0, 1.0), "inherited Specific key");
        assert_eq!(c[3], Rgb::grey(0.1), "derived from the child's background");
    }

    #[test]
    fn reports_what_is_missing() {
        let mut set = set();
        assert_eq!(
            set.resolve("nope", "Test", Test::PALETTE),
            Err(ResolveError::UnknownTheme("nope".into()))
        );
        assert!(matches!(
            set.resolve("base", "Other", Test::PALETTE),
            Err(ResolveError::MissingSpecific { .. })
        ));
        set.insert(Theme::from_toml("name = \"Loop\"\nextends = \"loop\"").unwrap());
        assert_eq!(
            set.resolve("loop", "Test", Test::PALETTE),
            Err(ResolveError::ExtendsCycle("Loop".into()))
        );
    }

    #[test]
    fn self_derivation_is_an_error() {
        let palette = [PaletteEntry {
            name: "Odd",
            spec: ColourSpec::Derived {
                base: puzzles_core::palette::ColourRef(0),
                op: DeriveOp::Shade(1.0),
            },
        }];
        assert_eq!(
            set().resolve("base", "Test", &palette),
            Err(ResolveError::BadDerivation(0))
        );
    }

    #[test]
    fn classic_is_built_in_and_defines_every_role() {
        let set = ThemeSet::builtin();
        let classic = set.get("classic").expect("built in");
        for role in crate::model::ROLES {
            assert!(classic.roles.contains_key(role_key(role)), "{role:?}");
        }
    }
}
