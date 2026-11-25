# Analysis of the C codebase (`..\P2`)

Source analysed: `D:\P2`, a checkout of upstream
`https://git.tartarus.org/simon/puzzles.git` at commit `616da16`
(2026-09-22, "Mines: add a user preference for question marks").
2,328 commits of history. Licence: MIT (copyright Simon Tatham and
contributors). The licence notice must travel with the port.

The checkout also contains an in-tree MSVC Debug build (`Debug\*.exe`,
`Debug\core.lib`, `Debug\common.lib`, `*.vcxproj`). Those artifacts are
useful: they give us **reference binaries** for differential testing.

## 1. What the project is

Simon Tatham's Portable Puzzle Collection: 40 single-player logic puzzles
that share one engine. Each puzzle is a *back end*. A shared *mid-end*
handles undo/redo, animation, serialisation, presets and preferences.
Each platform supplies a *front end* (Win32, GTK, macOS, Emscripten/JS,
NestedVM/Java).

```
 ┌────────────── front end (per platform) ───────────────┐
 │ windows.c / gtk.c / osx.m / emcc.c+js / nestedvm.c    │
 │  - window, menus, dialogs, timers, input → buttons    │
 │  - implements `drawing_api` (vtable of 25 fn ptrs)    │
 └───────────────▲───────────────────────────┬───────────┘
                 │ midend_* API               │ drawing_api calls
 ┌───────────────┴──────── mid-end (midend.c) ▼───────────┐
 │ undo/redo chain of game_state, anim/flash timing,      │
 │ game-ID & seed parsing, save/load, presets, prefs      │
 └───────────────▲────────────────────────────────────────┘
                 │ `const struct game thegame` (vtable, ~55 members)
 ┌───────────────┴──── back ends (one .c per puzzle) ─────┐
 │ params / desc generation / solver / moves / redraw     │
 └───────────────▲────────────────────────────────────────┘
                 │
 ┌───────────────┴──── shared algorithms ("core") ────────┐
 │ random(SHA-1), dsf, tdq, tree234, latin, grid, ...     │
 └────────────────────────────────────────────────────────┘
```

## 2. Size

| Area | Files | Lines of C |
|---|---|---|
| Puzzle back ends (40 official) | `*.c` per game | ~95,000 |
| Core library + headers | see §4 | ~18,000 + ~5,300 headers |
| Front ends | windows.c 3,458 · gtk.c 4,789 · osx.m 1,932 · emcc.c 1,090 + 1,711 JS · nestedvm.c 485 | ~13,500 |
| Unfinished puzzles | `unfinished/` (group, numgame, path, separate, slide, sokoban) | not counted |
| Auxiliary test/gen tools | `auxiliary/` (15 programs) | small |
| **Total `*.c`/`*.h` at top level** | | **~136,000** |

The biggest back ends are mines (5,899), solo (5,789), galaxies (4,485),
loopy (3,912), dominosa (3,578), map (3,474), net (3,357) and bridges (3,350).

## 3. The central abstractions (`puzzles.h`)

### 3.1 `struct game`: the back-end vtable
Each puzzle defines opaque `game_params`, `game_state`, `game_ui` and
`game_drawstate` structs, and exports a `const struct game thegame` that
holds about 55 function pointers, flags and constants:

* **Params**: `default_params`, `fetch_preset`/`preset_menu`,
  `encode_params`/`decode_params` (a compact string such as `9x9de`),
  `dup/free_params`, `configure`/`custom_params` (a config dialog),
  `validate_params`.
* **Generation**: `new_desc(params, random_state*, char **aux, interactive)`
  returns the *game description* string. `aux` is private solution data.
  `validate_desc` checks a description.
* **State**: `new_game`, `dup_game`, `free_game`, `set_public_desc`.
* **Solving and text**: `solve` (returns a move string), `text_format`.
* **UI**: `new_ui`/`free_ui`/`encode_ui`/`decode_ui`, `get_prefs`/`set_prefs`,
  `changed_state`, `request_keys`, `current_key_label`.
* **Moves**: `interpret_move(state, ui, ds, x, y, button)` returns a
  **move string**, or one of the sentinels `MOVE_UI_UPDATE`,
  `MOVE_NO_EFFECT`, `MOVE_UNUSED`. Then
  `execute_move(state, move_string)` returns a new state, or NULL if the
  move is invalid.
* **Drawing**: `compute_size`, `set_size`, `colours`, `new/free_drawstate`,
  `redraw(dr, ds, oldstate, newstate, dir, ui, anim_time, flash_time)`,
  `anim_length`, `flash_length`, `get_cursor_location`, `status`.
* **Printing**: `print_size`, `print` (24 games support printing).
* **Misc**: `wants_statusbar`, `is_timed`/`timing_state`, `flags`
  (mouse-button priorities, `SOLVE_ANIMATES`, `REQUIRE_RBUTTON`, ...).

