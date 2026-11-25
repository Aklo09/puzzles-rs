# Machine setup (temporary)

> Temporary note for moving work between the Windows and macOS machines.
> To be removed once this is covered by a proper contributor guide.

## Cloning

Clone with the submodule in one step:

```sh
git clone --recurse-submodules https://github.com/Aklo09/puzzles-rs.git
```

For an existing clone, or after pulling a commit that adds or bumps the
submodule, fetch the upstream C sources used by the oracle tests:

```sh
git submodule update --init
```

The submodule (`third_party/puzzles-c`, upstream pinned to `616da16`) is
only needed for the C oracle tests; `cargo xtask ci --no-oracle`, a plain
`cargo build` and `cargo test` work without it.

## Cargo tools (once per machine)

These are installed per machine and aren't recorded in the repo:

```sh
cargo install cargo-binstall      # only if cargo-binstall is missing
cargo binstall cargo-insta cargo-nextest cargo-deny
```

- `cargo-insta`: review and accept snapshot tests.
- `cargo-nextest`: faster test runner.
- `cargo-deny`: licence and security-advisory checks for dependencies.

## Github cli
```sh
brew install gh
gh auth login
```