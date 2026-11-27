//! The theme lint (D7 §7d and the themes decision): every built-in theme
//! has every role and every series Classic has (Classic grows series as
//! games need them), and the app's own themes keep foreground roles
//! readable against the background (WCAG contrast). Classic is C's
//! palette and is exempt from the contrast check.

use puzzles_core::palette::{ColourSpec, PaletteEntry, Role};
use puzzles_theme::{ROLES, Rgb, SERIES, ThemeSet, role_key, series_key};

/// WCAG relative luminance of an sRGB colour.
fn luminance(c: Rgb) -> f32 {
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio, from 1 to 21.
fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn role(themes: &ThemeSet, theme: &str, role: Role) -> Rgb {
    let palette = [PaletteEntry {
        name: "x",
        spec: ColourSpec::Role(role),
    }];
    themes.resolve(theme, "lint", &palette).expect("defined")[0]
}

#[test]
fn every_theme_has_every_role_and_classics_series() {
    let themes = ThemeSet::builtin();
    let classic = themes.get("classic").expect("built in");
    for name in themes.names() {
        for r in ROLES {
            let palette = [PaletteEntry {
                name: "x",
                spec: ColourSpec::Role(r),
            }];
            assert!(
                themes.resolve(&name, "lint", &palette).is_ok(),
                "{name}: no {}",
                role_key(r)
            );
        }
        let theme = themes.get(&name).expect("listed");
        for series in SERIES {
            let Some(needed) = classic.series.get(series_key(series)) else {
                continue;
            };
            let own = theme.series.get(series_key(series)).map_or(0, Vec::len);
            let has = if name.eq_ignore_ascii_case("classic") {
                needed.len()
            } else {
                own
            };
            assert!(
                has >= needed.len(),
                "{name}: series {} has {has} colours, Classic {}",
                series_key(series),
                needed.len()
            );
        }
    }
}

#[test]
fn dark_themes_set_every_role_themselves() {
    // So that only game-specific and derived colours are adapted.
    let themes = ThemeSet::builtin();
    for name in themes.names().into_iter().filter(|n| themes.is_dark(n)) {
        let theme = themes.get(&name).expect("listed");
        for r in ROLES {
            assert!(
                theme.roles.contains_key(role_key(r)),
                "{name}: {}",
                role_key(r)
            );
        }
    }
}

#[test]
fn foreground_roles_are_readable() {
    let themes = ThemeSet::builtin();
    for name in themes.names().into_iter().filter(|n| n != "Classic") {
        let bg = role(&themes, &name, Role::Background);
        for (r, needed) in [
            (Role::Ink, 4.5),
            (Role::Clue, 4.5),
            (Role::UserEntry, 4.5),
            (Role::Error, 3.0),
            (Role::Grid, 3.0),
        ] {
            let ratio = contrast(role(&themes, &name, r), bg);
            assert!(
                ratio >= needed,
                "{name}: {} contrast {ratio:.2} < {needed}",
                role_key(r)
            );
        }
        let ui = themes.ui_colours(&name).expect("complete");
        assert!(contrast(ui.text, ui.panel) >= 4.5, "{name}: UI text");
    }
}

#[test]
fn dark_adapts_what_it_takes_from_light() {
    // Own test themes: the built-in Dark tunes Flip by hand.
    let mut themes = ThemeSet::builtin();
    let light = "name = \"T-light\"\nextends = \"Light\"\n[game.test]\nright = \"#ffffff\"\n";
    let dark = "name = \"T-dark\"\nextends = \"T-light\"\ndark = true\n[roles]\nbackground = \"#1c1c1f\"\n";
    themes.insert(puzzles_theme::Theme::from_toml(light).unwrap());
    themes.insert(puzzles_theme::Theme::from_toml(dark).unwrap());
    let palette = [
        PaletteEntry {
            name: "Background",
            spec: ColourSpec::Role(Role::Background),
        },
        PaletteEntry {
            name: "Right",
            spec: ColourSpec::Specific("right"),
        },
        PaletteEntry {
            name: "Wrong",
            spec: ColourSpec::Derived {
                base: puzzles_core::palette::ColourRef(0),
                op: puzzles_core::palette::DeriveOp::Shade(1.0 / 3.0),
            },
        },
        PaletteEntry {
            name: "Peg",
            spec: ColourSpec::Series(puzzles_core::palette::Series::Pieces, 0),
        },
    ];
    let light = themes.resolve("t-light", "test", &palette).unwrap();
    let dark = themes.resolve("t-dark", "test", &palette).unwrap();
    let l = puzzles_theme::lightness;
    // A white Specific colour, lighter than the light background, ends up
    // darker than the dark one; a dark derived colour ends up light.
    assert!(l(light[1]) > l(light[0]) && l(dark[1]) < l(dark[0]));
    assert!(l(light[2]) < l(light[0]) && l(dark[2]) > l(dark[0]) + 0.3);
    // Series are never mirrored: T-dark has none, so Light's come as they
    // are.
    assert_eq!(dark[3], light[3]);
}