Unused capabilities are expressed as `false, NULL` pairs.
`nullgame.c` is the documented template.

Implication for Rust: this maps naturally onto a trait with associated
types (`Params`, `State`, `Ui`, `DrawState`), plus default methods for
the optional parts.

### 3.2 Strings are the wire format
Three string formats are the stable public interface, and porting them
exactly matters:

1. **Game ID** = `encoded_params:description` (for example `3x3:a2e9a5...`)
   and **random seed ID** = `params#seed`. Users share these, and they are
   guaranteed to reproduce the same puzzle on every platform.
2. **Move strings** (for example `R7,1,1` in Solo) are stored in the undo
   chain and in save files.
3. **Save files**: `KEY     :len:value` records
   (see `icons/*.sav`, 40 samples), with SAVEFILE / VERSION / GAME / PARAMS /
   CPARAMS / DESC / NSTATES / STATEPOS / MOVE / UI / TIME, and so on.
   Preferences use a similar format.

### 3.3 `drawing_api`: what the front end must implement
`draw_text`, `draw_rect`, `draw_line`, `draw_polygon`, `draw_circle`,
`draw_thick_line`, `draw_update`, `clip`/`unclip`, `start_draw`/`end_draw`,
`status_bar`, `blitter_new/free/save/load`, `text_fallback`, plus print
hooks: `begin_doc/page/puzzle`, `end_*`, `line_width`, `line_dotted`.
Colours are **indices** into a palette that the game returns from
`colours()` as RGB floats.

**Important constraint:** back ends draw **incrementally**. `redraw`
compares against its `game_drawstate` cache and repaints only the tiles
that changed. The front end must therefore keep a **persistent backing
canvas** between frames. A retained or immediate-mode GUI that clears
every frame cannot host the back ends directly. Blitters (save and restore
of pixel rectangles) also need a real pixel buffer.

### 3.4 Mid-end (`midend.c`, 3,233 lines)
It owns the `random_state`, the current params, an array of
`{state, movestr, movetype}` (the undo chain), the animation and flash
timers, the game-ID/seed parsing, serialisation and deserialisation
(callback-based reader and writer), the preset menu tree, preferences,
the status bar and the timer. It contains 50 `goto`s (mostly error
cleanup in deserialisation).

Special case: Mines calls `midend_supersede_game_desc` because the first
click rewrites the description. So the back end needs a handle back to
the mid-end.

## 4. Core library (shared algorithms)

| Module | Purpose | Used by |
|---|---|---|
| `random.c` | SHA-1-based PRNG: `random_new/bits/upto/copy`, state encode/decode. **Must be bit-exact** for seed compatibility. | all |
| `misc.c`, `malloc.c` | helpers: hex, obfuscation, colour mix, `shuffle`, `move_cursor`, `draw_text_outline`, `button2label`, ... | all |
| `dsf.c` | union-find, including the "min" and "flip" variants | 22 games |
| `tdq.c` | de-duplicating to-do queue | pearl |
| `tree234.c` | counted 2-3-4 tree (sorted set with index lookup) | flip, net, netslide, pegs, untangle, latin, loopgen, grid, penrose, spectre |
| `sort.c` | `arraysort` (qsort with a context argument) | dominosa |
| `combi.c` | combinations iterator | lightup |
| `divvy.c` | random polyomino partition of a rectangle | palisade, solo (jigsaw) |
| `findloop.c` | bridge and loop detection (callback + `void*` ctx) | bridges, dominosa, net, slant, tracks |
| `matching.c` | bipartite matching | tents, latin |
| `laydomino.c` | random domino tiling | dominosa, magnets |
| `latin.c` | Latin-square solver and generator framework | keen, singles, towers, unequal |
| `grid.c` (3,961) + `penrose*.c`, `hat.c`, `spectre.c` (+ ~3,500 lines of generated tables) | general planar grids: 19 tilings, including aperiodic Penrose, Hat and Spectre | loopy, pearl, **mines** |
| `loopgen.c` | random loop generation on a grid | loopy, pearl |
| `draw-poly.c` | software polygon rasteriser (fallback) | front ends |
| `drawing.c` | thin dispatch over `drawing_api` | all |
| `printing.c`, `ps.c` | multi-puzzle page layout, PostScript output | printing |

Header-level dependencies by game (from real `#include`s and API calls):

* **No extra deps**: blackbox, cube, fifteen, flood, guess, inertia,
  mosaic, samegame, sixteen, twiddle, unruly
* **tree234**: flip, netslide, pegs, untangle, net (+dsf, findloop)
* **dsf / findloop / divvy / matching / laydomino / combi**: bridges, dominosa,
  filling, galaxies, lightup, magnets, map, palisade, range, rect,
  signpost, slant, tents, tracks, undead, pattern
* **latin**: keen, singles, towers, unequal, solo (solo has its own
  solver, plus divvy)
