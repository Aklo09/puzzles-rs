//! Colour palettes as semantic specs, with no RGB values (decision D7).
//!
//! A game declares its colours with [`palette!`](crate::palette!): each
//! entry names a shared [`Role`], a colour of a categorical [`Series`],
//! a colour [derived](DeriveOp) from another entry, or a game-specific
//! key. A theme (the `puzzles-theme` crate) turns the specs into RGB.
//! Drawing calls use the entries' indices, as C uses palette indices.
//!
//! The derivations cover what the C palettes compute from other colours;
//! the set grows as games are ported. Classic-theme parity tests check
//! that each game's specs reproduce C's `game_colours()` exactly.

/// A colour role shared by all games; every theme defines each one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The puzzle's background.
    Background,
    /// The light side of a 3D bevel (C's `game_mkhighlight` highlight of
    /// the default background). A theme role, so flat themes can set it
    /// close to `Background`.
    Highlight,
    /// The dark side of a 3D bevel (C's `game_mkhighlight` lowlight).
    Lowlight,
    /// Main foreground: lines, text, wires.
    Ink,
    /// Grid lines.
    Grid,
    /// The keyboard cursor.
    Cursor,
    /// Errors and clashes.
    Error,
    /// The completion flash.
    Flash,
    /// Given clues.
    Clue,
    /// Entries the player made.
    UserEntry,
    /// Pencil marks.
    Pencil,
    /// Satisfied or completed items.
    Done,
    /// Hints.
    Hint,
    /// Selected items.
    Selected,
    /// Faint or unknown items.
    Faint,
    /// A shaded ("black") cell: Pattern's filled squares, Light Up's,
    /// Range's and Singles' black cells, Mosaic's marked cells. Real
    /// black-and-white pieces stay game-specific.
    Filled,
}

/// A categorical series: distinguishable colours, the `n`-th of which a
/// palette entry can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Series {
    /// Coloured pieces or regions (Flood, Guess, Same Game, Map).
    Pieces,
    /// Colours for numbers (Mines).
    Numbers,
    /// Two contrasting colours (Unruly).
    Binary,
    /// Hues for chains (Signpost).
    Chains,
}

/// Another entry of the same palette, by index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColourRef(pub usize);

/// How a derived colour is computed from its base entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeriveOp {
    /// Each component times the factor, as in C's
    /// `ret[BORDER] = ret[BACKGROUND] * 0.5F`.
    Shade(f32),
    /// `base * (1 - t) + with * t` (C's `colour_mix`).
    #[doc(alias = "colour_mix")]
    Mix {
        /// The other entry.
        with: ColourRef,
        /// How much of it.
        t: f32,
    },
    /// The bevel highlight C's `game_mkhighlight_specific` makes from the
    /// base.
    Highlight,
    /// The bevel lowlight C's `game_mkhighlight_specific` makes from the
    /// base.
    Lowlight,
}

/// What one palette entry is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColourSpec {
    /// A shared role.
    Role(Role),
    /// The `n`-th colour of a series (from 0).
    Series(Series, u8),
    /// Computed from another entry.
    Derived {
        /// The entry it is computed from.
        base: ColourRef,
        /// How.
        op: DeriveOp,
    },
    /// A game-specific colour, looked up under this key in the theme's
    /// section for the game (for example Net's `"powered"`).
    Specific(&'static str),
}

