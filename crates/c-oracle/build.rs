//! Build script for `c-oracle`: compiles the upstream C sources when the
//! `oracle` feature is enabled, and does nothing otherwise.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "oracle")]
    oracle::build();
}

#[cfg(feature = "oracle")]
mod oracle {
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    /// Upstream C files compiled into the oracle. Grows as more of the
    /// engine and the games are ported and need a reference.
    const C_SOURCES: &[&str] = &[
        "combi.c",
        "divvy.c",
        "drawing.c",
        "dsf.c",
        "findloop.c",
        "latin.c",
        "laydomino.c",
        "malloc.c",
        "matching.c",
        "midend.c",
        "misc.c",
        "random.c",
        "sort.c",
        "tdq.c",
        "tree234.c",
    ];

    /// Games compiled into the oracle, by the name of their `struct game`
    /// (and C file). They are built with `COMBINED`, as in C's
    /// all-in-one builds, so each defines `const game <name>` rather than
    /// `thegame`.
    const C_GAMES: &[&str] = &["fifteen", "flip", "nullgame"];

    pub fn build() {
        println!("cargo::rerun-if-env-changed=PUZZLES_C_SRC");
        let c_dir = c_source_dir();

        let shims = [
            PathBuf::from("csrc/shim.c"),
            PathBuf::from("csrc/recorder.c"),
            PathBuf::from("csrc/game_harness.c"),
            PathBuf::from("csrc/midend_harness.c"),
        ];
        for shim in &shims {
            println!("cargo::rerun-if-changed={}", shim.display());
        }
        println!(
            "cargo::rerun-if-changed={}",
            c_dir.join("puzzles.h").display()
        );

        // With COMBINED, puzzles.h declares every game from the list in
        // generated-games.h, which C's CMake build writes; write ours.
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        let games_h: String = C_GAMES.iter().flat_map(|g| ["GAME(", g, ")\n"]).collect();
        fs::write(out_dir.join("generated-games.h"), games_h).expect("write generated-games.h");

        let mut build = cc::Build::new();
        build
            .include(&out_dir)
            .include(&c_dir)
            .define("COMBINED", None)
            .files(&shims)
            .warnings(false);
        let games = C_GAMES.iter().map(|g| format!("{g}.c"));
        for source in C_SOURCES.iter().map(|s| (*s).to_owned()).chain(games) {
            let path = c_dir.join(source);
            println!("cargo::rerun-if-changed={}", path.display());
            build.file(path);
        }

        // Never fuse a*b+c into an FMA: Rust doesn't, so contracting C code
        // would disagree with the port on ARM (docs/porting-plan.md §3.5).
        if build.get_compiler().is_like_msvc() {
            // /fp:precise is MSVC's default and doesn't contract without
            // /fp:contract; spell it out so a default change can't bite us.
            build.flag("/fp:precise").flag("/std:c11");
            build.define("_CRT_SECURE_NO_WARNINGS", None);
        } else {
            build.flag("-ffp-contract=off");
        }

        build.compile("puzzles_c");
    }

    /// Location of the upstream C tree: `PUZZLES_C_SRC` if set, otherwise the
    /// `third_party/puzzles-c` submodule.
    fn c_source_dir() -> PathBuf {
        let dir = env::var_os("PUZZLES_C_SRC").map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../third_party/puzzles-c"),
            PathBuf::from,
        );
        assert!(
            dir.join("puzzles.h").is_file(),
            "C sources not found at {}. Run `git submodule update --init`, \
             or set PUZZLES_C_SRC to an upstream puzzles checkout.",
            dir.display()
        );
        dir
    }
}
