//! C's lenient number parsing, as games use it on game IDs and move
//! strings: `atoi`, `sscanf`'s `%d`, and skipping digits with `isdigit`.
//!
//! The functions work on bytes, as C does, so a multi-byte UTF-8
//! character is never a digit or a space and never splits.
//!
//! Out-of-range numbers are undefined behaviour in C. Real C libraries
//! read into a `long` (saturating at its range) and convert to `int`
//! by truncation. These functions do the same with a 64-bit `long`, as
//! on Linux and macOS; on Windows `long` has 32 bits, so C there
//! saturates at `i32` instead. The two agree on every number that fits
//! in an `i32`.

/// C's `isspace` in the "C" locale.
const fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Reads a number like `strtol(s, &end, 10)`: leading spaces, an
/// optional sign, then digits. Returns the value (truncated to `i32`,
/// see the module notes) and the rest of the input, or `None` if there
/// are no digits.
#[must_use]
pub fn scan_int(s: &[u8]) -> Option<(i32, &[u8])> {
    let start = s.iter().position(|&b| !is_c_space(b)).unwrap_or(s.len());
    let mut rest = &s[start..];
    let negative = match rest.first() {
        Some(b'-') => {
            rest = &rest[1..];
            true
        }
        Some(b'+') => {
            rest = &rest[1..];
            false
        }
        _ => false,
    };
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    // Accumulate the magnitude negatively so i64::MIN fits, saturating
    // as strtol does.
    let mut value: i64 = 0;
    for &d in &rest[..digits] {
        value = value.saturating_mul(10).saturating_sub(i64::from(d - b'0'));
    }
    if !negative {
        value = value.saturating_neg();
    }
    Some((value as i32, &rest[digits..]))
}

/// C's `atoi`: [`scan_int`]'s value, or 0 if there are no digits.
#[must_use]
pub fn atoi(s: &[u8]) -> i32 {
    scan_int(s).map_or(0, |(n, _)| n)
}

/// Skips leading ASCII digits (C's `while (*s && isdigit(*s)) s++`).
#[must_use]
pub fn skip_digits(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| b.is_ascii_digit()).count();
    &s[n..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_like_strtol() {
        assert_eq!(scan_int(b"12,3"), Some((12, &b",3"[..])));
        assert_eq!(scan_int(b"  -7x"), Some((-7, &b"x"[..])));
        assert_eq!(scan_int(b"\t+5"), Some((5, &b""[..])));
        assert_eq!(scan_int(b"-"), None);
        assert_eq!(scan_int(b"x1"), None);
        assert_eq!(scan_int(b""), None);
        assert_eq!(scan_int(b"- 1"), None);
    }

    #[test]
    fn large_numbers_truncate_a_64_bit_long() {
        assert_eq!(atoi(b"2147483647"), i32::MAX);
        assert_eq!(atoi(b"-2147483648"), i32::MIN);
        assert_eq!(atoi(b"4294967301"), 5);
        // Saturates at i64::MAX, whose low 32 bits are all ones.
        assert_eq!(atoi(b"99999999999999999999999"), -1);
        assert_eq!(atoi(b"-99999999999999999999999"), 0);
    }

    #[test]
    fn atoi_without_digits_is_zero() {
        assert_eq!(atoi(b"abc"), 0);
        assert_eq!(atoi(b"\xc3\xa9"), 0);
    }

    #[test]
    fn skips_digits_only() {
        assert_eq!(skip_digits(b"123x4"), b"x4");
        assert_eq!(skip_digits(b"-1"), b"-1");
        assert_eq!(skip_digits(b""), b"");
    }
}
