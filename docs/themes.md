# Writing a theme

A theme is a small TOML file that gives colours to the puzzles and to
the app around them. The app has three built in, Classic (the original
C colours), Light and Dark. You can add your own, usually by taking one
of these and changing a few colours.

## Where theme files go

Put the file in the `themes` folder of the app's own folder:

| OS | Folder |
|---|---|
| macOS | `~/Library/Application Support/puzzles-rs/themes/` |
| Linux | `~/.config/puzzles-rs/themes/` (or `$XDG_CONFIG_HOME/puzzles-rs/themes/`) |
| Windows | `%APPDATA%\puzzles-rs\themes\` |

Any file ending in `.toml` there is read; the file's name doesn't
matter, its `name` key does. Choose the theme in **Settings**, which
also shows that folder. The app watches it, so changes show as soon as
you save the file. Problems are listed in Settings, under the theme
choice: a file that can't be read, and keys that colour nothing,
such as a misspelt game or colour name (the theme is used without them).

## A first theme

```toml
name = "Aklo"          # as shown in Settings
extends = "Dark"       # take every colour you don't set from Dark
dark = true            # offered as a dark theme under "Follow system"

[roles]
done = "#d9de4a"       # satisfied clues, in every game that has them

[game.mosaic.overrides]
text_solved = "#d9de4a"  # the same, in Mosaic only
```

* `name` must not be a built-in theme's (names compare without case).
* `extends` names the theme to take missing colours from. Without it,
  the theme must set every colour itself, so start from one.
* `dark` puts the theme among the dark ones; leave it out for a light
  theme.

Colours are any CSS colour (`"#d9de4a"`, `"gold"`, `"rgb(217 222 74)"`;
not transparent) or three numbers from 0 to 1 (`[0.85, 0.87, 0.29]`).

## How the sections work

- **`[roles]`** sets shared colours: `background`, `highlight`,
  `lowlight` (the two sides of 3D bevels), `ink`, `grid`, `cursor`,
  `error`, `flash`, `clue`, `user_entry`, `pencil`, `done`, `hint`,
  `selected`, `faint` and `filled` (shaded, "black" squares). They apply
  to every game whose palette uses that role.
- **`[series]`** sets colour lists: `pieces` (Guess's pegs, and later
  Flood's, Same Game's and Map's regions), `numbers`, `binary` (Unruly's
  white and black squares) and `chains`.
- **`[game.<game>]`** sets a game's own colours: the ones marked "its
  own" in the tables below. For Mosaic these are `unmarked`, `blank`,
  `text_dark` and `text_light`. Themes that extend yours inherit them.
- **`[game.<game>.overrides]`** replaces any of a game's colours, by its
  name in the tables below. Mosaic's are `background`, `unmarked`,
  `grid`, `marked`, `blank`, `text_solved`, `error`, `cursor`,
  `text_dark` and `text_light`. Overrides apply only in this theme;
  themes that extend it don't inherit them.
- **`[ui]`** sets the app's own colours: `panel` (panels, toolbar and
  dialogs; by default the `background` role), `text` (by default `ink`)
  and `accent` (selections, highlighted controls and links; by default
  `selected`).

Game names are the game's name in lower case without spaces
(`mosaic`, `unruly`).

An override names a game's **colour**, not the role it comes from: in
Mosaic, `[game.mosaic.overrides] done = ...` colours nothing, because
Mosaic's colour that uses the `done` role is called `text_solved`.

## Dark themes built on light ones

When a dark theme takes a colour from a light theme it extends (a role,
or a game's own colour), it doesn't use it as it is: it mirrors its
lightness around the background, keeping its hue, so a light grey on a
light background becomes a dark grey on a dark one. Colours computed
from others (the "×", "mixed" and "bevel" rows below) are computed as in
the light theme and mirrored too. Series and anything the dark theme
sets itself are used as written. Dark, for example, extends Light and
sets every role and series itself.

## Each game's colours

"Its own" colours go under `[game.<game>]`; any colour can be replaced
under `[game.<game>.overrides]`. "× 0.5" is the other colour at half
its brightness; the bevel colours are the light and dark edges C draws
from a colour. A role or series colour changed in `[roles]` or
`[series]` changes every row that comes from it.


### Fifteen

| Colour | Default |
|---|---|
| `background` | role `background` |
| `text` | role `ink` |
| `highlight` | role `highlight` |
| `lowlight` | role `lowlight` |

### Flip

| Colour | Default |
|---|---|
| `background` | role `background` |
| `wrong` | `background` × 0.33333334 |
| `right` | its own, under `[game.flip]` |
| `grid` | role `grid` |
| `diag` | `grid` × 1 |
| `hint` | role `hint` |
| `cursor` | role `cursor` |

### Guess

| Colour | Default |
|---|---|
| `background` | role `background` |
| `frame` | role `ink` |
| `cursor` | role `cursor` |
| `markable` | its own, under `[game.guess]` |
| `hold` | its own, under `[game.guess]` |
| `empty` | `background` × 0.6666667 |
| `peg1` | series `pieces`, colour 1 |
| `peg2` | series `pieces`, colour 2 |
| `peg3` | series `pieces`, colour 3 |
| `peg4` | series `pieces`, colour 4 |
| `peg5` | series `pieces`, colour 5 |
| `peg6` | series `pieces`, colour 6 |
| `peg7` | series `pieces`, colour 7 |
| `peg8` | series `pieces`, colour 8 |
| `peg9` | series `pieces`, colour 9 |
| `peg10` | series `pieces`, colour 10 |
| `correct_place` | its own, under `[game.guess]` |
| `correct_colour` | its own, under `[game.guess]` |

### Mosaic

| Colour | Default |
|---|---|
| `background` | role `background` |
| `unmarked` | its own, under `[game.mosaic]` |
| `grid` | role `grid` |
| `marked` | role `filled` |
| `blank` | its own, under `[game.mosaic]` |
| `text_solved` | role `done` |
| `error` | role `error` |
| `cursor` | role `cursor` |
| `text_dark` | its own, under `[game.mosaic]` |
| `text_light` | its own, under `[game.mosaic]` |

### Unruly

| Colour | Default |
|---|---|
| `background` | role `background` |
| `grid` | role `grid` |
| `empty` | its own, under `[game.unruly]` |
| `zero` | series `binary`, colour 1 |
| `zero_highlight` | bevel highlight of `zero` |
| `zero_lowlight` | bevel lowlight of `zero` |
| `one` | series `binary`, colour 2 |
| `one_highlight` | bevel highlight of `one` |
| `one_lowlight` | bevel lowlight of `one` |
| `cursor` | role `cursor` |
| `error` | role `error` |


