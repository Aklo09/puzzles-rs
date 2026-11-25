//! Test-only bindings to the original C puzzle code (upstream
//! `third_party/puzzles-c`), used as a reference oracle: Rust tests call the
//! C implementation and the port with the same inputs and compare. Those
//! differential tests live in this crate's `tests/*_parity.rs`.
//!
//! Everything is behind the `oracle` feature, which compiles the C sources;
//! without it this crate is empty. See `docs/porting-plan.md` §6.

#[cfg(feature = "oracle")]
mod ffi;

#[cfg(feature = "oracle")]
pub use ffi::{
    CDsf, CDsfKind, CRandom, CTdq, c_bin2hex, c_hex2bin, c_n_times_root_k, c_obfuscate_bitmap,
    c_shuffle, sha1,
};
