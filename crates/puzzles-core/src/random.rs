//! Port of `third_party/puzzles-c/random.c` (upstream 616da16).
//!
//! The random number generator behind every puzzle. It must reproduce the
//! C generator bit for bit: a `params#seed` game ID produces the same
//! puzzle on every platform and in every version only because every
//! implementation draws exactly the same numbers from the same seed.
//!
//! The generator hashes the seed with SHA-1 into a 40-byte seed buffer,
//! then repeatedly hashes that buffer (incrementing its first 20 bytes as a
//! little-endian counter) to produce 20 bytes of output at a time. SHA-1
//! itself is standard, so it comes from `sha1_smol`; everything on top of
//! it is ported from C.

use std::fmt::Write as _;

const SEED_LEN: usize = 40;
const COUNTER_LEN: usize = 20;
const DATA_LEN: usize = 20;

/// Number of bytes in an encoded state: seed buffer, data buffer and the
/// read position.
const ENCODED_BYTES: usize = SEED_LEN + DATA_LEN + 1;

/// State of the puzzles' random number generator (C's `random_state`).
///
/// Cloning gives an independent copy that continues the same sequence
/// (C's `random_copy`).
///
/// ```
/// use puzzles_core::random::RandomState;
///
/// let mut rng = RandomState::new(b"p2r");
/// assert_eq!(rng.bits(32), 3_136_067_529);
/// assert!(rng.upto(6) < 6);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomState {
    seedbuf: [u8; SEED_LEN],
    databuf: [u8; DATA_LEN],
    /// Index of the next unused byte of `databuf`; `DATA_LEN` means all
    /// bytes are used and the next draw refills the buffer.
    pos: usize,
}

fn sha1(data: &[u8]) -> [u8; 20] {
    sha1_smol::Sha1::from(data).digest().bytes()
}

impl RandomState {
    /// Creates a generator from an arbitrary seed (C's `random_new`).
    #[must_use]
    pub fn new(seed: &[u8]) -> Self {
        let mut seedbuf = [0; SEED_LEN];
        let first = sha1(seed);
        seedbuf[..20].copy_from_slice(&first);
        seedbuf[20..].copy_from_slice(&sha1(&first));
        Self {
            databuf: sha1(&seedbuf),
            seedbuf,
            pos: 0,
        }
    }

    /// Returns the next `bits` random bits (C's `random_bits`).
    ///
    /// Whole bytes are always consumed: asking for 5 bits uses up a full
    /// byte, exactly like the C code, so the sequence of calls (not just
    /// the values) must match C for later results to match.
    ///
    /// # Panics
    /// If `bits` isn't in `1..=32`. C has undefined behaviour outside this
    /// range, and no caller asks for more than 32 bits.
    pub fn bits(&mut self, bits: u32) -> u32 {
        assert!(
            (1..=32).contains(&bits),
            "bits must be in 1..=32, got {bits}"
        );
        let mut value: u32 = 0;
        for _ in (0..bits).step_by(8) {
            if self.pos >= DATA_LEN {
                self.refill();
            }
            // At most four bytes are read, so nothing is shifted out of a u32
            // (C uses `unsigned long`, which is at least 32 bits).
            value = (value << 8) | u32::from(self.databuf[self.pos]);
            self.pos += 1;
        }
        let mask = if bits == 32 {
            u32::MAX
        } else {
            (1 << bits) - 1
        };
        value & mask
    }

    /// Returns a uniformly distributed value in `0..limit` (C's
    /// `random_upto`), by rejection sampling with three extra bits.
    ///
    /// # Panics
    /// If `limit` is 0, or `limit >= 2^28`: C asserts that the number of
    /// bits needed (the bit length of `limit` plus 3) stays below 32.
    pub fn upto(&mut self, limit: u32) -> u32 {
        assert!(limit > 0, "random_upto limit must be positive");
        let bits = limit.bit_width() + 3;
        assert!(bits < 32, "random_upto limit too large: {limit}");

        let divisor = (1 << bits) / limit;
        let max = limit * divisor;
        loop {
            let data = self.bits(bits);
            if data < max {
                return data / divisor;
            }
        }
    }

