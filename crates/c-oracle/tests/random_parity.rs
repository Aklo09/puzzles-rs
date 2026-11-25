//! Differential tests: the Rust `RandomState` against C's `random.c`.
//!
//! Every puzzle generated from a seed depends on these producing exactly
//! the same numbers, so we compare whole call sequences, encoded states
//! (which Mines stores in game descriptions) and the lenient decoder.

#![cfg(feature = "oracle")]

use c_oracle::CRandom;
use proptest::prelude::*;
use puzzles_core::random::RandomState;

/// One call on the generator, mirroring how puzzles use it.
#[derive(Clone, Debug)]
enum Op {
    Bits(u32),
    Upto(u32),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (1..=32_u32).prop_map(Op::Bits),
        // Small limits are by far the most common in puzzle code; also
        // cover the whole valid range up to 2^28 - 1.
        (1..=64_u32).prop_map(Op::Upto),
        (1..(1_u32 << 28)).prop_map(Op::Upto),
    ]
}

/// Applies `ops` to both generators and returns their outputs.
fn run(rust: &mut RandomState, c: &mut CRandom, ops: &[Op]) -> (Vec<u32>, Vec<u32>) {
    ops.iter()
        .map(|op| match *op {
            Op::Bits(bits) => (rust.bits(bits), c.bits(bits)),
            Op::Upto(limit) => (rust.upto(limit), c.upto(limit)),
        })
        .unzip()
}

proptest! {
    #[test]
    fn any_seed_and_call_sequence_matches_c(
        seed in proptest::collection::vec(any::<u8>(), 0..100),
        ops in proptest::collection::vec(op(), 0..300),
    ) {
        let mut rust = RandomState::new(&seed);
        let mut c = CRandom::new(&seed);
        let (rust_out, c_out) = run(&mut rust, &mut c, &ops);
        prop_assert_eq!(rust_out, c_out);
        prop_assert_eq!(rust.encode(), c.encode());
    }

    #[test]
    fn decoding_any_string_matches_c(input in "[0-9a-fA-F!z ]{0,140}") {
        let rust = RandomState::decode(&input);
        let c = CRandom::decode(&input);
        prop_assert_eq!(rust.encode(), c.encode());
    }

    /// Well-formed buffers followed by read-position bytes around the
    /// limit of 20 (0x14) and junk: the tricky tail of the decoder, which
    /// random strings rarely reach.
    #[test]
    fn decoding_read_position_tail_matches_c(
        buffers in "[0-9a-f]{120}",
        tail in proptest::collection::vec(
            prop_oneof![(0..=25_u8).prop_map(|b| format!("{b:02x}")), "[0-9a-fA-F!z]{1,2}"],
            0..4,
        ),
    ) {
        let input = format!("{buffers}{}", tail.concat());
        let rust = RandomState::decode(&input);
        let c = CRandom::decode(&input);
        prop_assert_eq!(rust.encode(), c.encode());
    }

    #[test]
    fn decoded_state_continues_like_c(
        seed in proptest::collection::vec(any::<u8>(), 0..32),
        skip in 0..100_usize,
        ops in proptest::collection::vec(op(), 1..100),
    ) {
        // Encode a state mid-sequence (as Mines does), decode it on both
        // sides, and check both continue identically.
        let mut origin = RandomState::new(&seed);
        for _ in 0..skip {
            origin.bits(8);
        }
        let encoded = origin.encode();
        let mut rust = RandomState::decode(&encoded);
        let mut c = CRandom::decode(&encoded);
        let (rust_out, c_out) = run(&mut rust, &mut c, &ops);
        prop_assert_eq!(rust_out, c_out);
    }
}

proptest! {
    /// States whose counter (the first 20 seed bytes) starts with a run of
    /// `0xFF` bytes, with the data buffer exhausted, so the next draw
    /// carries far into the counter. Normal generation never gets here
    /// (it would take ~2^152 refills), but a decoded state can, and Mines
    /// decodes states from game descriptions. `ff_run == 20` covers the
    /// full wraparound to all zeros.
    #[test]
    fn counter_carry_from_decoded_state_matches_c(
        ff_run in 0..=20_usize,
        counter_rest in "[0-9a-f]{40}",
        seed_tail in "[0-9a-f]{40}",
        ops in proptest::collection::vec(op(), 1..20),
    ) {
        // 20 counter bytes, 20 more seed bytes, 20 data bytes, then the
        // read position 0x14 = 20 (buffer exhausted): 122 hex digits.
        let counter = format!("{}{}", "ff".repeat(ff_run), &counter_rest[2 * ff_run..]);
        let encoded = format!("{counter}{seed_tail}{}14", "00".repeat(20));
        prop_assert_eq!(encoded.len(), 122);
        let mut rust = RandomState::decode(&encoded);
        let mut c = CRandom::decode(&encoded);
        prop_assert_eq!(rust.encode(), c.encode());
        let (rust_out, c_out) = run(&mut rust, &mut c, &ops);
        prop_assert_eq!(rust_out, c_out);
        prop_assert_eq!(rust.encode(), c.encode());
    }
}

/// A long run crosses thousands of refills, so the 20-byte counter carries
/// through `0xFF` bytes many times; any off-by-one there shows up here.
#[test]
fn long_sequence_matches_c_across_counter_carries() {
    let mut rust = RandomState::new(b"long run");
    let mut c = CRandom::new(b"long run");
    for i in 0..200_000 {
        assert_eq!(rust.bits(32), c.bits(32), "diverged at draw {i}");
    }
    assert_eq!(rust.encode(), c.encode());
}
