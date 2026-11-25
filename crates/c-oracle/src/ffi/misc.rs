//! Bindings to `misc.c`: the engine-independent helpers ported so far.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

use super::random::RandomStateC;
use super::{CRandom, sfree};

unsafe extern "C" {
    fn shuffle(array: *mut c_void, nelts: c_int, eltsize: c_int, rs: *mut RandomStateC);
    fn obfuscate_bitmap(bmp: *mut u8, bits: c_int, decode: bool);
    fn bin2hex(input: *const u8, inlen: c_int) -> *mut c_char;
    fn hex2bin(input: *const c_char, outlen: c_int) -> *mut u8;
    fn n_times_root_k(n: c_int, k: c_int) -> c_int;
}

/// C's `shuffle` on a slice of `u32`, drawing from `rng`.
///
/// # Panics
/// If the slice is longer than `c_int::MAX` elements.
pub fn c_shuffle(items: &mut [u32], rng: &mut CRandom) {
    let nelts = c_int::try_from(items.len()).expect("too many elements");
    let eltsize = c_int::try_from(size_of::<u32>()).expect("u32 size fits");
    // SAFETY: `items` is valid for `nelts * eltsize` bytes of reads and
    // writes; `shuffle` only swaps elements within it. The rng state is
    // live and exclusively borrowed for the call.
    unsafe { shuffle(items.as_mut_ptr().cast(), nelts, eltsize, rng.as_mut_ptr()) }
}

/// C's `obfuscate_bitmap(bitmap, bits, decode)`.
///
/// # Panics
/// If `bitmap` is shorter than `bits.div_ceil(8)` bytes (C would overrun it).
pub fn c_obfuscate_bitmap(bitmap: &mut [u8], bits: usize, decode: bool) {
    assert!(bitmap.len() >= bits.div_ceil(8), "bitmap too short");
    let bits = c_int::try_from(bits).expect("bits exceeds c_int::MAX");
    // SAFETY: `bitmap` holds at least `ceil(bits / 8)` bytes, which is all
    // the function reads or writes.
    unsafe { obfuscate_bitmap(bitmap.as_mut_ptr(), bits, decode) }
}

/// C's `bin2hex`.
///
/// # Panics
/// If `bytes` is longer than `c_int::MAX`.
#[must_use]
pub fn c_bin2hex(bytes: &[u8]) -> String {
    let len = c_int::try_from(bytes.len()).expect("too many bytes");
    // SAFETY: `bin2hex` reads `len` bytes from `bytes` and returns a fresh
    // NUL-terminated string (smalloc never returns NULL for size >= 1).
    let hex = unsafe { bin2hex(bytes.as_ptr(), len) };
    assert!(!hex.is_null(), "bin2hex returned NULL");
    // SAFETY: `hex` is a valid NUL-terminated string until freed below.
    let text = unsafe { CStr::from_ptr(hex) }
        .to_str()
        .expect("bin2hex wrote non-UTF-8")
        .to_owned();
    // SAFETY: `hex` came from smalloc, is freed exactly once, and isn't
    // used afterwards.
    unsafe { sfree(hex.cast()) };
    text
}

/// C's `hex2bin(input, len)`.
///
/// # Panics
/// If `input` is shorter than `2 * len` bytes or has a NUL in that range
/// (C asserts, which would abort the test process).
#[must_use]
pub fn c_hex2bin(input: &str, len: usize) -> Vec<u8> {
    let digits = input.as_bytes().get(..2 * len).expect("input too short");
    assert!(!digits.contains(&0), "NUL inside the hex digits");
    if len == 0 {
        // smalloc(0) returns NULL in C; there is nothing to decode.
        return Vec::new();
    }
    let input =
        CString::new(input.split('\0').next().unwrap_or_default()).expect("NULs were stripped");
    let outlen = c_int::try_from(len).expect("len exceeds c_int::MAX");
    // SAFETY: `input` is NUL-terminated and its first `2 * len` bytes are
    // non-NUL (checked above), which is all `hex2bin` reads. It returns a
    // fresh allocation of `len` bytes.
    let bytes = unsafe { hex2bin(input.as_ptr(), outlen) };
    assert!(!bytes.is_null(), "hex2bin returned NULL");
    // SAFETY: `bytes` points to `len` initialised bytes until freed below.
    let out = unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec();
    // SAFETY: `bytes` came from smalloc, is freed exactly once, and isn't
    // used afterwards.
    unsafe { sfree(bytes.cast()) };
    out
}

/// C's `n_times_root_k`.
#[must_use]
pub fn c_n_times_root_k(n: i32, k: i32) -> i32 {
    // SAFETY: a pure function of two integers. (Its internal assertions
    // hold for the inputs the tests use; a failure would abort the tests.)
    unsafe { n_times_root_k(n, k) }
}