    /// Shuffles `items` in place (C's `shuffle`, in `misc.c`).
    ///
    /// A Fisher-Yates shuffle from the back: for each position `i` from
    /// `len - 1` down to 1, swap with `upto(i + 1)`. Same draws in the same
    /// order as C, so generators that shuffle produce the same puzzles.
    ///
    /// # Panics
    /// If `items` has `2^28` elements or more (see [`upto`](Self::upto)).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            // Lengths past u32 saturate, so `upto` reports them as too large.
            let limit = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let j = self.upto(limit) as usize;
            // C skips the swap when j == i; swapping an element with itself
            // is a no-op, so the result is the same.
            items.swap(i, j);
        }
    }

    /// Encodes the full state as lowercase hex (C's
    /// `random_state_encode`): seed buffer, data buffer, then the read
    /// position, two hex digits each (122 characters).
    #[must_use]
    pub fn encode(&self) -> String {
        // Writing to a String can't fail, so the fmt results are ignored.
        let mut out = String::with_capacity(2 * ENCODED_BYTES);
        for byte in self.seedbuf.iter().chain(&self.databuf) {
            let _ = write!(out, "{byte:02x}");
        }
        // `pos` is at most 20, so it is always exactly two hex digits.
        let _ = write!(out, "{:02x}", self.pos);
        out
    }

    /// Decodes a state written by [`RandomState::encode`] (C's
    /// `random_state_decode`).
    ///
    /// Never fails, mirroring C's leniency so that any string the C code
    /// accepts decodes to the same state: a character that isn't a hex
    /// digit counts as `0`, missing bytes stay zero, a read position
    /// above 20 is ignored, and every complete byte after the data buffer
    /// that is at most 20 overwrites the read position.
    #[must_use]
    pub fn decode(input: &str) -> Self {
        let mut state = Self {
            seedbuf: [0; SEED_LEN],
            databuf: [0; DATA_LEN],
            pos: 0,
        };
        let mut byte: u8 = 0;
        let mut digits = 0;
        let mut index = 0;
        for c in input.bytes() {
            let nibble = match c {
                b'0'..=b'9' => c - b'0',
                b'A'..=b'F' => c - b'A' + 10,
                b'a'..=b'f' => c - b'a' + 10,
                _ => 0,
            };
            byte = (byte << 4) | nibble;
            digits += 1;
            if digits == 2 {
                if index < SEED_LEN {
                    state.seedbuf[index] = byte;
                    index += 1;
                } else if index < SEED_LEN + DATA_LEN {
                    state.databuf[index - SEED_LEN] = byte;
                    index += 1;
                } else if usize::from(byte) <= DATA_LEN {
                    // C doesn't advance past this slot, so later bytes keep
                    // overwriting the position.
                    state.pos = usize::from(byte);
                }
                byte = 0;
                digits = 0;
            }
        }
        state
    }

    /// Advances the counter in the first 20 seed bytes (little-endian,
    /// carrying through `0xFF` bytes) and rehashes into the data buffer.
    fn refill(&mut self) {
        for byte in &mut self.seedbuf[..COUNTER_LEN] {
            if *byte == 0xFF {
                *byte = 0;
            } else {
                *byte += 1;
                break;
            }
        }
        self.databuf = sha1(&self.seedbuf);
        self.pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0..10` shuffled with seed `"p2r"` by the C code, recorded from the
    /// oracle (`misc_parity.rs` re-checks it against C).
    const GOLDEN_SHUFFLE_P2R_10: [u32; 10] = [0, 6, 3, 5, 9, 2, 8, 1, 7, 4];

    /// Same golden values as the C oracle's test: recorded from the C code,
    /// first five also derived independently with Python's `hashlib`.
    #[test]
    fn sequence_matches_c_golden_values() {
        let mut rng = RandomState::new(b"p2r");
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

    #[test]
    fn clone_continues_the_same_sequence() {
        let mut rng = RandomState::new(b"clone");
        rng.bits(13);
        let mut copy = rng.clone();
        let original: Vec<u32> = (0..50).map(|_| rng.bits(32)).collect();
        let copied: Vec<u32> = (0..50).map(|_| copy.bits(32)).collect();
        assert_eq!(original, copied);
    }

    #[test]
    fn bits_never_exceed_requested_width() {
        let mut rng = RandomState::new(b"width");
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
    fn upto_stays_below_limit() {
        let mut rng = RandomState::new(b"upto");
        for limit in [1, 2, 3, 7, 10, 100, 1000, (1 << 28) - 1] {
            assert!((0..200).all(|_| rng.upto(limit) < limit), "limit = {limit}");
        }
    }

    #[test]
    #[should_panic(expected = "bits must be in 1..=32")]
    fn bits_panics_on_zero() {
        RandomState::new(b"").bits(0);
    }

    #[test]
    #[should_panic(expected = "bits must be in 1..=32")]
    fn bits_panics_above_32() {
        RandomState::new(b"").bits(33);
    }

    #[test]
    #[should_panic(expected = "limit must be positive")]
    fn upto_panics_on_zero_limit() {
        RandomState::new(b"").upto(0);
    }

    #[test]
    #[should_panic(expected = "limit too large")]
    fn upto_panics_on_limit_of_2_pow_28() {
        RandomState::new(b"").upto(1 << 28);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut items: Vec<u32> = (0..50).collect();
        RandomState::new(b"perm").shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_of_zero_or_one_items_draws_nothing() {
        let mut rng = RandomState::new(b"none");
        let before = rng.clone();
        rng.shuffle::<u8>(&mut []);
        rng.shuffle(&mut [42]);
        assert_eq!(rng, before);
    }

    /// Recorded from the C `shuffle`; also checked live by the oracle.
    #[test]
    fn shuffle_matches_c_golden_order() {
        let mut items: Vec<u32> = (0..10).collect();
        RandomState::new(b"p2r").shuffle(&mut items);
        assert_eq!(items, GOLDEN_SHUFFLE_P2R_10);
    }

    #[test]
    fn encode_writes_122_lowercase_hex_digits() {
        let mut rng = RandomState::new(b"encode");
        rng.bits(24);
        let encoded = rng.encode();
        assert_eq!(encoded.len(), 122);
        assert!(
            encoded
                .bytes()
                .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert!(encoded.ends_with("03"), "read position 3 is the last byte");
    }

    #[test]
    fn decode_of_encode_restores_the_state() {
        let mut rng = RandomState::new(b"roundtrip");
        for _ in 0..37 {
            rng.bits(7);
        }
        assert_eq!(RandomState::decode(&rng.encode()), rng);
    }

    #[test]
    fn decode_accepts_uppercase_hex() {
        let rng = RandomState::new(b"upper");
        let upper = rng.encode().to_ascii_uppercase();
        assert_eq!(RandomState::decode(&upper), rng);
    }

    #[test]
    fn decode_treats_non_hex_characters_as_zero() {
        let state = RandomState::decode("1z");
        assert_eq!(state.seedbuf[0], 0x10);
    }

    #[test]
    fn decode_of_empty_string_is_all_zero() {
        let state = RandomState::decode("");
        assert_eq!(state.encode(), "0".repeat(122));
    }

    #[test]
    fn decode_ignores_read_position_above_20() {
        let mut encoded = "00".repeat(60);
        encoded.push_str("15");
        assert_eq!(RandomState::decode(&encoded).pos, 0);
    }

    #[test]
    fn decode_lets_trailing_bytes_overwrite_read_position() {
        let mut encoded = "00".repeat(60);
        encoded.push_str("05ff07");
        assert_eq!(RandomState::decode(&encoded).pos, 7);
    }

    /// Exhausted state whose 20-byte counter is `0xFF` in its first
    /// `ff_run` bytes and `0x00` after.
    fn exhausted_state_with_ff_run(ff_run: usize) -> RandomState {
        let mut state = RandomState::decode("");
        state.seedbuf[..ff_run].fill(0xFF);
        state.pos = DATA_LEN;
        state
    }

    #[test]
    fn refill_carries_through_leading_ff_bytes() {
        let mut state = exhausted_state_with_ff_run(5);
        state.bits(8);
        assert_eq!(state.seedbuf[..6], [0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn refill_wraps_an_all_ff_counter_to_zero() {
        let mut state = exhausted_state_with_ff_run(COUNTER_LEN);
        state.bits(8);
        assert_eq!(state.seedbuf[..COUNTER_LEN], [0; COUNTER_LEN]);
    }

    #[test]
    fn refill_rehashes_the_whole_seed_buffer() {
        let mut state = exhausted_state_with_ff_run(0);
        state.bits(8);
        assert_eq!(state.databuf, sha1(&state.seedbuf));
    }
}
