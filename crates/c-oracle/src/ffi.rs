//! Raw declarations of the C functions and safe wrappers around them.

// The workspace denies `unsafe_code`; this module is the one place allowed
// to opt out, because calling C is unavoidably unsafe. Every `unsafe` block
// states why its call is sound.
#![allow(unsafe_code, reason = "FFI bindings to the C reference implementation")]

use std::ffi::{c_char, c_int, c_ulong, c_void};
use std::marker::{PhantomData, PhantomPinned};
use std::ptr::NonNull;

/// Opaque C `random_state` (defined privately in `random.c`).
#[repr(C)]
struct RandomStateC {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}

unsafe extern "C" {
    fn random_new(seed: *const c_char, len: c_int) -> *mut RandomStateC;
    fn random_bits(state: *mut RandomStateC, bits: c_int) -> c_ulong;
    fn random_upto(state: *mut RandomStateC, limit: c_ulong) -> c_ulong;
    fn random_free(state: *mut RandomStateC);
    #[link_name = "SHA_Simple"]
    fn sha_simple(p: *const c_void, len: c_int, output: *mut u8);
}

/// An owned C `random_state`: the generator every puzzle uses to turn a
/// seed into a game. The Rust port must reproduce its output exactly.
pub struct CRandom {
    state: NonNull<RandomStateC>,
}

impl CRandom {
    /// Creates a generator from a seed, like C's `random_new(seed, len)`.
    ///
    /// # Panics
    /// If `seed` is longer than `c_int::MAX` bytes.
    #[must_use]
    pub fn new(seed: &[u8]) -> Self {
        let len = c_int::try_from(seed.len()).expect("seed longer than c_int::MAX");
        // SAFETY: `random_new` reads exactly `len` bytes from `seed`, which
        // is valid for that length, and doesn't keep the pointer. It returns
        // a fresh allocation (smalloc aborts instead of returning NULL).
        let state = unsafe { random_new(seed.as_ptr().cast(), len) };
        Self {
            state: NonNull::new(state).expect("random_new returned NULL"),
        }
    }

    /// Returns the next `bits` random bits, like C's `random_bits`.
    ///
    /// # Panics
    /// If `bits` isn't in `1..=32` (C's behaviour is undefined outside it).
    pub fn bits(&mut self, bits: u32) -> u32 {
        assert!(
            (1..=32).contains(&bits),
            "bits must be in 1..=32, got {bits}"
        );
        // SAFETY: `self.state` is a live state owned by `self`, and `bits`
        // is in the range the C function supports.
        let value = unsafe { random_bits(self.state.as_ptr(), bits as c_int) };
        ulong_to_u32(value)
    }

    /// Returns a uniform value in `0..limit`, like C's `random_upto`.
    ///
    /// # Panics
    /// If `limit` is 0 or `limit >= 2^28` (the C code asserts `bits < 32`
    /// after adding 3 guard bits, which would abort the test process).
    pub fn upto(&mut self, limit: u32) -> u32 {
        assert!(
            limit > 0 && limit < 1 << 28,
            "limit must be in 1..2^28, got {limit}"
        );
        // SAFETY: `self.state` is a live state owned by `self`, and `limit`
        // is within the range where the C function's assertion holds.
        let value = unsafe { random_upto(self.state.as_ptr(), u32_to_ulong(limit)) };
        ulong_to_u32(value)
    }
}

// C's `unsigned long` is 32 bits on Windows but 64 bits on Linux and macOS,
// so these conversions are no-ops on one platform and real on the other.
// `allow` rather than `expect`: the lint only fires on Windows.

#[allow(
    clippy::useless_conversion,
    reason = "c_ulong is u32 on Windows, u64 elsewhere"
)]
fn ulong_to_u32(value: c_ulong) -> u32 {
    u32::try_from(value).expect("C returned a value wider than 32 bits")
}

#[allow(
    clippy::useless_conversion,
    reason = "c_ulong is u32 on Windows, u64 elsewhere"
)]
fn u32_to_ulong(value: u32) -> c_ulong {
    c_ulong::from(value)
}

impl Drop for CRandom {
    fn drop(&mut self) {
        // SAFETY: `self.state` came from `random_new`, is owned by `self`
        // and is freed exactly once, here.
        unsafe { random_free(self.state.as_ptr()) }
    }
}

/// SHA-1 digest of `data`, computed by the C code's `SHA_Simple`.
///
/// # Panics
/// If `data` is longer than `c_int::MAX` bytes.
#[must_use]
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let len = c_int::try_from(data.len()).expect("data longer than c_int::MAX");
    let mut digest = [0u8; 20];
    // SAFETY: `SHA_Simple` reads `len` bytes from `data` and writes exactly
    // 20 bytes to `digest`; both buffers are valid for those sizes.
    unsafe { sha_simple(data.as_ptr().cast(), len, digest.as_mut_ptr()) };
    digest
}
