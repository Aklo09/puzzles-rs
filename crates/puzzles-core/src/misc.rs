//! Port of the engine-independent parts of `third_party/puzzles-c/misc.c`
//! (upstream 616da16).
//!
//! Elsewhere: `button2label` is [`Button::label`]; the drawing helpers
//! (`draw_rect_outline`, `draw_rect_corners`, `draw_text_outline`) are
//! methods of [`Drawing`](crate::drawing::Drawing); `game_mkhighlight` and
//! `colour_mix` are theme roles and derived colours (D7); `c2pos`,
//! `c2diff` and `pos2c` arrive with Netslide and Sixteen; `free_*` are
//! `Drop`; `shuffle` is
//! [`RandomState::shuffle`](crate::random::RandomState::shuffle);
//! `swap_regions` and `compare_integers` are `slice::swap` and `Ord::cmp`;
//! `fgetline`, `make_prefs_path` and `copy_left_justified` belong to front
//! ends.

use std::ffi::OsStr;

use crate::button::{Button, ButtonKind, CursorKey};
use crate::game::MoveResult;

/// Obfuscates the first `bits` bits of `bitmap` in place (C's
/// `obfuscate_bitmap(bmp, bits, false)`).
///
/// Mines, Blackbox and Guess store their solution in the game description;
/// this keyless, reversible masking stops it from being readable at a
/// glance. It is not encryption. The output must match C exactly, since
/// it is part of game IDs.
///
/// The bitmap is padded to whole bytes and split in half (first half
/// rounded down). A mask derived (with SHA-1) from the second half is
/// combined by exclusive-or into the first half, then one derived from the
/// (now masked) first half into the second. Padding bits are cleared after
/// each step.
///
/// # Panics
/// If `bitmap` is shorter than `bits.div_ceil(8)` bytes.
pub fn obfuscate_bitmap(bitmap: &mut [u8], bits: usize) {
    mask_bitmap(bitmap, bits, Direction::Obfuscate);
}

/// Reverses [`obfuscate_bitmap`] (C's `obfuscate_bitmap(bmp, bits,
/// true)`): the same two steps in the opposite order.
///
/// # Panics
/// If `bitmap` is shorter than `bits.div_ceil(8)` bytes.
#[doc(alias = "obfuscate_bitmap")]
pub fn deobfuscate_bitmap(bitmap: &mut [u8], bits: usize) {
    mask_bitmap(bitmap, bits, Direction::Deobfuscate);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Obfuscate,
    Deobfuscate,
}

fn mask_bitmap(bitmap: &mut [u8], bits: usize, direction: Direction) {
    let bytes = bits.div_ceil(8);
    assert!(
        bitmap.len() >= bytes,
        "bitmap has {} bytes, {bits} bits need {bytes}",
        bitmap.len()
    );
    let first_half = 0..bytes / 2;
    let second_half = bytes / 2..bytes;

    // Each step: (seed range, target range).
    let mask_first = (second_half.clone(), first_half.clone());
    let mask_second = (first_half, second_half);
    let steps = match direction {
        Direction::Obfuscate => [mask_first, mask_second],
        Direction::Deobfuscate => [mask_second, mask_first],
    };

    for (seed, target) in steps {
        let mut base = sha1_smol::Sha1::new();
        base.update(&bitmap[seed]);
        // The mask is SHA-1(seed || "0"), SHA-1(seed || "1"), ... chained.
        let mask = (0_u32..).flat_map(|counter| {
            let mut hash = base.clone();
            hash.update(counter.to_string().as_bytes());
            hash.digest().bytes()
        });
        for (byte, mask_byte) in bitmap[target].iter_mut().zip(mask) {
            *byte ^= mask_byte;
        }
        // Clear the padding bits in the final byte. C does this after each
        // step, which matters: the next step hashes these bytes.
        if !bits.is_multiple_of(8) {
            bitmap[bits / 8] &= ((0xFF00_u32 >> (bits % 8)) & 0xFF) as u8;
        }
    }
}

