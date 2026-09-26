//! C `printf` number formatting, byte for byte.
//!
//! Rust's `{:.N}` is the exact, round-half-even conversion both C libraries
//! perform for `%.Nf`, which a differential test confirms; what differs is the
//! spelling of non-finite values and the existence of `%g`, which are handled
//! here. Padding is by bytes, as `printf` pads, never by characters.

#![forbid(unsafe_code)]

fn non_finite(value: f64) -> Option<&'static str> {
    if value.is_nan() {
        Some(if value.is_sign_negative() { "-nan" } else { "nan" })
    } else if value.is_infinite() {
        Some(if value < 0.0 { "-inf" } else { "inf" })
    } else {
        None
    }
}

/// `%.*f`.
pub fn fixed(value: f64, precision: usize) -> String {
    match non_finite(value) {
        Some(text) => text.to_owned(),
        None => format!("{value:.precision$}"),
    }
}

/// `%*.*f`: right aligned in `width` bytes.
pub fn fixed_width(value: f64, width: usize, precision: usize) -> String {
    pad_left(&fixed(value, precision), width)
}

/// `%*s`: right aligned in `width` bytes.
pub fn pad_left(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(width.max(text.len()));
    for _ in text.len()..width {
        out.push(' ');
    }
    out.push_str(text);
    out
}

/// `%-*s`: left aligned in `width` bytes.
pub fn pad_right(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(width.max(text.len()));
    out.push_str(text);
    for _ in text.len()..width {
        out.push(' ');
    }
    out
}

/// `%.*g`: the shorter of `%e` and `%f` at `precision` significant digits,
/// trailing zeros removed, as C99 7.19.6.1 specifies.
pub fn general(value: f64, precision: usize) -> String {
    if let Some(text) = non_finite(value) {
        return text.to_owned();
    }
    let p = if precision == 0 { 1 } else { precision };
    // The exponent X that %e would print at precision P - 1, after rounding.
    let scientific = format!("{:.*e}", p - 1, value);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific.as_str(), "0"));
    let x: i32 = exponent.parse().unwrap_or(0);
    if (x as i64) < p as i64 && x >= -4 {
        let digits = (p as i64 - 1 - x as i64) as usize;
        strip_fraction_zeros(fixed(value, digits))
    } else {
        let mantissa = strip_fraction_zeros(mantissa.to_owned());
        let sign = if x < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", x.unsigned_abs())
    }
}

fn strip_fraction_zeros(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_matches_the_c_rules() {
        assert_eq!(general(25.0, 4), "25");
        assert_eq!(general(100.0 / 3.0, 4), "33.33");
        assert_eq!(general(100.0 / 17.0, 4), "5.882");
        assert_eq!(general(123456.0, 4), "1.235e+05");
        assert_eq!(general(0.0001234, 4), "0.0001234");
        assert_eq!(general(0.00001234, 4), "1.234e-05");
        assert_eq!(general(9999.5, 4), "1e+04");
        assert_eq!(general(0.0, 4), "0");
        assert_eq!(general(-0.0, 4), "-0");
        assert_eq!(general(f64::INFINITY, 4), "inf");
    }

    #[test]
    fn padding_counts_bytes() {
        assert_eq!(pad_left("1\u{b0}", 5), "  1\u{b0}");
        assert_eq!(pad_right("ab", 4), "ab  ");
        assert_eq!(fixed_width(2.125, 5, 1), "  2.1");
    }
}
