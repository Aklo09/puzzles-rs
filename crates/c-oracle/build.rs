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
    use std::path::PathBuf;

    /// Upstream C files compiled into the oracle. Grows as more of the
    /// engine and the games are ported and need a reference.
    const C_SOURCES: &[&str] = &[
        "divvy.c",
        "dsf.c",
        "findloop.c",
        "laydomino.c",
        "malloc.c",
        "matching.c",
        "misc.c",
        "random.c",
        "tdq.c",
    ];

    pub fn build() {
        println!("cargo::rerun-if-env-changed=PUZZLES_C_SRC");
        let c_dir = c_source_dir();

        let shim = PathBuf::from("csrc/shim.c");
        println!("cargo::rerun-if-changed={}", shim.display());
        println!(
            "cargo::rerun-if-changed={}",
            c_dir.join("puzzles.h").display()
        );

        let mut build = cc::Build::new();
        build.include(&c_dir).file(&shim).warnings(false);
        for source in C_SOURCES {
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