/// Lowercase hex encoding of `bytes` (C's `bin2hex`).
#[doc(alias = "bin2hex")]
#[must_use]
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Decodes `len` bytes from the first `2 * len` characters of `input`
/// (C's `hex2bin`).
///
/// Deliberately as lenient as C, because game descriptions are only
/// length-checked before decoding: any character that isn't a hex digit
/// counts as `0`, and characters after the first `2 * len` are ignored.
/// Where C would assert (input shorter than `2 * len`, or a NUL inside
/// it), this returns `None` instead, since descriptions are untrusted.
#[doc(alias = "hex2bin")]
#[must_use]
pub fn hex_to_bytes(input: &str, len: usize) -> Option<Vec<u8>> {
    let digits = input.as_bytes().get(..2 * len)?;
    if digits.contains(&0) {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    };
    // `digits` has even length, so there is no remainder.
    let (pairs, _) = digits.as_chunks::<2>();
    Some(
        pairs
            .iter()
            .map(|&[high, low]| (nibble(high) << 4) | nibble(low))
            .collect(),
    )
}

/// `n * sqrt(k)` rounded to the nearest integer, computed exactly in
/// integers (C's `n_times_root_k`; the grid code uses it for the
/// irrational coordinates of hexagonal, triangular and other tilings).
///
/// The arithmetic is on `u32` and wraps exactly like C's `unsigned`, so the
/// result matches C for every input, including ones large enough to wrap.
///
/// # Panics
/// Only if the algorithm's internal invariants fail, as C's assertions
/// would (they hold for all inputs the grid code uses).
#[must_use]
pub fn n_times_root_k(n: i32, k: i32) -> i32 {
    let negative = n < 0;
    let n = n.unsigned_abs();
    // C converts `k` to unsigned when mixing it with unsigned operands.
    let k = k.cast_unsigned();

    // Invariants (mod 2^32, as in C): x^2 + r == k m^2, and r < 2x + 1,
    // where m is the prefix of n consumed so far and x = floor(m sqrt(k)).
    let (mut x, mut r, mut m) = (0_u32, 0_u32, 0_u32);
    for bitpos in (0..u32::BITS).rev().map(|shift| 1_u32 << shift) {
        let b = u32::from(n & bitpos != 0);
        assert_eq!(
            x.wrapping_mul(x).wrapping_add(r),
            k.wrapping_mul(m).wrapping_mul(m),
            "n_times_root_k: residual invariant broken"
        );
        assert!(
            r < x.wrapping_mul(2).wrapping_add(1),
            "n_times_root_k: x is not maximal"
        );

        // Replace m by 2m+b and x by 2x+a, with a the largest digit that
        // keeps the new residual non-negative.
        let pos = r.wrapping_mul(4).wrapping_add(
            k.wrapping_mul(b)
                .wrapping_mul(m.wrapping_mul(4).wrapping_add(1)),
        );
        let neg = |a: u32| {
            a.wrapping_mul(4)
                .wrapping_mul(x)
                .wrapping_add(a.wrapping_mul(a))
        };
        let mut a = 0;
        while pos >= neg(a) {
            a += 1;
        }
        // The loop stops one past the right digit.
        let a = a - 1;

        r = pos.wrapping_sub(neg(a));
        m = m.wrapping_mul(2).wrapping_add(b);
        x = x.wrapping_mul(2).wrapping_add(a);
    }

    // Round to nearest: (x + 1/2)^2 < k m^2 exactly when r > x. sqrt(k) is
    // irrational (or an integer), so there are no exact halves to break.
    if r > x {
        x = x.wrapping_add(1);
    }
    let x = x.cast_signed();
    if negative { x.wrapping_neg() } else { x }
}

/// Reads a yes/no environment variable (C's `getenv_bool`): `None` if it is
/// unset, otherwise `Some(true)` if its value starts with `y`, `Y`, `t` or
/// `T`, and `Some(false)` for anything else.
///
/// Callers pick their own default with `unwrap_or`. C takes the default as
/// a parameter and some callers pass `-1` for "no preference", which is
/// what `None` expresses.
///
/// Quirk kept from C: an empty value counts as true (C's `strchr` matches
/// the string terminator).
#[doc(alias = "getenv_bool")]
#[must_use]
pub fn env_bool(name: &str) -> Option<bool> {
    std::env::var_os(name).map(|value| parse_env_bool(&value))
}

fn parse_env_bool(value: &OsStr) -> bool {
    matches!(
        value.as_encoded_bytes().first(),
        None | Some(b'y' | b'Y' | b't' | b'T')
    )
}