/// Declares a game's palette: an enum of its colours, in palette order,
/// and the matching `PALETTE` spec table.
///
/// Entries are `Name => Role(role)`, `Name => Series(series, n)`,
/// `Name => Specific("key")`, or `Name => Derived(Base, op)` where `op`
/// is `Shade(f)`, `Mix(Other, t)`, `Highlight` or `Lowlight` and `Base`
/// and `Other` name earlier or later entries.
///
/// ```
/// puzzles_core::palette! {
///     /// Net's colours.
///     pub enum Col {
///         Background => Role(Background),
///         Wire => Role(Ink),
///         Powered => Specific("powered"),
///         Border => Derived(Background, Shade(0.5)),
///         Tint => Derived(Background, Mix(Powered, 0.25)),
///     }
/// }
///
/// use puzzles_core::palette::{ColourRef, ColourSpec, DeriveOp, Role};
/// assert_eq!(Col::PALETTE.len(), 5);
/// assert_eq!(Col::PALETTE[Col::Wire as usize], ColourSpec::Role(Role::Ink));
/// assert_eq!(
///     Col::PALETTE[Col::Border as usize],
///     ColourSpec::Derived { base: ColourRef(0), op: DeriveOp::Shade(0.5) }
/// );
/// let index: usize = Col::Powered.into();
/// assert_eq!(index, 2);
/// ```
#[macro_export]
macro_rules! palette {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $kind:ident $args:tt ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(usize)]
        #[allow(missing_docs, reason = "palette entries are named for what they colour")]
        #[allow(dead_code, reason = "every entry keeps its index, as in C, even if unused")]
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),*
        }

        impl $name {
            /// The colour specs, in palette order (decision D7).
            pub const PALETTE: &'static [$crate::palette::ColourSpec] = &[
                $( $crate::palette!(@spec $name $kind $args) ),*
            ];
        }

        impl ::core::convert::From<$name> for $crate::drawing::ColourIdx {
            fn from(colour: $name) -> Self {
                colour as usize
            }
        }
    };

    (@spec $name:ident Role ($role:ident)) => {
        $crate::palette::ColourSpec::Role($crate::palette::Role::$role)
    };
    (@spec $name:ident Series ($series:ident, $n:expr)) => {
        $crate::palette::ColourSpec::Series($crate::palette::Series::$series, $n)
    };
    (@spec $name:ident Specific ($key:literal)) => {
        $crate::palette::ColourSpec::Specific($key)
    };
    (@spec $name:ident Derived ($base:ident, Shade($f:expr))) => {
        $crate::palette::ColourSpec::Derived {
            base: $crate::palette::ColourRef($name::$base as usize),
            op: $crate::palette::DeriveOp::Shade($f),
        }
    };
    (@spec $name:ident Derived ($base:ident, Mix($with:ident, $t:expr))) => {
        $crate::palette::ColourSpec::Derived {
            base: $crate::palette::ColourRef($name::$base as usize),
            op: $crate::palette::DeriveOp::Mix {
                with: $crate::palette::ColourRef($name::$with as usize),
                t: $t,
            },
        }
    };
    (@spec $name:ident Derived ($base:ident, Highlight)) => {
        $crate::palette::ColourSpec::Derived {
            base: $crate::palette::ColourRef($name::$base as usize),
            op: $crate::palette::DeriveOp::Highlight,
        }
    };
    (@spec $name:ident Derived ($base:ident, Lowlight)) => {
        $crate::palette::ColourSpec::Derived {
            base: $crate::palette::ColourRef($name::$base as usize),
            op: $crate::palette::DeriveOp::Lowlight,
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::palette! {
        /// Every kind of entry.
        enum Test {
            Background => Role(Background),
            Piece0 => Series(Pieces, 0),
            Piece1 => Series(Pieces, 1),
            Gem => Specific("gem"),
            Border => Derived(Background, Shade(0.5)),
            Hi => Derived(Gem, Highlight),
            Lo => Derived(Gem, Lowlight),
            Mixed => Derived(Background, Mix(Later, 0.75)),
            Later => Role(Error),
        }
    }

    #[test]
    fn specs_are_in_declaration_order() {
        use ColourSpec::{Derived, Role as R, Series as S, Specific};
        assert_eq!(
            Test::PALETTE,
            [
                R(Role::Background),
                S(Series::Pieces, 0),
                S(Series::Pieces, 1),
                Specific("gem"),
                Derived {
                    base: ColourRef(0),
                    op: DeriveOp::Shade(0.5)
                },
                Derived {
                    base: ColourRef(3),
                    op: DeriveOp::Highlight
                },
                Derived {
                    base: ColourRef(3),
                    op: DeriveOp::Lowlight
                },
                Derived {
                    base: ColourRef(0),
                    op: DeriveOp::Mix {
                        with: ColourRef(8),
                        t: 0.75
                    }
                },
                R(Role::Error),
            ]
        );
        assert_eq!(Test::Later as usize, 8);
        assert_eq!(usize::from(Test::Gem), 3);
    }
}
