//! Raw declarations of the C functions and safe wrappers around them, one
//! submodule per upstream C file.

// The workspace denies `unsafe_code`; this module (and its submodules) is
// the one place allowed to opt out, because calling C is unavoidably
// unsafe. Every `unsafe` block states why its call is sound.
#![allow(unsafe_code, reason = "FFI bindings to the C reference implementation")]

mod divvy;
mod dsf;
mod findloop;
mod latin;
mod laydomino;
mod matching;
mod misc;
mod random;
mod tdq;

pub use divvy::c_divvy_rectangle_attempt;
pub use dsf::{CDsf, CDsfKind};
pub use findloop::CFindLoop;
pub use latin::{
    C_DIFF_AMBIGUOUS, C_DIFF_IMPOSSIBLE, C_DIFF_UNFINISHED, C_MAX_LEVELS, CLatinRules,
    CLatinSolver, CLatinView, CLevels, c_latin_check, c_latin_generate, c_latin_generate_rect,
};
pub use laydomino::c_domino_layout;
pub use matching::{CMatching, c_matching, c_matching_with_scratch};
pub use misc::{c_bin2hex, c_hex2bin, c_n_times_root_k, c_obfuscate_bitmap, c_shuffle};
pub use random::{CRandom, sha1};
pub use tdq::CTdq;

use std::ffi::c_void;

unsafe extern "C" {
    /// `malloc.c`: frees memory the C code allocated (`smalloc`, `dupstr`).
    fn sfree(p: *mut c_void);
}