/// Moves a cursor at `(x, y)` one step for an arrow key, within a
/// `max_w` × `max_h` grid, wrapping round if `wrap` and stopping at the
/// edges otherwise (C's `move_cursor`).
///
/// Returns [`MoveResult::Unused`] for anything but an unmodified arrow
/// key, [`MoveResult::UiUpdate`] if the cursor moved or was just made
/// visible (via `visible`, if given), and [`MoveResult::NoEffect`]
/// otherwise.
///
/// # Panics
/// If `wrap` is set and the grid is empty (C divides by zero).
pub fn move_cursor<M>(
    button: Button,
    x: &mut i32,
    y: &mut i32,
    max_w: i32,
    max_h: i32,
    wrap: bool,
    visible: Option<&mut bool>,
) -> MoveResult<M> {
    // C compares the whole code, so a modified arrow key is unused.
    let (dx, dy) = match (button.kind, button.mods.is_empty()) {
        (ButtonKind::Cursor(CursorKey::Up), true) => (0, -1),
        (ButtonKind::Cursor(CursorKey::Down), true) => (0, 1),
        (ButtonKind::Cursor(CursorKey::Right), true) => (1, 0),
        (ButtonKind::Cursor(CursorKey::Left), true) => (-1, 0),
        _ => return MoveResult::Unused,
    };
    let (ox, oy) = (*x, *y);
    if wrap {
        *x = (*x + dx + max_w) % max_w;
        *y = (*y + dy + max_h) % max_h;
    } else {
        *x = (*x + dx).max(0).min(max_w - 1);
        *y = (*y + dy).max(0).min(max_h - 1);
    }
    if let Some(visible) = visible
        && !*visible
    {
        *visible = true;
        return MoveResult::UiUpdate;
    }
    if (*x, *y) == (ox, oy) {
        MoveResult::NoEffect
    } else {
        MoveResult::UiUpdate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscate_then_deobfuscate_restores_the_bitmap() {
        let original: Vec<u8> = (0..23_u32).map(|i| (i * 37 + 11) as u8).collect();
        let mut bitmap = original.clone();
        obfuscate_bitmap(&mut bitmap, 23 * 8);
        assert_ne!(bitmap, original);
        deobfuscate_bitmap(&mut bitmap, 23 * 8);
        assert_eq!(bitmap, original);
    }

    /// Recorded from the C `obfuscate_bitmap` (`misc_parity.rs` re-checks
    /// it). A round trip can't catch a wrong-but-symmetric mask; this can.
    #[test]
    fn obfuscation_matches_c_golden_value() {
        let mut bitmap: Vec<u8> = (0..16).collect();
        obfuscate_bitmap(&mut bitmap, 125);
        assert_eq!(
            bitmap,
            [
                106, 82, 9, 224, 0, 77, 0, 124, 178, 79, 204, 93, 171, 147, 16, 120
            ]
        );
    }

    #[test]
    fn obfuscation_keeps_padding_bits_clear() {
        // 81 bits = 11 bytes, 1 bit used in the last byte.
        let mut bitmap = [0xFF_u8; 11];
        bitmap[10] = 0x80;
        obfuscate_bitmap(&mut bitmap, 81);
        assert_eq!(bitmap[10] & 0x7F, 0);
    }

    #[test]
    fn obfuscation_ignores_bytes_past_the_bitmap() {
        let mut bitmap = [1, 2, 3, 4, 0xAA];
        obfuscate_bitmap(&mut bitmap, 32);
        assert_eq!(bitmap[4], 0xAA);
    }

    #[test]
    fn obfuscation_of_zero_bits_is_a_no_op() {
        let mut bitmap = [7_u8; 3];
        obfuscate_bitmap(&mut bitmap, 0);
        assert_eq!(bitmap, [7; 3]);
    }

    #[test]
    #[should_panic(expected = "bits need")]
    fn obfuscation_panics_on_short_bitmap() {
        obfuscate_bitmap(&mut [0; 2], 17);
    }

    #[test]
    fn bytes_to_hex_is_lowercase() {
        assert_eq!(bytes_to_hex(&[0x00, 0xAB, 0x5f]), "00ab5f");
    }

    #[test]
    fn hex_to_bytes_decodes_mixed_case() {
        assert_eq!(hex_to_bytes("00aB5F", 3), Some(vec![0x00, 0xAB, 0x5F]));
    }

    #[test]
    fn hex_to_bytes_treats_non_hex_as_zero() {
        assert_eq!(hex_to_bytes("z1g2", 2), Some(vec![0x01, 0x02]));
    }

    #[test]
    fn hex_to_bytes_ignores_trailing_characters() {
        assert_eq!(hex_to_bytes("abcdef", 1), Some(vec![0xAB]));
    }

    #[test]
    fn hex_to_bytes_rejects_short_input() {
        assert_eq!(hex_to_bytes("abc", 2), None);
    }

    #[test]
    fn hex_to_bytes_rejects_embedded_nul() {
        assert_eq!(hex_to_bytes("a\0bc", 2), None);
    }

    #[test]
    fn n_times_root_k_rounds_to_nearest() {
        // 10 * sqrt(3) = 17.32..., 7 * sqrt(2) = 9.899..., 1 * sqrt(5) = 2.236...
        assert_eq!(
            [
                n_times_root_k(10, 3),
                n_times_root_k(7, 2),
                n_times_root_k(1, 5)
            ],
            [17, 10, 2]
        );
    }

    #[test]
    fn n_times_root_k_is_odd_in_n() {
        assert!((-500..=500).all(|n| n_times_root_k(-n, 3) == -n_times_root_k(n, 3)));
    }

    #[test]
    fn n_times_root_k_is_exact_for_perfect_squares() {
        assert!((0..1000).all(|n| n_times_root_k(n, 4) == 2 * n));
    }

    #[test]
    fn n_times_root_k_matches_float_away_from_halves() {
        for k in [2, 3, 5, 6, 7] {
            for n in 0..2000 {
                let exact = f64::from(n) * f64::from(k).sqrt();
                // Skip values within float error of a .5 boundary.
                if (exact.fract() - 0.5).abs() > 1e-9 {
                    assert_eq!(n_times_root_k(n, k), exact.round() as i32, "n={n} k={k}");
                }
            }
        }
    }

    #[test]
    fn env_bool_values_follow_c() {
        let cases: [(&str, bool); 7] = [
            ("yes", true),
            ("Y", true),
            ("true", true),
            ("T", true),
            ("no", false),
            ("1", false),
            ("", true),
        ];
        for (value, expected) in cases {
            assert_eq!(
                parse_env_bool(OsStr::new(value)),
                expected,
                "value {value:?}"
            );
        }
    }

    #[test]
    fn env_bool_of_unset_variable_is_none() {
        assert_eq!(env_bool("PUZZLES_RS_TEST_SURELY_UNSET_VARIABLE"), None);
    }

    #[test]
    fn move_cursor_steps_clamps_and_wraps() {
        use crate::button::{Modifiers, MouseAction, MouseButton};
        let up = Button::cursor(CursorKey::Up);
        let right = Button::cursor(CursorKey::Right);
        let (mut x, mut y) = (0, 0);
        assert_eq!(
            move_cursor::<()>(up, &mut x, &mut y, 3, 2, false, None),
            MoveResult::NoEffect
        );
        assert_eq!((x, y), (0, 0));
        assert_eq!(
            move_cursor::<()>(up, &mut x, &mut y, 3, 2, true, None),
            MoveResult::UiUpdate
        );
        assert_eq!((x, y), (0, 1));
        assert_eq!(
            move_cursor::<()>(right, &mut x, &mut y, 3, 2, false, None),
            MoveResult::UiUpdate
        );
        assert_eq!((x, y), (1, 1));

        let mut visible = false;
        let left = Button::cursor(CursorKey::Left);
        let (mut x, mut y) = (0, 0);
        let r = move_cursor::<()>(left, &mut x, &mut y, 3, 2, false, Some(&mut visible));
        assert_eq!((r, visible, x), (MoveResult::UiUpdate, true, 0));
        let r = move_cursor::<()>(left, &mut x, &mut y, 3, 2, false, Some(&mut visible));
        assert_eq!(r, MoveResult::NoEffect);

        // A hidden cursor that moves is also made visible.
        let mut visible = false;
        let r = move_cursor::<()>(right, &mut x, &mut y, 3, 2, false, Some(&mut visible));
        assert_eq!((r, visible, x), (MoveResult::UiUpdate, true, 1));

        for other in [
            Button::key('a'),
            Button::cursor(CursorKey::Select),
            Button::mouse(MouseButton::Left, MouseAction::Press),
            up.with(Modifiers::SHIFT),
        ] {
            let r = move_cursor::<()>(other, &mut x, &mut y, 3, 2, false, None);
            assert_eq!(r, MoveResult::Unused, "{other:?}");
        }
    }
}
