//! Checks that the C reference compiles, links and behaves identically on
//! every platform (Windows/x86 with MSVC, macOS/ARM with clang, Linux).

#![cfg(feature = "oracle")]

use std::fmt::Write as _;

use c_oracle::{CRandom, sha1};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        write!(out, "{b:02x}").expect("writing to a String can't fail");
        out
    })
}

#[test]
fn sha1_of_abc_matches_fips_180_vector() {
    assert_eq!(
        hex(&sha1(b"abc")),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
}

#[test]
fn sha1_of_empty_input_matches_fips_180_vector() {
    assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
}

#[test]
fn sha1_of_two_block_message_matches_fips_180_vector() {
    let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(hex(&sha1(msg)), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
}

#[test]
fn random_bits_with_same_seed_produce_same_sequence() {
    let mut a = CRandom::new(b"p2r");
    let mut b = CRandom::new(b"p2r");
    let seq_a: Vec<u32> = (0..64).map(|_| a.bits(32)).collect();
    let seq_b: Vec<u32> = (0..64).map(|_| b.bits(32)).collect();
    assert_eq!(seq_a, seq_b);
}

#[test]
fn random_bits_with_different_seeds_produce_different_sequences() {
    let mut a = CRandom::new(b"p2r");
    let mut b = CRandom::new(b"p2s");
    let seq_a: Vec<u32> = (0..8).map(|_| a.bits(32)).collect();
    let seq_b: Vec<u32> = (0..8).map(|_| b.bits(32)).collect();
    assert_ne!(seq_a, seq_b);
}

#[test]
fn random_bits_never_exceed_requested_width() {
    let mut rng = CRandom::new(b"width");
    for bits in 1..=32 {
        let max = if bits == 32 {
            u32::MAX
        } else {
            (1 << bits) - 1
        };
        assert!((0..100).all(|_| rng.bits(bits) <= max), "bits = {bits}");
    }
}

#[test]
fn random_upto_stays_below_limit() {
    let mut rng = CRandom::new(b"upto");
    for limit in [1, 2, 3, 7, 10, 100, 1000, (1 << 28) - 1] {
        assert!((0..200).all(|_| rng.upto(limit) < limit), "limit = {limit}");
    }
}

/// Golden values recorded from the C code. They must be identical on every
/// OS and architecture: that's what makes `params#seed` game IDs portable,
/// and it's the reference the Rust port of `random.c` must reproduce.
///
/// The first five `bits(32)` values were also derived independently from
/// the algorithm with Python's `hashlib` (SHA-1 of the seed, of that
/// digest, then of both concatenated, read as big-endian u32s).
#[test]
fn random_sequence_matches_recorded_golden_values() {
    let mut rng = CRandom::new(b"p2r");
    let bits32: Vec<u32> = (0..6).map(|_| rng.bits(32)).collect();
    let bits5: Vec<u32> = (0..6).map(|_| rng.bits(5)).collect();
    let upto: Vec<u32> = (0..6).map(|_| rng.upto(1000)).collect();
    assert_eq!(
        bits32,
        [
            3_136_067_529,
            2_536_548_918,
            2_491_443_338,
            809_245_359,
            2_371_454_992,
            2_304_575_249
        ]
    );
    assert_eq!(bits5, [9, 2, 23, 16, 10, 28]);
    assert_eq!(upto, [763, 20, 829, 554, 847, 775]);
}
