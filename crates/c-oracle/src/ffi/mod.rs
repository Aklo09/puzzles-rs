//! Raw declarations of the C functions and safe wrappers around them, one
//! submodule per upstream C file.

// The workspace denies `unsafe_code`; this module (and its submodules) is
// the one place allowed to opt out, because calling C is unavoidably
// unsafe. Every `unsafe` block states why its call is sound.
#![allow(unsafe_code, reason = "FFI bindings to the C reference implementation")]

mod dsf;
mod random;
mod tdq;

pub use dsf::{CDsf, CDsfKind};
pub use random::{CRandom, sha1};
pub use tdq::CTdq;

use std::ffi::c_void;

unsafe extern "C" {
    /// `malloc.c`: frees memory the C code allocated (`smalloc`, `dupstr`).
    fn sfree(p: *mut c_void);
}
