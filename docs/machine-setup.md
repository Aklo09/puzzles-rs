# Machine setup (temporary)

> Temporary note for moving work between the Windows and macOS machines.
> To be removed once this is covered by a proper contributor guide.

## After cloning (or pulling a commit that adds/bumps the submodule)

Fetch the upstream C sources used by the oracle tests:

```sh
git submodule update --init
```

The submodule (`third_party/puzzles-c`, upstream pinned to `616da16`) is
added in Phase 0. It's only needed for the `oracle` feature; a plain
`cargo build` / `cargo test` works without it.

## Cargo tools (once per machine)

These are installed per machine and aren't recorded in the repo:

```sh
cargo install cargo-binstall      # only if cargo-binstall is missing
cargo binstall cargo-insta cargo-nextest cargo-deny
```

- `cargo-insta`: review and accept snapshot tests.
- `cargo-nextest`: faster test runner.
- `cargo-deny`: licence and security-advisory checks for dependencies.
