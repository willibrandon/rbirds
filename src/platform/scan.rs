//! The C library's `sscanf`, for the one place the reference parses with it:
//! the channels of an OSC colour reply (`parse_osc_colour` in cbirds boids.c).
//! `%x` skips white space, takes a sign and a `0x` prefix within its width,
//! and wraps a negative value as `strtoul` does; calling the C library itself
//! is what makes a malformed terminal reply read exactly as the reference
//! reads it.

use std::ffi::{CString, c_char, c_int, c_uint};

#[cfg_attr(windows, link(name = "legacy_stdio_definitions"))]
unsafe extern "C" {
    fn sscanf(text: *const c_char, format: *const c_char, ...) -> c_int;
}

/// `sscanf(text, "%4x/%4x/%4x", &r, &g, &b) == 3`, over the bytes of a C
/// string (the caller has already cut it at its first NUL).
pub fn scan_osc_rgb(text: &[u8]) -> Option<(u32, u32, u32)> {
    let text = CString::new(text).ok()?;
    let (mut r, mut g, mut b): (c_uint, c_uint, c_uint) = (0, 0, 0);
    // SAFETY: both strings are NUL terminated and live across the call; the
    // format has exactly three `%x` conversions, each given a valid pointer
    // to a distinct `unsigned int`, passed with the variadic convention.
    let matched = unsafe {
        sscanf(
            text.as_ptr(),
            c"%4x/%4x/%4x".as_ptr(),
            &mut r as *mut c_uint,
            &mut g as *mut c_uint,
            &mut b as *mut c_uint,
        )
    };
    (matched == 3).then_some((r, g, b))
}
