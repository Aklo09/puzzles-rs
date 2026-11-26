# Porting plan: Simon Tatham's puzzles (C) → puzzles-rs (Rust)

Companion to [c-codebase-analysis.md](c-codebase-analysis.md). Every
**[DECISION]** section was reviewed and decided on 2026-11-25. Each one
records the outcome and the rejected alternatives, and the log below
summarises them all.

## Decision log
| # | Topic | Status | Outcome |
|---|---|---|---|
| D1 | Compatibility with C | **Decided** 2026-11-25 | Two levels: engine I/O always identical to C; drawing identical only as a porting gate |
| D2 | Virtual workspace | **Decided** 2026-11-25 | Root becomes a virtual workspace; `src/main.rs` removed |
| D3 | Games crate granularity | **Decided** 2026-11-25 | One `puzzles-games` crate, module + cargo feature per game |
| D4 | Move representation | **Decided** 2026-11-25 | Typed moves in the `Game` trait; byte-identical text encoding for saves |
| D5 | Dependencies | **Decided** 2026-11-25 | Prefer established crates over reimplementing; the only exception is code whose output must match C (D1 level 1), where a crate is allowed only for uniquely determined results proven by oracle tests. `sha1_smol` for SHA-1 |
| D6 | GUI stack | **Decided** 2026-11-25 | Puzzle canvas: `tiny-skia` pixmap (toolkit-independent). UI toolkit: `egui`/`eframe` |
| D7 | Theme architecture | **Decided** 2026-11-25 | Semantic palette specs, RGB only in TOML themes; `Filled` + bevel (`Highlight`/`Lowlight`) roles; one theme file styles canvas and app UI; files + hot reload now, in-app editor after release 1 (Phase 9) |
| — | Release-1 features | **Decided** 2026-11-25 | Foundation (§4.1–4.3) + one extra: the rules and controls side panel. Everything else in §4.4 comes after release 1 |
| — | Licensing | Default (MIT terms) | Keep `LICENCE` with all original copyright holders; add a Rust-port attribution line |
| — | Optional C features | **Decided** 2026-11-25 | Printing (PostScript/PDF) and a web/wasm build come after release 1. Puzzle editors, picture generators and `unfinished/` puzzles are out of scope |
| — | C source for oracle tests | **Decided** 2026-11-25 | Git submodule of upstream at `third_party/puzzles-c`, pinned to `616da16` |
| — | Fonts | **Decided** 2026-11-25 | Inter + JetBrains Mono (OFL), bundled, for both the puzzle canvas and the app UI; Noto Sans fallback added with localisation |
| — | CI script | **Decided** 2026-11-25 | `cargo xtask ci` (Rust `xtask` crate, no extra tools); GitHub Actions runs it |
| — | CI frequency | **Decided** 2026-11-25 | PRs run Ubuntu + Windows; macOS on demand (manual run or a local Mac); docs-only PRs skip CI; no rerun after merge to `main` (see §6) |
| — | Porting style | **Decided** 2026-11-25 | Map C constructs to idiomatic Rust (types + traits, `Clone`/`Drop`, `Option`/`Result`, closures) with identical observable behaviour; idiomatic names with `#[doc(alias)]` to the C name. First applied to `dsf`: three types sharing a trait (see `CLAUDE.md`) |
| — | Latin solver API | **Decided** 2026-11-25 | Games plug in through a `LatinRules` trait (`deduce` per difficulty level, `is_valid`, `branch` for a fresh per-guess context, `None` = shared, replacing `ctxnew`/`ctxfree`); `Deduction` and `Outcome` enums replace C's int codes and `diff_impossible`/`ambiguous`/`unfinished`; solver reasoning text is ported now through a `SolverLog` (verbosity + recursion depth, replacing the `solver_show_working`/`solver_recurse_depth` globals), text parity checked in Phase 6. Rejected: a struct of closures; C's int codes; adding messages only in Phase 6 |
| — | tree234 container | **Decided** 2026-11-25 | `sorted-vec`'s `SortedSet` (re-exported as `puzzles_core::sorted_set`, plus a `find_rel` helper for `findrel234`/`findrelpos234`) where C uses positions; std `BTreeSet`/`BTreeMap` elsewhere. Rejected: `indexset` (O(log n), but far less established), a local Vec wrapper (D5 prefers a crate) |
| — | Mid-end drawing | **Decided** 2026-11-25 | Deferred redraws: the mid-end is a state machine that never draws by itself; events mark it dirty and the front end calls `redraw(&mut dyn DrawingApi)` (once per frame). The draw state is built in `new_game`/`size` as in C, without a drawing: blitters are plain handles a game creates freely, and the canvas allocates their pixels on first use. C's `interactive` flag (`drawing != NULL`) is set when the mid-end is created. Game logic is unaffected (checked: `interpret_move` reads only layout fields set in `new_drawstate`/`set_size`); mid-end-level draw sequences may differ from C (UI), game-level draw traces still match (D1 level 2). Rejected: drawing passed per call; mid-end owning the canvas |
| — | Mid-end ↔ front end | **Decided** 2026-11-25 | No callbacks: the seed is passed to `Midend::new`, and the front end reads `wants_timer()`, `is_dirty()` and a take-once game-ID-changed flag after each call (replacing `activate_timer`/`deactivate_timer`, `get_random_seed`, `midend_request_id_changes`). Rejected: a `FrontEnd` callback trait (callbacks can't use the borrowed mid-end, so they'd only set flags) |
| — | Phase 2 order | **Decided** 2026-11-25 | One PR each: core types (Button, config, presets, `palette!`, `DrawingApi` + blitter handles + `Drawing` helpers, rest of `misc.c`); `Game` trait + nullgame + Flip (game-level parity); mid-end + `dyn Puzzle` + save files and prefs; theme crate + Classic; CLI; Fifteen; Guess. Rejected: mid-end first, shaped by nullgame only |
| — | `Game` trait details | **Decided** 2026-11-26 | (a) `new_game` and `execute_move` take `&mut GameCtx`, which collects mid-end requests (Mines' `supersede_desc(desc, privdesc)`) for the mid-end to apply after the call; other games ignore it. Rejected: a take-once poll hook on the state (needs a `RefCell` in Mines' layout); a wrapper return type. (b) Capabilities are `bool` consts defaulting to false (`CAN_CONFIGURE`, `CAN_SOLVE`, `CAN_FORMAT_AS_TEXT_EVER`, `WANTS_STATUSBAR`, `IS_TIMED`; `CAN_PRINT`/`CAN_PRINT_IN_COLOUR` join with printing in Phase 9); C's `flags` int is `const FLAGS: GameFlags` (bitflags); a generic conformance test checks each const against its method. Rejected: one bitflags for everything. (c) nullgame lives in `puzzles-games` behind a `nullgame` feature, not in `default` or the registry. Rejected: a `puzzles_core::testing` module. (d) `decode_move` returns `Result<Move, MoveParseError>`, a unit error: a rejected move is an error for developers (tests, logs, tools), while users still see the mid-end's generic C message. Rejected: `Option` (same information, but not typed as an error); an error with a reason per game (messages C never had, which users wouldn't see) |
| — | Threading | **Decided** 2026-11-26 | The desktop app runs puzzle generation on one worker thread ("Generating…" overlay, cancel discards the result; the ported generators are unchanged); moves, undo, solving, redraw and animation stay on the UI thread. Game states stay single-threaded with `Rc` for shared parts; only `Params: Send + 'static` is required, since `new_desc` takes params + seed and returns a string. The web build can generate in place. Rejected: `Arc` + `Send + Sync` states for off-thread solving (solving is fast, mostly from `aux`; revisit per game if one proves slow) |

---

## 1. Goals and non-goals

**Goals**
* Port the engine (core algorithms + mid-end) and all 40 official puzzles.
* **Cross-platform: Linux, macOS and Windows are all first-class.** Every
  platform gets CI, and there is no platform-specific code outside a few
  thin, `cfg`-gated spots.
* **A new, modern desktop front end.** It deliberately does *not* mimic the
  original front ends. It should offer what users expect today: themes
  (light, dark, follow-system, accessible palettes), fluid resizing and
  HiDPI, full-screen and immersive modes, a game library, session restore,
  and so on. See §4.
* Keep the C project's **engine-level** behaviour: game IDs, random
  seeds, move strings, save files and preference files. (The UI is free
  to change; the puzzles are not.)
* Use idiomatic, safe Rust (`#![forbid(unsafe_code)]` outside FFI test code).

**After release 1 (planned, Phase 9)**
* Printing: port `printing.c`/`ps.c` to `.ps`, plus PDF output (24 games support printing).
* A web/wasm build of the desktop app (eframe), replacing the C
  Emscripten/JS front end.

**Non-goals (out of scope)**
* Porting the original platform front ends (`windows.c`, `gtk.c`, `osx.m`,
  `emcc.c`, `nestedvm.c`, KaiOS) or reproducing their look and feel.
* `unfinished/` puzzles.
* Puzzle editors (`galaxieseditor`, `grapheditor`) and picture generators
  (`galaxiespicture`, `patternpicture`). The `#ifdef EDITOR` /
  `STANDALONE_PICTURE_GENERATOR` code paths are not ported.
* `mineobfusc` and `pearlbench` stand-alone tools (`puzzles-cli` covers
  benchmarking).
* Halibut docs toolchain and the icon generation pipeline. Icons and
  thumbnails are rendered by our own engine instead (§4), and rules text is
  converted once to Markdown (Phase 7).
* Native OS print dialogs (printing produces files).

### [DECISION 1] Compatibility with the C version: two levels (**decided**)

**Level 1: permanent, never broken.** Everything a user can exchange
with, or carry over from, the C version:
* `params#seed` produces the same description (and aux), and game IDs
  are accepted the same way;
* solver output and `text_format`;
* the text form of moves, save files and preference files: C files load,
  replay and re-serialise **byte-identically**.

This makes the C code a test oracle: every generator, solver and
codec can be checked mechanically, which is the main thing that keeps
a 95k-line port honest. Consequence: generators and solvers are ported
**faithfully** (same algorithm, same iteration order, same PRNG calls).
We modernise data ownership and error handling, not algorithms.
Idiomatic refactors happen behind the differential tests.

**Level 2: a porting gate, then free.** The sequence of draw calls
(with palette indices) that `redraw` emits must match C **until the
game is accepted as ported**. That proves the drawing code was
transliterated correctly. Afterwards the draw-trace oracle test for
that game is retired and becomes an ordinary snapshot, so we can then
improve the puzzle's graphics (anti-aliasing, nicer shapes, animation)
by deliberately updating snapshots.

Everything above the engine (window, layout, theme colours, menus,
chrome animations) is the new front end's and was never constrained.

Rejected alternatives: *fully exact forever* (blocks graphics
improvements); *equivalent but not exact* (no oracle, and existing game
IDs and saves break).

---

## 2. Repository / workspace layout

### [DECISION 2] Turn the root into a virtual workspace (**decided**)
The original root package (hello world) becomes a virtual workspace.
`src/main.rs` is removed.

```
puzzles-rs/
├─ Cargo.toml                  # [workspace], shared lints, profiles, deps
├─ rust-toolchain.toml         # pin stable (1.100) + clippy, rustfmt
├─ docs/
├─ crates/
│  ├─ puzzles-core/            # lib: everything platform-independent except the games
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ random.rs          # PRNG on top of sha1_smol (bit-exact with C)
│  │     ├─ misc.rs            # obfuscation, lenient hex, n_times_root_k, env_bool
│  │     ├─ dsf.rs, todo_queue.rs (tdq.c), divvy.rs,   # ported (D5 exception)
│  │     ├─ findloop.rs,                               # ported (D5 exception)
│  │     ├─ matching.rs, laydomino.rs, latin.rs        # ported (D5 exception)
│  │     ├─ sort.rs            # arraysort's heapsort (ported: tie order)
│  │     ├─ sorted_set.rs      # tree234 → sorted-vec SortedSet + find_rel
│  │     ├─ solver_log.rs      # SolverLog (C's solver_show_working)
│  │     │                     # combi → itertools (no wrapper)
│  │     ├─ grid/              # grid.rs, penrose.rs, hat.rs, spectre.rs, tables
│  │     ├─ loopgen.rs
│  │     ├─ game.rs            # `Game` trait, Button, MoveResult, ConfigItem, PresetMenu
│  │     ├─ palette.rs         # Role, Series, ColourSpec, `palette!` macro (no RGB)
│  │     ├─ drawing.rs         # `DrawingApi` trait + `Drawing` helper wrapper
│  │     ├─ midend/            # Midend<G>, undo chain, serialise, prefs, game-id parsing
│  │     ├─ printing.rs, ps.rs
│  │     └─ erased.rs          # object-safe `dyn Puzzle` wrapper for front ends
│  ├─ puzzles-theme/           # lib: Theme model, TOML loading, resolver, OKLab adaptation,
│  │                           #   built-in themes (themes/*.toml, incl. generated classic.toml)
│  ├─ puzzles-games/           # lib: one module per puzzle + registry `GAMES`
│  │  └─ src/{blackbox.rs, bridges.rs, ..., untangle.rs, lib.rs}
│  ├─ puzzles-desktop/         # bin `puzzles`: GUI front end (all games in one exe)
│  │  └─ src/
│  │     ├─ app/               # UI-toolkit-agnostic app state: session, settings,
│  │     │                     #   theme resolution, layout/zoom maths, persistence
│  │     ├─ canvas/            # tiny-skia `DrawingApi` impl, text, blitters, DPR
│  │     └─ ui/                # egui: library, toolbar, overlays, dialogs, settings
│  ├─ puzzles-cli/             # bin `puzzles-cli`: generate / solve / grade / test-solve /
│  │                           #   benchmark / roundtrip (replaces *solver.exe, fuzzpuzz, benchmark.sh)
│  └─ c-oracle/                # publish=false, test-only: builds third_party/puzzles-c via `cc`,
│                              #   exposes them over FFI for differential tests
├─ third_party/
│  └─ puzzles-c/               # git submodule: upstream puzzles.git pinned to 616da16
├─ assets/fonts/               # Inter, JetBrains Mono (+ OFL licence texts)
└─ tests/corpus/               # copied icons/*.sav + generated golden data
```

### [DECISION 3] Granularity of the games crate (**decided**)
**One `puzzles-games` crate, one module per game, a cargo
feature per game (all on by default).** It's simple, and the registry is
a single `static GAMES: &[GameEntry]` instead of the generated
`generated-games.h`.

Alternative: one crate per game (40 crates). That gives better parallel
compilation and enforces isolation, but means more `Cargo.toml`
boilerplate. We can split later if compile times hurt. The module
boundary already gives most of the isolation.

---

## 3. Core design

### 3.1 The `Game` trait (replaces `struct game`)
Static dispatch inside each game and inside `Midend<G>`. Dynamic dispatch
only at the front-end boundary, where heterogeneous games are needed
(per rust-best-practices ch. 6: "box at the boundary").

```rust
// puzzles-core/src/game.rs (abridged; methods with a body are optional)
pub trait Game: 'static {
    type Params: Clone + PartialEq + fmt::Debug + Send + 'static;  // generated on a worker thread
    type State: Clone;            // shared immutable parts inside as Rc<…>
    type Ui;
    type DrawState;
    type Move: Clone + PartialEq + fmt::Debug;   // D4: typed moves

    const NAME: &'static str;
    const PREFERRED_TILESIZE: i32;
    const PALETTE: &'static [ColourSpec];  // §4.2 / D7: named colours only, no RGB
    const FLAGS: GameFlags = GameFlags::empty();  // SOLVE_ANIMATES, REQUIRE_RBUTTON, BUTTON_BEATS, ...
    const CAN_CONFIGURE: bool = false;     // also CAN_SOLVE, CAN_FORMAT_AS_TEXT_EVER,
    // ...                                 //   WANTS_STATUSBAR, IS_TIMED

    // params
    fn default_params() -> Self::Params;
    fn presets() -> PresetMenu<Self::Params> { PresetMenu::new() }
    fn decode_params(params: &mut Self::Params, s: &str);
    fn encode_params(params: &Self::Params, full: bool) -> String;
    fn configure(p: &Self::Params) -> Option<Vec<ConfigItem>> { None }
    fn custom_params(cfg: &[ConfigItem]) -> Result<Self::Params, GameError> { Err(GameError::unsupported()) }
    fn validate_params(p: &Self::Params, full: bool) -> Result<(), GameError>;

    // generation & state
    fn new_desc(p: &Self::Params, rng: &mut RandomState, interactive: bool) -> NewDesc;  // desc + aux
    fn validate_desc(p: &Self::Params, desc: &str) -> Result<(), GameError>;
    fn new_game(ctx: &mut GameCtx, p: &Self::Params, desc: &str) -> Self::State;
    fn set_public_desc(s: &mut Self::State, public_desc: &str) {}            // Mines
    fn solve(orig: &Self::State, cur: &Self::State, aux: Option<&str>)
        -> Result<Self::Move, GameError> { Err(GameError::unsupported()) }
    fn can_format_as_text_now(p: &Self::Params) -> bool { Self::CAN_FORMAT_AS_TEXT_EVER }
    fn text_format(s: &Self::State) -> Option<String> { None }
    fn status(s: &Self::State) -> GameStatus;   // Ongoing | Won | Lost
    fn timing_state(s: &Self::State, ui: &mut Self::Ui) -> bool { true }

    // UI and moves
    fn new_ui(s: &Self::State) -> Self::Ui;
    fn prefs(ui: &Self::Ui) -> Vec<ConfigItem> { .. }   // also set_prefs, encode_ui, decode_ui,
    // ...                                             //   request_keys, changed_state, current_key_label
    fn interpret_move(s: &Self::State, ui: &mut Self::Ui, ds: &Self::DrawState,
                      x: i32, y: i32, button: Button) -> MoveResult<Self::Move>;
    fn execute_move(ctx: &mut GameCtx, s: &Self::State, mv: &Self::Move) -> Option<Self::State>;
    fn encode_move(mv: &Self::Move, out: &mut String);                     // D4
    fn decode_move(s: &Self::State, text: &str) -> Result<Self::Move, MoveParseError>;  // D4, untrusted

    // drawing (draw state built without a drawing: mid-end drawing decision)
    fn compute_size(p: &Self::Params, tilesize: i32, ui: &Self::Ui) -> (i32, i32);
    fn new_drawstate(s: &Self::State) -> Self::DrawState;
    fn set_size(ds: &mut Self::DrawState, p: &Self::Params, tilesize: i32);
    fn redraw(dr: &mut Drawing<'_>, ds: &mut Self::DrawState, old: Option<&Self::State>,
              new: &Self::State, dir: i32, ui: &Self::Ui, anim_time: f32, flash_time: f32);
    fn anim_length(old: &Self::State, new: &Self::State, dir: i32, ui: &mut Self::Ui) -> f32 { 0.0 }
    fn flash_length(old: &Self::State, new: &Self::State, dir: i32, ui: &mut Self::Ui) -> f32 { 0.0 }
    fn cursor_location(ui: &Self::Ui, ds: &Self::DrawState, s: &Self::State,
                       p: &Self::Params) -> Option<Rect> { None }
}

pub enum MoveResult<M> { Unused, NoEffect, UiUpdate, Move(M) }
```

Notes:
* `can_solve`, `can_configure`, and so on become `bool` consts defaulting
  to false (`const CAN_SOLVE: bool`); C's `flags` int is `const FLAGS:
  GameFlags`. The conformance test (`puzzles-games/tests/conformance.rs`)
  checks each const against its method for every game.
* `GameCtx` gives `new_game`/`execute_move` the narrow mid-end callback
  that Mines needs (`supersede_game_desc`) without a back-pointer: it
  collects the request, and the mid-end applies it after the call.
* `free_*` / `dup_*` disappear (`Drop` / `Clone`). `fetch_preset` and
  `preset_menu` become `presets()`.
* `decode_move` returns the unit `MoveParseError` where C's
  `execute_move` returns NULL for text it can't parse; like C, it gives
  no reason (decision log).
* Sentinels become `Option`: `current_key_label`'s `""` is `None`;
  `get_cursor_location` leaving its outputs unset is `None`.
* `redraw` keeps C's argument list, so ports stay easy to compare.
* Printing (`can_print`, `print_size`, `print`) joins the trait with
  printing in Phase 9, as default methods, so no game changes then.

### [DECISION 4] Move representation: typed moves (**decided**)
Each game defines `type Move`. `interpret_move` produces it, `execute_move`
consumes it, `solve` returns it, and the mid-end's undo chain stores it.
Text exists only at the persistence edge (save files) through the
game's codec.

**What the C formats look like (survey):**
* mostly a letter + comma-separated integers: `R7,1,1` (solo),
  `A3,4` (net), `F2,5` (mines);
* **compound moves** joined by `;` in 15 games (bridges, dominosa,
  galaxies, lightup, magnets, map, mines, net, pearl, singles, slant,
  tents, tracks, undead, untangle);
* `solve` output almost always starts with `S` (`S`, `S;…`, or `S` + a
  whole-grid payload).

**Design consequences:**
* Decoding **needs context**, so it isn't a plain `FromStr`: Solo's `S`
  payload is validated against the grid size, and Mines coordinates depend
  on the grid type. Hence `decode_move(&State, &str)`. Encoding is
  context-free.
* Typical shape:
  ```rust
  // net.rs (illustrative; the exact variants come from net.c's execute_move)
  pub enum NetMove {
      Ops { jumble: bool, ops: Vec<NetOp> },   // "J;…" / plain "A3,4;C1,2;…"
      Solve(Vec<NetOp>),                        // "S;…"
  }
  pub enum NetOp { RotA(i32, i32), RotC(i32, i32), Flip(i32, i32), Lock(i32, i32), … }
  ```
* `decode_move` only does **syntax and range** checks. Legality against
  the current state stays in `execute_move`, which still returns `None`
  for an illegal move, as in C.
* **Canonical encoding:** `encode_move(decode_move(s)) == s` byte-for-byte
  for every string the C encoders produce. C's `sscanf` is lenient (leading
  spaces, `+` signs). We accept at least everything C *emits*, and record
  any deliberate strictness differences per game.
* The type-erased `dyn Puzzle` layer and save/load work in text; inside
  the engine everything is typed.
* Bonus for the new front end: a typed move can later carry an optional
  `describe_move()` (for example "Place 5 at C4") for a readable move
  history, screen-reader announcements and replay.

Cost accepted: every game gets a `Move` enum and a hand-written codec,
which is more upfront work than keeping strings. The tests in §6
(codec round-trip, C corpus, differential fuzz) keep it safe.

Rejected: strings at the trait boundary with private parsing (less type
safety, and the engine can't reason about moves).

### 3.2 Drawing
```rust
pub trait DrawingApi {
    fn draw_text(&mut self, x: i32, y: i32, font: FontType, size: i32, align: Align, colour: ColourIdx, text: &str);
    fn draw_rect(...); fn draw_line(...); fn draw_polygon(&mut self, pts: &[Point], fill: Option<ColourIdx>, outline: ColourIdx);
    fn draw_circle(...); fn draw_thick_line(...);
    fn draw_update(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn clip(...); fn unclip(&mut self);
    fn start_draw(&mut self); fn end_draw(&mut self);
    fn status_bar(&mut self, text: &str) {}
    fn blitter_save(&mut self, bl: &Blitter, x: i32, y: i32);   // allocates on first use
    fn blitter_load(&mut self, bl: &Blitter, x: i32, y: i32);
    // print hooks with no-op defaults
}
```
Blitters are plain handles (`Blitter { id, w, h }`) that games create
without a drawing (mid-end drawing decision): the draw state can then be
built in `new_game`/`size` while drawing itself is deferred to
`Midend::redraw`.
Games receive `&mut Drawing<'_>`. This wrapper holds `&mut dyn DrawingApi`
plus the helpers from `drawing.c`/`misc.c` (`draw_rect_outline`,
`draw_text_outline`, `draw_rect_corners`, status-bar rewriting). Using
`dyn` here is deliberate: it keeps each game monomorphic, and one vtable
call per primitive is negligible.

Implementations:
* `NullDrawing` (tests, `roundtrip`),
* `RecordingDrawing` (logs every call, used for **draw-trace differential tests**),
* the GUI canvas (desktop),
* `PsDrawing` (PostScript printing).

### 3.3 Other core mappings
| C | Rust |
|---|---|
| `random_state` | `RandomState` (bit-exact counter generator on `sha1_smol`; `random_upto` keeps the same rejection loop; deliberately not a `rand_core::RngCore`) |
| `combi_ctx` | `itertools::Itertools::combinations` (same lexicographic order; oracle-checked) |
| `bin2hex`/`hex2bin` | `hex` crate |
| `DSF*` | three types `Dsf`, `MinDsf`, `FlipDsf` sharing the `DisjointSets` trait (`find`, `set_size`, `same_set`, `reset`); `union` per type (`FlipDsf::union` takes the flip); `dsf_copy`/`dsf_free` → `Clone`/`Drop` |
| `tdq` | `TodoQueue` (`todo_queue` module): `VecDeque` + membership flags; `tdq_remove`'s `-1` → `pop() -> Option<usize>`, so loops become `while let Some(k) = todo.pop()` |
| `shuffle(void *, n, eltsize, rs)` | `RandomState::shuffle<T>(&mut [T])`: generic instead of `void *` + size; same draws as C |
| `obfuscate_bitmap(bmp, bits, decode)` | `misc::obfuscate_bitmap` / `misc::deobfuscate_bitmap`: two functions instead of a flag |
| `bin2hex` / `hex2bin` | `misc::bytes_to_hex` (`hex` crate) / `misc::hex_to_bytes` (ported: C's leniency is observable in game IDs; `None` where C asserts) |
| `getenv_bool(name, dflt)` | `misc::env_bool(name) -> Option<bool>`; callers `unwrap_or` their default (C's `-1` "no preference" is `None`) |
| rest of `misc.c` | `move_cursor` → `misc::move_cursor` (returns `MoveResult`); `button2label` → `Button::label` (`None` where C calls `fatal`); `draw_rect_outline`/`draw_rect_corners`/`draw_text_outline` → `Drawing` methods; `game_mkhighlight`/`colour_mix` → theme roles and `DeriveOp::Highlight`/`Lowlight`/`Mix` (D7); `c2pos`/`c2diff`/`pos2c` with Netslide and Sixteen; `free_cfg`/`free_keys` → `Drop`; `fgetline`/`make_prefs_path`/`copy_left_justified` → mid-end and front end; `swap_regions`/`compare_integers` → `slice::swap`/`Ord::cmp` |
| `tree234` | `BTreeSet`/`BTreeMap` where only add/del/find/count are used; `sorted_set::SortedSet` (the `sorted-vec` crate) where positional ops are used: `add234` → `find_or_insert` (keeps an equal element already present, as C does), `index234` → indexing, `delpos234` → `remove_index`, `findrel234`/`findrelpos234` → `sorted_set::find_rel` (exact as long as at most one element compares equal to the key, which holds for every C caller). No tree234 port |
| `arraysort` (`sort.c`) | `sort::heap_sort_by(&mut [T], cmp)`: C's heapsort ported, because Dominosa's comparators tie and heapsort orders ties its own way; `slice::sort_by` only where a comparator can't tie |
| `grid` pointers | arena `Grid { faces: Vec<Face>, edges: Vec<Edge>, dots: Vec<Dot> }` with `FaceId(u32)` etc. |
| `findloop` | `LoopFinder` (reusable buffers): `run(n, \|v\| neighbours_of(v))` with a closure returning an iterator instead of the `void *ctx` first/next callback; `has_loop()`, `is_loop_edge(u, v)` (C's "non-edge counts as loop edge" quirk kept), `is_bridge(u, v) -> Option<(usize, usize)>` instead of a bool + two out-parameters |
| `matching` | `Matcher` (reusable buffers, replacing C's scratch space): `run(nl, nr, \|l\| neighbours_of(l), rng: Option<&mut RandomState>) -> usize` with a closure instead of `int **adjlists` + `adjsizes`; `left_to_right()`/`right_to_left()` return `&[Option<usize>]` instead of `-1`-filled out-arrays; `witness() -> (Vec<bool>, Vec<bool>)`. The randomised search permutes the matcher's own copy of the lists, not the caller's (no caller reuses them); same draws as C |
| `divvy_rectangle` / `divvy_rectangle_attempt` | `divvy::divvy_rectangle(w, h, k, &mut RandomState) -> Dsf` and `divvy::try_divvy_rectangle(..) -> Option<Dsf>` (`None` where C returns NULL); the result is a `Dsf` built with C's unions, so representatives match C too (Solo numbers jigsaw blocks by them); C's `-1`/`-3`/`-2` markers become `Owner`/`Visit` enums; an empty rectangle gives an empty `Dsf` where C asserts |
| `domino_layout` / `domino_layout_prealloc` | `laydomino::domino_layout(w, h, &mut RandomState) -> Vec<usize>` (partner square per square, itself for an odd area's leftover); one function, since `_prealloc`'s caller-supplied buffers are only an allocation detail; an empty grid gives an empty layout where C asserts |
| `latin.c` solver | `latin::LatinSolver` (owns its grid; `new` = `latin_solver_alloc`, `solve` = `latin_solver_main`, `into_grid` writes back); games implement `LatinRules` (see the decision log); `Deduction`/`Outcome` enums; `Levels` struct for the `diff_*` arguments; scratch space is per call. Strategies stay `pub` (`elim`, `set`, `forcing`, `diff_simple`, `diff_set`) with C's raw cube offsets, since game deductions use them |
| `latin_generate` / `_rect` / `latin_check` | `latin::generate` / `generate_rect` (on `Matcher`, same draws); `latin_check` → `has_latin_counts` (true = is Latin, the negation; like C it checks symbol counts only) |
| `solver_show_working` / `solver_recurse_depth` globals | `solver_log::SolverLog` (verbosity, depth, collected text), carried by each solver (`LatinSolver::log`) |
| `config_item` union | `enum ConfigValue { String(String), Choices { names: Vec<&'static str>, selected: usize }, Bool(bool) }` |
| `preset_menu` tree | `PresetMenu<P> { entries: Vec<PresetEntry<P>> }` with `enum PresetEntry { Preset{title, params, id}, Submenu{title, menu, id} }` |
| `midend_serialise(write cb)` | `fn serialise(&self, w: &mut impl io::Write) -> io::Result<()>` |
| `midend_deserialise(read cb)` | `fn deserialise(r: &mut impl io::Read) -> Result<…, DeserialiseError>` |
| button ints + `MOD_*` | `struct Button { kind: ButtonKind, mods: Modifiers }` (bitflags) with `From<i32>`/`Into<i32>` for the save-file/UI-string paths |
| `#ifdef STANDALONE_SOLVER` | solvers live in the game module as `pub` fns; `puzzles-cli` calls them; verbosity is a `SolverLog` parameter, not a global |
| `atoi`, `sscanf("%d")`, `isdigit` loops on game IDs and moves | `parse::atoi`, `parse::scan_int`, `parse::skip_digits` on bytes: C's leniency (spaces, signs, trailing text); out-of-range numbers truncate a 64-bit `long`, as C on Linux and macOS (Windows C saturates at `i32`; both agree on every number that fits) |
| `debug(())` | `log::debug!` (gated, zero-cost when disabled) |

### 3.4 Errors (rust-best-practices ch. 4)
* `puzzles-core`: `thiserror` enums per area (`DeserialiseError`,
  `GameIdError`, `ConfigError`).
* Game validation: `GameError(Cow<'static, str>)` carrying the **same
  user-facing messages as C** (useful in differential tests and for users).
* Untrusted input (game IDs, descriptions, move strings, save files) must
  **never panic**. It returns `Err`/`None`. The fuzz round-trip enforces this.
* Internal invariants: `assert!` stays on in release, matching the C
  project's deliberate policy. No `unwrap()` on user-input paths.
* Binaries (`puzzles-cli`, `puzzles-desktop`) may use `anyhow`.

### 3.5 Numeric conventions
* Coordinates and game arithmetic stay `i32`, as in C (negative offsets
  and `-1` sentinels are everywhere). Index with `usize` via small helpers
  (`idx(x, y, w)`), so conversions happen in one place.
* `f32` vs `f64` follow the C types exactly in generators
  (rect.c, undead.c) and in drawing.
* **No fused multiply-add on either side.** Rust never contracts
  `a * b + c` into an FMA implicitly. C compilers may: clang defaults to
  `-ffp-contract=on`, which fuses on Apple Silicon/ARM, and GCC in GNU mode
  defaults to `fast`. That would change the last bit of some float results,
  so the C oracle would disagree with Rust on ARM machines only. Rule: the
  oracle build always passes `-ffp-contract=off` for clang/gcc, and keeps
  MSVC at `/fp:precise` without `/fp:contract`. In Rust, ported code
  never uses `mul_add` (the C code never calls `fma()`).
* Every `qsort` call site is checked for comparator ties. Where ties are
  possible, we reproduce the tie-break explicitly and add an oracle test.

---

## 4. Front end design (new, not a port)

The front end is a new application built on the engine. The engine
API (`Midend<G>` behind `dyn Puzzle`) is the only contract. The mid-end
never calls the front end: each frame the front end feeds it events,
reads `wants_timer()`, `is_dirty()` and the game-ID-changed flag, and
calls `redraw(&mut canvas)` when dirty (decision log). It already
supports everything below: arbitrary window sizes, device pixel ratio,
cursor location reporting (for keyboard/accessibility), serialisation
(for session restore), and preferences.

Testability rule: logic that doesn't need egui lives in `app/` and has
unit tests. This covers layout and zoom maths, theme resolution, session
persistence, and input mapping. `ui/` stays thin.

### 4.1 Window, resizing and HiDPI
* **Fluid resize:** on every size change we compute the largest tile size
  that fits the canvas area **in physical pixels** (C's `midend_size`
  logic with `device_pixel_ratio` = egui `pixels_per_point`), rebuild the
  pixmap and do a full redraw. The image is always crisp, never a scaled
  texture. A full redraw of a puzzle is a few milliseconds on the CPU, so
  live-resize can re-render every frame. If that ever proves too slow,
  throttle it and show the scaled old texture in between.
* **Aspect handling:** the puzzle is centred, and the surrounding area is
  filled with the theme's background so letterboxing is invisible.
* **Zoom:** "fit to window" by default, plus Ctrl/Cmd `+`/`-`/`0` and
  Ctrl+wheel for manual zoom with scrolling/panning when larger than the window.
* **Per-monitor DPI changes** (dragging between screens) trigger a re-layout.
* Window size, position, maximised and full-screen state are remembered
  per game.

### 4.2 Themes
Problem found in the C code: palettes are built from
`frontend_default_colour()` (the background), colours derived from it
(`game_mkhighlight` in 22 games, `border = 0.5 × background`), and
**hard-coded RGB** (for example Net's "wires are black", "powered is cyan").
Changing only the background works for light tints but breaks dark mode
(black wires on a dark grey, borders darker than the background).

### [DECISION 7] Theme architecture: no RGB in back ends (**decided**)
**Principle: back ends never contain colour values.** A game *declares*
its palette as a list of named, semantic colour specs. A **theme** (pure
data) supplies every RGB value. The C `game_colours(frontend*)` function
disappears from the `Game` trait.

#### 7a. What the C palettes contain (survey of all 40 games)
| Kind | Examples | Share |
|---|---|---|
| Shared UI roles | BACKGROUND, HIGHLIGHT/LOWLIGHT (`game_mkhighlight`, 22 games), GRID, TEXT/INK/FOREGROUND, CURSOR, ERROR, FLASH | nearly every game |
| Shared puzzle roles | CLUE (given), USER (entered), PENCIL (notes), DONE/CORRECT/SATISFIED, HINT, SELECTED, FAINT/UNKNOWN, shaded cells (pattern FULL, lightup/range/singles BLACK, mosaic MARKED) | ~25 games |
| Categorical series | flood 1–10, guess 1–10, samegame 1–9, map 0–3, mines numbers 1–12, unruly 0/1 (+ hi/lo), signpost chain hues | 7 games |
| Derived from another colour | net BORDER = 0.5 × bg, LOCKED = 0.75 × bg; unruly 0_HIGHLIGHT | many |
| Truly game-specific | net POWERED/BARRIER/ENDPOINT, tents GRASS/TREELEAF, undead GHOST/VAMPIRE/ZOMBIE, inertia GEM/MINE, mines FLAG/BANG | ~20 games, a few each |

#### 7b. Types (in `puzzles-core::palette`, no dependencies)
```rust
/// Semantic roles shared by all games; the theme defines each one.
pub enum Role {
    Background,
    Highlight, Lowlight,   // 3D bevel light/shadow. Theme roles, not computed from
                           // Background as in C, so flat themes can drop bevels
                           // (set both close to Background); Classic keeps C's values.
    Ink, Grid, Cursor, Error, Flash,
    Clue, UserEntry, Pencil, Done, Hint, Selected, Faint,
    Filled,                // a shaded/"black" cell: pattern FULL, lightup/range/
                           // singles BLACK, mosaic MARKED. Real black/white *pieces*
                           // (pearl pearls, galaxies dots) stay `Specific`.
}

/// Named categorical series (distinguishable colours, colour-blind aware).
pub enum Series { Pieces, Numbers, Binary, Chains }

pub enum ColourSpec {
    Role(Role),
    Series(Series, u8),                         // n-th colour of a series
    Derived { base: ColourRef, op: DeriveOp },  // relational, not RGB: Shade(0.5), Mix(other, t), Lighten(..)
    Specific(&'static str),                     // game-scoped key, e.g. "powered"
}
```
Each game declares its palette with a small macro that generates
the colour-index enum *and* the static spec table:
```rust
palette! { // net.rs
    Background => Role(Background),
    Wire       => Role(Ink),
    Powered    => Specific("powered"),
    Barrier    => Specific("barrier"),
    Err        => Role(Error),
    Endpoint   => Specific("endpoint"),
    Border     => Derived { base: Background, op: Shade(0.5) },
    Locked     => Derived { base: Background, op: Shade(0.75) },
}
// → enum Col { Background = 0, Wire, … }  +  const PALETTE: &[ColourSpec]
```
`Game` gets `const PALETTE: &'static [ColourSpec]` in place of `colours()`.
Draw calls still use palette **indices** (the enum), so the engine and
the draw-trace parity tests are unaffected.

#### 7c. Themes are data (`puzzles-theme` crate)
* TOML files: roles, series, optional per-game sections for
  `Specific` keys (and overrides of any entry), and an optional `[ui]`
  section for the app chrome. Built-in themes are bundled with
  `include_str!`.
* **Authoring:** in the first release, user themes are TOML files in the
  config dir, **hot-reloaded** while the app runs (edit, save, see it
  instantly). An **in-app theme editor** (colour pickers, live preview of
  the current puzzle, "save as new theme", contrast warnings) comes in
  Phase 9 (after release 1). It writes the same TOML format.
  ```toml
  name = "Dark"
  extends = "classic"
  dark = true
  [roles]
  background = "#1e1f22"
  ink        = "#e3e3e3"
  error      = "#ff6b6b"
  filled     = "#d0d0d0"
  highlight  = "#2a2b2f"     # flat look: bevels close to background
  lowlight   = "#17181a"
  [series]
  pieces = ["#4e79a7", "#f28e2b", "#e15759", "#76b7b2", "#59a14f", "#edc948", "#b07aa1", "#ff9da7", "#9c755f", "#bab0ac"]
  [game.net]
  powered = "#22d3ee"
  [ui]                      # optional; anything omitted is derived from the roles
  accent  = "#7aa2f7"
  panel   = "#25262a"
  ```
* **One theme styles everything.** The egui `Visuals` (window and panel
  fills, text, widget states, selection, accent, hyperlinks) are built
  from the theme's `[ui]` section. Any key it leaves out is derived from
  the puzzle roles (`background` → window fill, `ink` → text, `selected`
  → selection, `cursor`/`accent` → focus rings). Choosing a theme
  re-skins the whole app consistently. Follow-system switches between a
  light and a dark theme that the user picks in settings.
* **Resolution** (pure function, unit-tested):
  `resolve(&[ColourSpec], game, &Theme) -> Vec<Rgb>`. Lookup order:
  per-game override → role/series/specific value in this theme → parent
  theme (`extends`). A game-specific key a dark theme doesn't define is
  **auto-adapted** from its parent's value: OKLab lightness is remapped
  relative to the two backgrounds, keeping hue and chroma. A new theme
  therefore only needs to define roles and series to be usable, and can
  refine individual games later.
* **"Classic"** is the complete base theme. It reproduces the C colours
  exactly, and it's **generated, not hand-typed**: the oracle dumps every
  C `game_colours()` output, and a script writes `classic.toml`. Role
  values come from the C defaults (Highlight/Lowlight from C's
  `game_mkhighlight` of the default background), and `Specific` keys come
  from each game. Wherever a game's C colour for a role differs from the
  Classic role value (for example one game's "ink" is a dark grey rather
  than black), the script emits a `[game.<name>]` override, so parity
  holds exactly.
* Shipped themes: Classic, Light, Dark, Follow-system (default),
  High-contrast, and colour-blind-safe variants (overriding `series` and
  the few hue-critical game keys).
* Theme switches are live: resolve the palette again, rebuild the egui
  `Visuals`, and force a full redraw.

#### 7d. Porting rule and tests
* Porting a game's `game_colours()` means classifying each entry as
  Role / Series / Derived / Specific. It's a small, reviewable table per game.
  A wrong classification can never change the Classic look, because of
  the test below.
* **Palette parity test:** for every game,
  `resolve(PALETTE, Classic)` must equal the C `game_colours()` output
  (oracle), within float epsilon.
* **Theme lint test:** every built-in theme defines every role and
  series; contrast checks (WCAG ratio) for Ink/Clue/UserEntry/Error
  against Background in each theme.
* Printing keeps its own monochrome/hatched mapping (the C `print()`
  functions use `print_mono_colour`, `print_grey_colour`, …), unaffected by themes.

Alternatives considered: (a) keep C's `colours()` functions with a theme
input and apply an automatic transform. Fewer upfront changes, but RGB
stays in 40 back ends and themes can't be authored as data. (b) Only
swap the background, which gives no real dark mode. (c) Post-process
filter on canvas pixels, which mangles meaning (red errors turn cyan).

### 4.3 Full-screen and immersive modes
* **Full-screen**: F11 on Windows/Linux, Ctrl+Cmd+F on macOS (native Spaces).
  Uses `ViewportCommand::Fullscreen`.
* **Focus/immersive mode**: hides all chrome. The puzzle is maximised; a
  translucent toolbar appears on mouse movement near the top edge or on
  Esc and auto-hides. The status bar and timer become a subtle overlay.
* **Reduced motion** *(after release 1)*: respect the user setting by
  shortening or skipping animations and flashes (`anim_length`/`flash_length`
  scaled in the front end, not in the games).

### 4.4 Feature scope by release (**decided** 2026-11-25)
**Release 1** = all 40 puzzles ported, plus the foundation (§4.1–4.3)
and exactly **one** extra feature: the rules and controls side panel.
Everything else in this table comes after release 1.

| Feature | Release | Notes |
|---|---|---|
| **Toolbar + shortcuts** | 1 (foundation) | New, Restart, Undo, Redo, Solve, Zoom, Full-screen; standard shortcuts per OS (Cmd on macOS) |
| **Simple game list** | 1 (foundation) | plain list of the 40 games with name and one-line description |
| **Session restore** | 1 (foundation) | auto-save every game on exit or switch (midend serialise), resume on launch; no lost progress |
| **Settings** | 1 (foundation) | global settings (theme, UI scale) and per-game preferences (C `get_prefs`/`set_prefs`), in one dialog |
| **Sharing** | 1 (foundation) | copy/paste game ID or seed, drag-and-drop `.sav` files onto the window, open `.sav` from the OS file manager |
| **Keyboard play** | 1 (foundation) | already provided by the back ends' cursor mode; the front end just maps keys (§4.5) |
| **Rules and controls side panel** | **1 (the one extra)** | per-game rules and controls, converted once from `puzzles.but` to Markdown, shown in a collapsible side panel next to the puzzle (hidden in immersive mode) |
| **Packaging** | 1 (needed to ship) | Windows (MSI/zip), macOS `.app`/`.dmg`, Linux (AppImage / Flatpak / `.deb`) via `cargo-packager`; signing/notarisation later |
| Game library upgrade | later | live-rendered thumbnails (our engine renders `icons/*.sav`), search, recently played |
| Statistics | later | completion times, games solved per preset, best times (stored locally) |
| Touch and pen | later | long-press = right click (for `REQUIRE_RBUTTON` games), pinch to zoom; uses the existing `STYLUS_SUPPORT` flag |
| Accessibility extras | later | high-contrast and colour-blind themes, reduced motion, screen-reader labels via AccessKit (built into egui), visible-focus polish |
| Localisation | later | translated UI. To keep this cheap later, release 1 keeps all UI strings in one module, with no translation framework yet |
| In-app theme editor | later | D7: colour pickers + live preview, writes theme TOML |
| Command palette | later | optional |

### 4.5 Input mapping
Keyboard and mouse events map to the engine's `Button` (including
`CURSOR_*`, `MOD_*` and the numeric keypad flag). There's a small, tested
table per OS (Cmd vs Ctrl). Mouse-button priority flags
(`BUTTON_BEATS`) are honoured as in the C mid-end.

---

## 5. Dependencies

### [DECISION 5] Dependency policy (**decided**) and dependency set

**Policy.** Keeping dependencies to a minimum is *not* a goal. When an
established crate does the job, we use it rather than reimplementing.
A crate qualifies if it is actively maintained, widely used, has an
MIT/Apache-compatible licence, and builds on Windows, macOS and Linux
(and on wasm if it's used by shared code).

**The one exception comes from D1 level 1.** Code whose *output*
decides which puzzle a seed produces, or how a C save file replays,
must behave exactly like the C code. A crate may replace such code only
if its result is **uniquely determined by its input** (a SHA-1 digest,
the order of a sorted set, the sequence of k-combinations) *and* an
oracle test proves equality. Algorithms that return *one of several
valid answers* are ported, because a different library would pick a
different answer and change the puzzles. That covers union-find
representatives, bipartite matchings, Latin squares, polyomino division,
domino layouts, loop generation and grid generation.

**About `rand`.** The C generator is a custom SHA-1 counter scheme, and
`rand`'s generators and helpers (`gen_range`, `shuffle`) consume random
bits differently from C's `random_upto`/`shuffle`. Using them in a
generator would change every puzzle. So:
* `RandomState` (C-compatible) is ours. It deliberately does **not**
  implement `rand_core::RngCore`, so `rand` helpers can't be used in a
  generator by accident.
* `rand` **is** used outside the compatibility boundary: creating fresh
  seeds for new games (C's `get_random_seed`, done by the front end and
  CLI), test data, and front-end effects.

#### Engine (`puzzles-core`, `puzzles-games`)
| Crate | Replaces / provides | Compatibility note |
|---|---|---|
| `sha1_smol` (**decided**) | SHA-1 under the PRNG and `obfuscate_bitmap` | digest is unique; the PRNG logic on top (`random_bits`, `random_upto`) is ported; oracle-checked in Phase 0 |
| `itertools` | `combi.c` (`combinations`), plus general iterator helpers | same lexicographic order as C's `next_combi`; oracle-checked |
| `BTreeSet`/`BTreeMap` (std) + `sorted-vec` (**decided**) | `tree234.c`. Std collections where only add/del/find/count are used; `sorted-vec` for the 9 files that use positional ops (`index234`, `delpos234`): flip, latin, loopgen, net, netslide, pegs, penrose, spectre, untangle | a sorted set's order is unique for a given comparator; oracle-checked against `tree234.c`; benchmark untangle/loopgen when they are ported |
| `hex` | `bin2hex`/`hex2bin` | format is unique |
| `thiserror` | error enums | — |
| `bitflags` | `Modifiers`, `GameFlags` | — |
| `log` | the C `debug()` macro | — |
| *ported, not replaced* | random generator logic, `dsf`, `tdq`, `matching`, `latin`, `divvy`, `laydomino`, `loopgen`, `findloop`, `grid` + Penrose/Hat/Spectre, `sort` call sites with possible ties | output is either one of many valid answers (would change puzzles) or tightly bound to C semantics. `findloop` (bridge detection) has a unique answer, but **`petgraph` was evaluated and rejected** (2026-11-25): 0.8's `algo::bridges` returns only bridge edges (no per-side component sizes, which Dominosa needs), needs a graph built from the callback on every call (Dominosa's solver loop), and treats a repeated neighbour as a loop where C doesn't. Ported instead |

#### Themes (`puzzles-theme`)
| Crate | Provides |
|---|---|
| `serde` + `toml` | theme files |
| `palette` | colour types, sRGB↔OKLab/OKLCH conversions for dark-theme auto-adaptation, contrast maths (replaces the planned hand-written OKLab code) |
| `csscolorparser` (with `serde`) | theme authors can write any CSS colour syntax (`#1e1f22`, `rgb(…)`, `hsl(…)`, named colours) |
| `notify` (feature-gated) | hot-reload of user theme files |

#### Desktop (`puzzles-desktop`)
| Crate | Provides |
|---|---|
| `eframe` / `egui` | window, menus, dialogs, clipboard, IME, AccessKit, a path to wasm (D6) |
| `egui_extras` | images/SVG in the UI (icons; later, library thumbnails) |
| `egui_commonmark` (or similar) | renders the per-game Markdown rules in the side panel (release 1) |
| `egui-phosphor` (or similar icon font) | toolbar and menu icons |
| `tiny-skia` (with `png`) | persistent puzzle canvas; PNG export for thumbnails and snapshot tests |
| `ab_glyph` + bundled fonts | text in the canvas. **Inter** for `FONT_VARIABLE` and **JetBrains Mono** for `FONT_FIXED` (both OFL). The same two fonts are registered with egui for the app UI, so puzzles and app render identically on every OS. Noto Sans is added as a fallback with localisation |
| `rfd` (`xdg-portal` on Linux) | native open/save dialogs |
| `directories` | per-OS config/data paths |
| `serde` + `toml` | app settings, window state, statistics (puzzle prefs and saves keep the C formats) |
| `rand` | fresh seeds for new games |
| `env_logger` | log output |
| *(Phase 9)* `fluent` or `rust-i18n` | localisation |

#### CLI (`puzzles-cli`)
`clap`, `anyhow`, `rand`, `env_logger`.

#### Dev, test and tooling
| Crate / tool | Use |
|---|---|
| `cc` | `c-oracle`: compile the C sources for FFI differential tests |
| `proptest` | property tests (core data structures, move codecs) |
| `insta` | snapshot tests (descriptions, solver output, draw traces after acceptance) |
| `image` (+ `image-compare` or similar) | tolerant PNG comparison for canvas snapshots |
| `criterion` or `divan` | benchmarks (generation time per preset, replaces `benchmark.sh` timings) |
| `cargo-fuzz` + `arbitrary` | fuzzing save files and move strings (Linux/macOS CI) |
| `cargo-packager` (tool) | installers per OS (Phase 8) |

Not used: any async runtime (nothing in the app needs one).

### [DECISION 6] GUI front end (**decided**)
Two independent choices:

* **D6a, puzzle canvas: a `tiny-skia` pixmap.** It implements
  `DrawingApi` and exposes a pixel buffer. It knows nothing about the UI
  toolkit, so it's reused unchanged for headless snapshot tests,
  thumbnails and PNG export.
* **D6b, UI toolkit: `egui` via `eframe`.** The canvas is uploaded as a
  texture when `draw_update` marks it dirty.

Why a separate canvas: the back ends draw **incrementally**. `redraw`
repaints only the tiles that changed since its `game_drawstate` cache,
so earlier pixels must survive between frames. egui rebuilds and
re-renders the whole frame on every repaint, so drawing puzzles straight
into egui's `Painter` would lose everything not redrawn. Every C front end
already solves this with a backing store: a cairo image surface in
`gtk.c`, an offscreen canvas in `emcclib.js`, GDI bitmaps in
`windows.c`. tiny-skia is the Rust equivalent. It's a pure-Rust port of a
Skia CPU rasteriser subset (no C++ build dependency) and is deterministic
on every platform. Blitters become sub-rectangle pixmap copies. Text is
drawn with `ab_glyph` into the same pixmap.

Why eframe: one codebase for Windows, macOS and Linux (X11 + Wayland,
via winit), with system light/dark detection, native full-screen,
HiDPI, clipboard and IME. It also has a credible path to a web/wasm build later.
Its immediate-mode style suits the dynamic parts of the new UI
(config forms generated from `ConfigItem`s, the game library, overlays).

Rejected alternatives:
* *Canvas:* **egui renders into a persistent wgpu texture**
  (render-to-texture, no clear between frames, GPU copies for blitters).
  It's feasible, but needs more plumbing for no visible gain, since
  puzzle frames are cheap on the CPU.
* *Toolkit:* **iced**. Its model/update/view structure and built-in
  theming are attractive for a large app, but it's slower to build with,
  dynamic forms take more code, and the web story is less mature.
* *Toolkit:* **Slint**. Its GPLv3/royalty-free licensing adds friction for an
  MIT project.
* *Toolkit:* **`winit` + `softbuffer`**. We'd hand-build every menu, dialog
  and overlay, which works against the "modern UI" goal.
* *Toolkit:* ~~Native Win32 port of `windows.c`~~. Ruled out by the cross-platform requirement.

Consequence for the code layout: egui-specific code is confined to
`puzzles-desktop/src/ui/`. `app/` and `canvas/` don't depend on egui,
which keeps them testable and would limit the blast radius of a
toolkit change.

---

## 6. Testing strategy (the backbone of the port)

1. **C oracle crate (`c-oracle`, dev-only).** A `build.rs` uses `cc` to
   compile the upstream `core` sources (from the submodule below) plus each
   game with `-DCOMBINED`, and adds a small C shim (`csrc/shim.c`)
   providing front-end stubs and flat functions:
   `oracle_random_bits`, `oracle_new_desc(game, params, seed)`,
   `oracle_solve(game, id)`, `oracle_execute(game, id, moves…)`,
   `oracle_redraw_trace(...)` (with a recording `drawing_api`),
   `oracle_colours(game)` (for palette parity and `classic.toml` generation), and so on.
   Rust tests call both implementations and compare.
   * It's cross-platform: `cc` picks MSVC on Windows, clang on macOS and
     gcc/clang on Linux, and the C sources are portable. It's gated behind
     a cargo feature (`oracle`), so contributors without a C toolchain can
     still run `cargo test`.
   * Its `build.rs` disables floating-point contraction
     (`-ffp-contract=off` for clang/gcc; MSVC stays at `/fp:precise`
     without `/fp:contract`), so results match Rust on x86 and ARM alike
     (see §3.5).
   * **C source: git submodule** `third_party/puzzles-c` (upstream
     `https://git.tartarus.org/simon/puzzles.git`, pinned to `616da16`, the
     commit analysed in `c-codebase-analysis.md`). CI on all 3 OSes checks
     it out with `submodules: true`. Bumping the pin is a deliberate change
     that re-runs the whole oracle suite. An env var `PUZZLES_C_SRC` can
     override the path for local experiments, for example pointing at
     another upstream checkout.
   * The submodule is only needed for the `oracle` feature. A plain
     `cargo build`/`cargo test` works without it.
2. **Golden corpus.** For every game and every preset, N seeds give the
   C-generated description and aux, solver output and `text_format`. These
   are generated once by the oracle and checked into `tests/corpus` as
   `insta` snapshots, so ordinary `cargo test` doesn't need a C compiler.
   The snapshots must pass **on all three OSes**, which is our
   proof that generation is cross-platform deterministic. (Checked:
   generators use only `f32` arithmetic and division, which IEEE 754
   makes identical everywhere. There are no `sin`/`cos`/`exp` calls in
   generation, so no platform-libm differences.)
3. **Save-file round trip.** `icons/*.sav` (40 files) are loaded, replayed,
   redrawn (null and recording drawing) and re-serialised. The output must
   be byte-identical to C `fuzzpuzz`.
4. **Draw-trace parity (D1 level 2, a porting gate).** The
   `RecordingDrawing` trace of `redraw` for the same state/UI/tilesize
   must equal the C trace while a game is being ported. When the game is
   accepted, its oracle draw test is retired and the trace becomes an
   `insta` snapshot that we may update deliberately when graphics change.
5. **Move codec tests (D4).**
   * Corpus: every `MOVE` line in `icons/*.sav`, every move produced by
     oracle-driven random play, and every C `solve` output must satisfy
     `encode_move(decode_move(s)) == s` byte-for-byte.
   * Property: `decode_move(encode_move(m)) == m` for generated `Move`
     values (`proptest` strategies per game).
   * Differential fuzz: for mutated move strings, compare C's
     `execute_move` with Rust's `decode_move` + `execute_move`. Accept/reject
     and the resulting state must agree, except for documented strictness
     differences.
6. **Unit and property tests** for core data structures (`proptest`).
7. **Fuzz round trip** via `puzzles-cli roundtrip` over mutated save files
   (and `cargo fuzz` later, optional): no panics on untrusted input.
8. **Lints.** Workspace `[lints]` with clippy `all = deny`,
   `pedantic = warn` (with a short, documented allow-list for
   cast-heavy ported code: `cast_possible_truncation`, `cast_sign_loss`,
   `similar_names`, `too_many_lines`, used via `#[expect]` where local),
   and `unsafe_code = deny`, with exactly one opt-out: the FFI module of
   `c-oracle`. One command runs every check, locally and in CI:
   **`cargo xtask ci`** (format, clippy with `-D warnings`, all tests
   including the C oracle). `cargo xtask ci --no-oracle` skips the C tests
   on machines without a C compiler or the submodule.
9. **CI (GitHub Actions, `.github/workflows/ci.yml`)** runs
   `cargo xtask ci`, *including the oracle*, kept frugal because macOS
   runners are scarce:
   - **pull requests:** `ubuntu-latest` (gcc) and `windows-latest` (MSVC);
     PRs touching only docs (`docs/**`, `*.md`, `LICENCE`, `.claude/`,
     `.agents/`) don't run;
   - **macOS (clang on ARM) on demand:** a manual run
     (`gh workflow run ci.yml --ref <branch>`, macOS on by default) or
     `cargo xtask ci` on a Mac. Do it before merging changes where
     platform differences can bite: float or drawing code, anything near
     FMA, `c_ulong` or other C type widths;
   - **no rerun after merging to `main`:** with squash merges the PR run
     already tested the same code.

   Linux CI will also build the desktop app once it exists (needs the
   usual X11/Wayland dev packages).
10. **Theme tests.** Palette parity (`resolve(PALETTE, Classic)` == C
   `game_colours()`), theme completeness and contrast lint (D7d), and
   resolver unit tests (inheritance, overrides, OKLab adaptation).
11. **Front-end tests.** Unit tests cover `app/` (layout/zoom maths, input
    mapping per OS, session save/restore). **Canvas snapshot tests** render
    each game headlessly through the tiny-skia canvas in Classic, Light and
    Dark, and compare PNGs with a small tolerance. This works without a
    window on any CI OS.

A game counts as **done** (accepted as ported) when it passes: golden
descriptions for all presets × N seeds, solver parity, its `.sav` round
trip, move-codec corpus and property tests, draw-trace parity for a
scripted move sequence, palette parity under Classic, a review of its
canvas snapshots in every built-in theme, and manual play in the desktop
app. At acceptance, its draw-trace oracle test is converted to a snapshot (D1).

---

## 7. Phases

| Phase | Content | Exit criteria |
|---|---|---|
| **0. Scaffolding** | workspace, lints, toolchain pin, `third_party/puzzles-c` submodule at `616da16`, `c-oracle` crate (C `random`/SHA-1 bindings + shim build), `cargo xtask ci`, **3-OS CI matrix** | oracle tests (SHA-1 vectors, golden `random_bits`/`random_upto` sequences) pass on Windows, macOS and Linux |
| **1. Core algorithms** | port: random (on `sha1_smol`), misc, dsf, tdq, findloop (`petgraph` evaluated and rejected, see §5), matching, laydomino, divvy, latin. Adopt crates: `itertools` for combi, std collections + sorted-vec for tree234, `slice::sort_by` for sort.c; verify each against C | parity with C on property-generated inputs (including the crate-backed replacements); ports of `auxiliary/*-test.c` |
| **2. Engine + walking skeleton** | the rest of `misc.c` (see the mapping table in §3.3), `Game` trait (**with `PALETTE` specs and DPR-aware sizing from day one**), `palette!` macro, `puzzles-theme` (model, resolver, Classic generated from the oracle), `Drawing`, `Midend<G>` (undo/redo, anim timing, game-ID/seed parsing, serialise/deserialise, presets, prefs), erased `dyn Puzzle`, `puzzles-cli` (`generate`, `solve`, `roundtrip`), **nullgame + flip + fifteen + guess** | the 3 games pass the full "done" bar headlessly, including palette parity |
| **3. Desktop front end: foundation** | eframe app on all 3 OSes: tiny-skia canvas + text + blitters, **fluid resize + HiDPI**, **themes (Classic/Light/Dark/Follow-system, user TOML + hot reload)**, **full-screen + immersive mode**, toolbar + per-OS shortcuts, simple game library, presets/custom-params/game-ID/seed dialogs generated from `ConfigItem`s, per-game prefs, save/load (incl. drag-and-drop), session restore, timer, status overlay | the Phase-2 games are playable and look right in every theme on Windows, macOS and Linux |
| **4a. Simple games** | sixteen, twiddle, flood, pegs, samegame, blackbox, netslide, cube, inertia, mosaic, unruly | done bar |
| **4b. Union-find family** | net, slant, range, filling, palisade, signpost, map, bridges, dominosa, tents, magnets, rect, galaxies, lightup, undead, tracks, pattern, untangle | done bar |
| **4c. Latin family** | towers, unequal, keen, singles, solo | done bar |
| **5. Grid family** | `grid` arena + 19 tilings (penrose, hat, spectre tables ported by a script), loopgen → loopy, pearl, **mines** | done bar, including hat/spectre `auxiliary` test parity |
| **6. CLI parity** | standalone solver/grader CLI parity (`-g`/`-v`), `benchmark` | output parity with the C `*solver` binaries |
| **7. Rules and controls panel** | convert each game's section of `puzzles.but` to Markdown (one-off script + manual review), render it in a collapsible side panel (egui markdown viewer), and show the controls for the current game. Hidden in immersive mode | all 40 games have a reviewed rules page; panel works on all 3 OSes |
| **8. Packaging and release 1** | `cargo-packager`: Windows MSI/zip, macOS `.app`/`.dmg`, Linux AppImage/Flatpak/`.deb`; file associations for `.sav`; signing/notarisation when we're ready | installable artifacts from CI for all 3 OSes → **release 1** |
| **9. After release 1** | **printing** (`ps.rs` + `printing.rs` → `.ps`, plus PDF; several puzzles per page, optional solutions), **web/wasm build** (eframe), and the §4.4 "later" items, prioritised then: library thumbnails + search, statistics, touch/pen, accessibility extras (high-contrast/colour-blind themes, reduced motion, AccessKit), localisation, in-app theme editor, command palette | per feature; printing output checked against C PostScript via the oracle |

Phases 4a–4c can run in parallel once Phase 2 is stable, because each
game is an independent module with its own oracle tests. Phase 7 can
run alongside Phases 4–5. Front-end and game work only meet at the
`dyn Puzzle` API.

### Porting recipe for one game
1. Reference the C source, don't copy it. The pinned submodule
   (`third_party/puzzles-c` @ `616da16`) is the canonical reference.
   Each game module starts with a header such as
   `//! Port of third_party/puzzles-c/net.c (upstream 616da16).`, and
   non-obvious functions note their C origin
   (`// C: net.c, new_game_desc()`). Correctness is proven by the oracle
   tests, not by line-by-line comments.
   Skip the out-of-scope `#ifdef` paths (`EDITOR`,
   `STANDALONE_PICTURE_GENERATOR`, `STANDALONE_SOAK_TEST`,
   `STANDALONE_OBFUSCATOR`). `STANDALONE_SOLVER` code is ported into
   `pub` solver functions for `puzzles-cli`. Port `Params` + encode/decode
   + presets first, then add golden tests for encode/decode.
2. Port `validate_desc`, `new_game`, `State`, then the generator (`new_desc`)
   and solver. The golden descriptions must match.
3. Define the `Move` enum from the cases in C's `execute_move` (and the
   strings `interpret_move`/`solve` build), and write
   `encode_move`/`decode_move`. The codec corpus and property tests must pass.
   Then port `interpret_move`/`execute_move`/`Ui`. The `.sav` replay must match.
4. Port the palette: classify each C colour as Role / Series / Derived /
   Specific in `palette!`. Palette parity under Classic must hold.
5. Port drawing. The draw trace must match. Review the canvas snapshots
   in every built-in theme, and add `[game.<name>]` theme overrides
   where the auto-adapted colours are poor.
6. Delete the dead C idioms (manual free, sentinels) and run clippy.
7. Play it in the desktop app on at least two OSes.

---

## 8. Status

**All planning decisions are made** (2026-11-25); see the decision log
at the top.

**Progress**
- **Phase 0: done** (2026-11-25). Workspace, `c-oracle`, `cargo xtask ci`;
  CI green on Ubuntu, Windows and macOS.
- **Phase 1: done** (2026-11-25):
  - `random` (`RandomState`, on `sha1_smol`; parity with C for call
    sequences, encode/decode and counter carries, property-tested);
  - `dsf` (`Dsf`/`MinDsf`/`FlipDsf` + `DisjointSets`; parity with C for
    canonical elements, sizes, minima and flips after arbitrary operation
    sequences, copies and long chains);
  - `tdq` (`TodoQueue`; parity with C for pop order after arbitrary
    push/pop/fill sequences and repeated ring-buffer wraparounds);
  - `misc`, engine-independent part (`RandomState::shuffle`, bitmap
    obfuscation, lenient hex, `n_times_root_k`, `env_bool`; parity with C
    including exhaustive `n_times_root_k` over ±20,000 × k 1–12). The rest
    of `misc.c` moves to Phase 2;
  - `findloop` (`LoopFinder`, ported after rejecting `petgraph`; parity with
    C for the loop flag and every pair's loop-edge and bridge answers,
    including side sizes, on random and grid-shaped graphs);
  - `matching` (`Matcher`, Hopcroft–Karp; parity with C for the count,
    both partner arrays, the maximality witness and the generator state
    after randomised runs, on random graphs of every density with empty
    sides and repeated edges, reused matchers, `latin.c`-style row-by-row
    generation and upstream's 256 + 256 subset graph);
  - `laydomino` (`domino_layout`; parity with C for the layout and the
    generator state on every grid up to 16 × 16, random grids up to
    24 × 24, layout sequences from one generator and large grids);
  - `divvy` (`divvy_rectangle`/`try_divvy_rectangle`; parity with C for
    success or failure, every square's representative and the generator
    state, on every rectangle up to 8 × 8 with every divisor of its area,
    random shapes up to 12 × 12, Solo jigsaw and Palisade sizes, the
    retry loop, and runs of failing attempts);
  - `latin` (`LatinSolver` + `LatinRules`, `generate`, `generate_rect`,
    `has_latin_counts`; parity with C for generated squares and rectangles
    with the generator state, every strategy step on random candidate
    cubes, and full solves with test game rules, comparing outcome, grid,
    cube and the exact sequence of game callbacks, including fresh or
    shared contexts while guessing; a coverage test checks every solver
    path is reached). Its `SolverLog` text matches C's
    `STANDALONE_SOLVER` output byte for byte on a checked-in fixture;
  - crate-backed replacements: `itertools::combinations` for `combi.c`
    (identical for every r of n up to 12), `sorted_set` (`sorted-vec` +
    `find_rel`) for `tree234.c` (identical results and contents after
    every step of random add/delete/index/relation sequences, including
    which of two equal elements is kept), and `sort::heap_sort_by` for
    `arraysort` (identical order of tied elements).

  Notes for later game ports, found on the way:
  - Lightup: C's `discount_clue` can call `new_combi(n + 1, n)` once a
    light already sits next to the clue, which trips C's `r <= n`
    assert; `combinations` just yields nothing there. Check when porting.
  - Keen, Towers, Unequal: `LatinRules::is_valid` runs only on complete
    grids, so a validator that rejects every grid below a guess makes the
    search enumerate all of them, in C as in Rust.
  - `qsort` call sites (10 games): libc's tie order differs between
    platforms, so check each comparator for ties (§3.5).
  - Flip: random shapes on 1 × 2 and 2 × 1 grids can never pass C's
    "no two shapes alike" check, so generation never ends there, in C as
    in Rust. The desktop app's cancellable generation thread covers it.

- **Phase 2: in progress.** Done:
  - core types (`button`, `config`, `preset`, `game` result types,
    `palette` + `palette!`, `drawing` with blitter handles,
    `Drawing` helpers, `NullDrawing`/`RecordingDrawing`, and
    `misc::move_cursor`; parity with C for button codes and macros,
    `button2label`, `move_cursor`, and random sequences of drawing calls
    and helpers through `drawing.c` and a C recording drawing API, whose
    trace text matches `RecordingDrawing` byte for byte);
  - the `Game` trait (`game`), C's number parsing (`parse`), nullgame and
    Flip (`puzzles-games`). The oracle compiles C games with `COMBINED`
    and drives them through a generic harness (`csrc/game_harness.c`,
    `CGame`/`CSession`), and `tests/common` runs a Rust game beside it
    (`Pair<G>`), reusable for every game. Flip matches C for presets,
    parameter decoding/encoding/validation of arbitrary strings, every
    preset's descriptions over 25 seeds and random sizes up to 8 × 8,
    description validation of mutated descriptions, and random play
    scripts (clicks, keys, solver, raw and malformed move strings)
    compared step by step: move strings, states, animation and flash
    lengths, every animation and flash frame's draw trace, and the
    status bar. A conformance test checks every game's `CAN_*` consts,
    parameter round trips, presets and palette. Flip's palette parity
    waits for the theme crate; its golden corpus (`insta`), `.sav`
    round trip and desktop play complete its done bar later in Phase 2
    and Phase 3.

  Next: the mid-end, `dyn Puzzle`, save files and prefs.

Differential tests (Rust vs C) live in `crates/c-oracle/tests/*_parity.rs`;
each ported module also keeps Rust-only unit tests, so `--no-oracle` runs
still check it.

**Deferred by design (decided when first needed, under the D5 policy):**
the icon font; `criterion` vs `divan`; `fluent` vs
`rust-i18n`; the Markdown viewer crate. Swapping one for an equivalent
crate is a normal review item, not a new decision.

