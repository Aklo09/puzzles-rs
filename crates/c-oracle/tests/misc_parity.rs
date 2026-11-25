//! Differential tests: the Rust ports of `misc.c` helpers against C.

#![cfg(feature = "oracle")]

use c_oracle::{CRandom, c_bin2hex, c_hex2bin, c_n_times_root_k, c_obfuscate_bitmap, c_shuffle};
use proptest::prelude::*;
use puzzles_core::misc::{
    bytes_to_hex, deobfuscate_bitmap, hex_to_bytes, n_times_root_k, obfuscate_bitmap,
};
use puzzles_core::random::RandomState;

/// Random bitmap plus a bit count that fits in it (padding bits included,
/// and deliberately not cleared: C must handle that the same way).
fn bitmap_and_bits() -> impl Strategy<Value = (Vec<u8>, usize)> {
    proptest::collection::vec(any::<u8>(), 0..48).prop_flat_map(|bitmap| {
        let max_bits = bitmap.len() * 8;
        (Just(bitmap), 0..=max_bits)
    })
}

proptest! {
    #[test]
    fn shuffle_matches_c_order_and_draws(
        seed in proptest::collection::vec(any::<u8>(), 0..32),
        len in 0..200_usize,
    ) {
        let mut rust_rng = RandomState::new(&seed);
        let mut c_rng = CRandom::new(&seed);
        let mut rust_items: Vec<u32> = (0..len as u32).collect();
        let mut c_items = rust_items.clone();
        rust_rng.shuffle(&mut rust_items);
        c_shuffle(&mut c_items, &mut c_rng);
        prop_assert_eq!(rust_items, c_items);
        // Same number of draws consumed: the generators stay in step.
        prop_assert_eq!(rust_rng.encode(), c_rng.encode());
    }

    #[test]
    fn obfuscation_matches_c((bitmap, bits) in bitmap_and_bits()) {
        let mut rust = bitmap.clone();
        let mut c = bitmap.clone();
        obfuscate_bitmap(&mut rust, bits);
        c_obfuscate_bitmap(&mut c, bits, false);
        prop_assert_eq!(&rust, &c);
    }

    #[test]
    fn deobfuscation_matches_c((bitmap, bits) in bitmap_and_bits()) {
        let mut rust = bitmap.clone();
        let mut c = bitmap.clone();
        deobfuscate_bitmap(&mut rust, bits);
        c_obfuscate_bitmap(&mut c, bits, true);
        prop_assert_eq!(&rust, &c);
    }

    #[test]
    fn hex_encoding_matches_c(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        prop_assert_eq!(bytes_to_hex(&bytes), c_bin2hex(&bytes));
    }

    #[test]
    fn lenient_hex_decoding_matches_c(input in "[0-9a-fA-FgxZ!~ ]{0,64}", len in 0..40_usize) {
        let rust = hex_to_bytes(&input, len);
        if input.len() >= 2 * len {
            prop_assert_eq!(rust, Some(c_hex2bin(&input, len)));
        } else {
            // C asserts here; Rust reports it instead of panicking.
            prop_assert_eq!(rust, None);
        }
    }

    #[test]
    fn n_times_root_k_matches_c_for_large_n(n in -(1_i32 << 24)..(1_i32 << 24), k in 1..=12_i32) {
        prop_assert_eq!(n_times_root_k(n, k), c_n_times_root_k(n, k));
    }
}

/// Exhaustive over the range the grid code actually uses.
#[test]
fn n_times_root_k_matches_c_exhaustively_for_small_n() {
    for k in 1..=12 {
        for n in -20_000..=20_000 {
            assert_eq!(n_times_root_k(n, k), c_n_times_root_k(n, k), "n={n} k={k}");
        }
    }
}

/// Prints the C obfuscation used as the golden value in `puzzles-core`'s
/// unit tests, and checks the Rust port against it.
#[test]
fn obfuscation_golden_value_matches_c() {
    let original: Vec<u8> = (0..16).collect();
    let mut c = original.clone();
    c_obfuscate_bitmap(&mut c, 125, false);
    let mut rust = original;
    obfuscate_bitmap(&mut rust, 125);
    println!("C obfuscation of 0..16 (125 bits): {c:?}");
    assert_eq!(rust, c);
}

/// Prints the C shuffle used as the golden value in `puzzles-core`'s unit
/// tests, and checks the Rust port against it.
#[test]
fn shuffle_golden_value_matches_c() {
    let mut c_items: Vec<u32> = (0..10).collect();
    c_shuffle(&mut c_items, &mut CRandom::new(b"p2r"));
    let mut rust_items: Vec<u32> = (0..10).collect();
    RandomState::new(b"p2r").shuffle(&mut rust_items);
    println!("C shuffle of 0..10 with seed p2r: {c_items:?}");
    assert_eq!(rust_items, c_items);
}
