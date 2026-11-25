# puzzles-rs: Rust port of Simon Tatham's Portable Puzzle Collection

Upstream C source: `third_party/puzzles-c` (git submodule, pinned to
`616da16`). The plan and every design decision live in
`docs/porting-plan.md` (decision log at the top); the C analysis is in
`docs/c-codebase-analysis.md`. Read the relevant section before changing
architecture.

## Working agreement
- Design choices not already settled in the plan are presented as options
  with a recommendation and trade-offs. The user decides, and the outcome
  is recorded in the plan's decision log.
- Commit only when asked, with a descriptive message.
- Branches: one short-lived branch per unit of work (`core/random`,
  `engine/midend`, `game/flip`, `ui/canvas`, experiments as `spike/...`),
  merged into `main` by pull request once CI passes (Ubuntu + Windows).
  Trivial fixes may go straight to `main`.
- Work happens on Windows and macOS, and CI covers Linux too. Never add
  OS-specific paths or assumptions outside `cfg`-gated code.

## Hard rules
- **C compatibility (D1, level 1):** the same `params#seed` must give the
  same puzzle as C, and moves, save files and prefs must round-trip
  byte-identically. Port generators and solvers faithfully: same algorithm,
  same iteration order, same PRNG calls.
- **Dependencies (D5):** prefer established crates. The exception is code
  whose output decides puzzles; port it, unless the crate's result is
  uniquely determined and an oracle test proves equality. Never use `rand`
  inside generators.
- **Floats:** keep C's `f32`/`f64` types; no `mul_add`; the C oracle is
  built with `-ffp-contract=off` (MSVC: `/fp:precise`, no `/fp:contract`).
- **No colour values in game code (D7):** games declare `PALETTE` specs;
  themes supply RGB.
- Untrusted input (game IDs, descriptions, moves, save files) never panics.
- No `unsafe` outside the `c-oracle` crate.

## Porting style: idiomatic Rust, identical behaviour
Map C constructs to idiomatic Rust rather than transliterating them. Keep
the *observable* behaviour identical (outputs, iteration order, PRNG calls,
which element/index results come out), not the C shape. For example:
- runtime "kinds" of one C struct → separate types sharing a trait
  (`Dsf`/`MinDsf`/`FlipDsf` + `DisjointSets`), so misuse is a compile error;
- `dup_*`/`*_copy`/`*_free` → `Clone`/`clone_from`/`Drop`;
- out-parameters → tuples or structs; sentinel values → `Option`/enums;
  error strings → `Result`; `void *ctx` callbacks → closures or traits;
- idiomatic names (`find`, `union`, `reset`) with
  `#[doc(alias = "c_function_name")]` so the C name stays searchable.

## Porting a game
Follow "Porting recipe for one game" in `docs/porting-plan.md` §7.
Reference the C source in module headers
(`//! Port of third_party/puzzles-c/<game>.c (upstream 616da16).`).
Never copy the C code into the repo.

## Checks (run before calling work done)
```
cargo xtask ci              # fmt, clippy -D warnings, all tests incl. C oracle (same as CI)
cargo xtask ci --no-oracle  # same, without the C reference tests
cargo insta test --review   # when snapshots change
```
CI (GitHub Actions) runs `cargo xtask ci` on pull requests, on Ubuntu and
Windows, and skips docs-only changes. macOS runs on demand only:
`gh workflow run ci.yml --ref <branch>` or `cargo xtask ci` on a Mac. Ask
for a macOS run before merging float, drawing or C-type-width-sensitive
changes.