* **grid + loopgen**: loopy, pearl, **mines** (upstream Mines now supports
  many grid shapes, including "wrapping" variants, and depends on `grid.c`)

## 5. Build and extra targets

CMake (`CMakeLists.txt`, `cmake/setup.cmake`, `cmake/platforms/*.cmake`):

* `core` / `common` static libraries, then one executable per puzzle.
  On Windows there is also an all-in-one `puzzles.exe`, and `net` builds as `netgame.exe`.
* `solver(<game>)` builds `<game>solver` CLIs with `-DSTANDALONE_SOLVER`
  for 27 games. Usage: `<game>solver [-g|-v] <game_id>` (grade or verbose).
* Other `#ifdef` variants of the same source file:
  `galaxiespicture`, `patternpicture` (`STANDALONE_PICTURE_GENERATOR`),
  `galaxieseditor`, `grapheditor` (`EDITOR`), `mineobfusc`,
  `pearlbench` (`STANDALONE_SOAK_TEST`), `polygon-test`.
* `fuzzpuzz`: loads a save file, redraws through a null drawing API,
  then re-serialises. This is the existing robustness harness.
* `auxiliary/`: unit-test style programs (tree234, latin, divvy, findloop,
  combi, sort, matching, penrose, hat, spectre) and table generators
  (`hatgen`, `spectre-gen`).
* `benchmark.sh` / `benchmark.pl`: `--test-solve --time-generation
  --generate 100 <preset>` for every preset. **These CLI flags exist only in
  the GTK front end.** `windows.c` has no command-line options, so the
  existing Windows build cannot emit seed → description directly.
* Docs: `puzzles.but` (Halibut source), `html/*.html` fragments for web pages.
* Icons: generated from `icons/*.sav` via Perl scripts.
* Assertions stay enabled in release builds (by design).

Verified locally: the `Debug\*solver.exe`, `pearlbench.exe` and
`fuzzpuzz.exe` reference binaries run.

## 6. C idioms that shape the Rust design

| C idiom | Where | Rust treatment |
|---|---|---|
| Opaque structs + vtable of fn pointers | `struct game`, `drawing_api` | traits with associated types; `dyn` only at the front-end boundary |
| Manual `snew`/`sfree`, `dup_*`/`free_*` | everywhere | `Clone`/`Drop`; most `free_*` disappear |
| Reference-counted shared immutable data inside `game_state` | 23 games (`refcount` fields) | `Rc<T>` (or `Arc` if we ever thread) |
| Pointer-linked graph (`grid_face/edge/dot` hold pointers to each other) | grid.c | arena + typed indices (`FaceId`, `EdgeId`, `DotId`). The header comment itself says indices would have worked. |
| `void *ctx` + callback | findloop, arraysort, serialise read/write | closures / `FnMut`, `io::Read`/`io::Write` |
| String sentinels (`MOVE_UI_UPDATE` …) | interpret_move | `enum MoveResult` |
| `const char *` error returns | validate_*, deserialise | `Result<_, E>` |
| `goto` (midend 50, solo 45, bridges 31, galaxies 29, magnets 28, …) | error cleanup, solver restart loops | `?`, labeled loops, helper functions |
| `sscanf` parsing (37 files) | params, desc, moves | hand-written parsers on `&str`/bytes |
| `#ifdef STANDALONE_SOLVER / EDITOR / ...` | 27+ files | separate binaries or cargo features; solver verbosity as a parameter, not a global |
| Mutable file-scope globals (~28, e.g. `solver_show_working`) | solvers | fields on a solver context |
| `int` arithmetic with `-1` sentinels and negative coordinates | everywhere | keep `i32` for coordinates in ported logic; convert at indexing |
| `qsort` with comparators | 10 games | `sort_unstable_by` is fine only if comparators are total (no ties). Must be verified, because tie order differs between libc qsort and Rust. |
| `tgmath.h` (float vs double chosen by type) | drawing; generators in rect.c, undead.c | keep `f32` vs `f64` exactly where it affects generation |
| `unsigned long` in `random_bits` (32-bit on Windows, 64-bit on Linux) | random.c | `u32`; only ≤32 bits are ever requested, so results match |

## 7. Risks

1. **Seed and game-ID compatibility.** Any divergence in PRNG use, iteration
   order, sort tie-breaking or float precision silently produces different
   puzzles. Mitigation: differential testing against the C code (see the plan).
2. **Incremental drawing model.** Picking the wrong GUI stack means
   rewriting every `redraw`. Mitigation: a persistent offscreen pixel canvas.
3. **Volume.** About 95k lines of back-end code. Porting is mechanical per
   game, but there is a lot of it, and the aperiodic tilings (hat/spectre
   tables) are large.
4. **Front-end features with no cross-platform equivalent**: Windows
   printing, WinHelp/CHM help.
5. **Mines** is now the most complex back end and depends on the full grid
   stack, so it cannot be one of the early, easy ports.
