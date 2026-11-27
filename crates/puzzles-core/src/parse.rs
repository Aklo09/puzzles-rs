//! C's lenient number parsing, as games use it on game IDs and move
//! strings (`atoi`, `sscanf`'s `%d`, skipping digits with `isdigit`), and
//! the float conversions of save files (`atof`, `printf`'s `%g`).
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

/// C's `atof` (`strtod` without the end pointer): the longest prefix
/// that reads as a decimal number, infinity or NaN, after leading spaces;
/// 0 if there is none.
///
/// Hexadecimal floats (`0x1p3`), which C also reads, give 0 here: no
/// writer of these files produces them.
///
/// # Panics
/// Never: the prefix handed to Rust's parser is ASCII by construction.
#[must_use]
pub fn atof(s: &[u8]) -> f64 {
    let start = s.iter().position(|&b| !is_c_space(b)).unwrap_or(s.len());
    let s = &s[start..];
    let sign_len = usize::from(matches!(s.first(), Some(b'+' | b'-')));
    let negative = s.first() == Some(&b'-');
    let body = &s[sign_len..];
    let lower: Vec<u8> = body.iter().take(8).map(u8::to_ascii_lowercase).collect();
    let special = if lower.starts_with(b"inf") {
        Some(f64::INFINITY)
    } else if lower.starts_with(b"nan") {
        Some(f64::NAN)
    } else {
        None
    };
    if let Some(v) = special {
        return if negative { -v } else { v };
    }

    let digits = |s: &[u8]| s.iter().take_while(|b| b.is_ascii_digit()).count();
    let int = digits(body);
    let mut len = int;
    let mut frac = 0;
    if body.get(len) == Some(&b'.') {
        frac = digits(&body[len + 1..]);
        if int + frac > 0 {
            len += 1 + frac;
        }
    }
    if int + frac == 0 {
        return 0.0;
    }
    if matches!(body.get(len), Some(b'e' | b'E')) {
        let exp_sign = usize::from(matches!(body.get(len + 1), Some(b'+' | b'-')));
        let exp_digits = digits(&body[(len + 1 + exp_sign).min(body.len())..]);
        if exp_digits > 0 {
            len += 1 + exp_sign + exp_digits;
        }
    }
    let text = std::str::from_utf8(&s[..sign_len + len]).expect("ASCII digits");
    text.parse().unwrap_or(0.0)
}

/// C's `printf("%g", x)`: six significant digits, in fixed or exponent
/// notation (whichever C picks), without trailing zeros.
///
/// A NaN is `nan` whatever its sign, as on macOS (glibc writes a negative
/// one as `-nan`); only a corrupt save file's `TIME` line can hold one.
///
/// # Panics
/// Never: Rust's `{:e}` output always has an integer exponent.
#[must_use]
pub fn format_g(x: f64) -> String {
    const PRECISION: i32 = 6;
    if x.is_nan() {
        return "nan".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    // The exponent C's %e would print, after rounding to the precision.
    let sci = format!("{:.*e}", (PRECISION - 1) as usize, x);
    let (mantissa, exp) = sci.split_once('e').expect("Rust's {:e} has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s.to_owned()
        }
    };
    if (-4..PRECISION).contains(&exp) {
        strip(&format!("{:.*}", (PRECISION - 1 - exp) as usize, x))
    } else {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", strip(mantissa), exp.abs())
    }
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
    fn atof_reads_the_longest_number() {
        assert_eq!(atof(b"12.5"), 12.5);
        assert_eq!(atof(b"  -3e2xyz"), -300.0);
        assert_eq!(atof(b"1e"), 1.0);
        assert_eq!(atof(b"1e+"), 1.0);
        assert_eq!(atof(b".5"), 0.5);
        assert_eq!(atof(b"7."), 7.0);
        assert_eq!(atof(b"."), 0.0);
        assert_eq!(atof(b"x"), 0.0);
        assert_eq!(atof(b"-"), 0.0);
        assert_eq!(atof(b"INFinity"), f64::INFINITY);
        assert!(atof(b"nan").is_nan());
        assert_eq!(atof(b"0x10"), 0.0);
    }

    #[test]
    fn format_g_matches_printf() {
        for (x, s) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (12.5, "12.5"),
            (0.1, "0.1"),
            (123_456.0, "123456"),
            (1_234_567.0, "1.23457e+06"),
            (999_999.5, "1e+06"),
            (0.0001, "0.0001"),
            (0.000_012_345, "1.2345e-05"),
            (1e100, "1e+100"),
            (f64::from(3.3_f32), "3.3"),
            (f64::INFINITY, "inf"),
        ] {
            assert_eq!(format_g(x), s, "{x}");
        }
    }

    #[test]
    fn skips_digits_only() {
        assert_eq!(skip_digits(b"123x4"), b"x4");
        assert_eq!(skip_digits(b"-1"), b"-1");
        assert_eq!(skip_digits(b""), b"");
    }
}
