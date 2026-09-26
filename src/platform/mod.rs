//! The operating-system boundary: the only module permitted `unsafe`.
//!
//! Every declaration here is an unchecked contract with the C library of a
//! supported target (docs/DESIGN.md §6). Types and constants are written out
//! per OS and architecture rather than assumed to be the same Unix, and are
//! checked against the native headers by the ABI probe in `tools/oracle/`.
//! Safe wrappers return `std::io::Error` carrying the raw `errno`, so callers
//! can print the C library's own message for them.

#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;
use std::os::fd::RawFd;

#[cfg(not(any(
    all(target_os = "macos", any(target_arch = "aarch64", target_arch = "x86_64")),
    all(
        target_os = "linux",
        target_env = "gnu",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
)))]
compile_error!(
    "rbirds supports aarch64/x86_64 macOS and aarch64/x86_64 GNU/Linux only; \
     other targets need their own verified bindings (docs/DESIGN.md §2)"
);

mod scan;
pub use scan::scan_osc_rgb;

pub const STDIN_FILENO: RawFd = 0;
pub const STDOUT_FILENO: RawFd = 1;
pub const STDERR_FILENO: RawFd = 2;

pub const EINTR: c_int = 4;
pub const EIO: c_int = 5;
pub const EPIPE: c_int = 32;
pub const ERANGE: c_int = 34;
#[cfg(target_os = "macos")]
pub const EAGAIN: c_int = 35;
#[cfg(target_os = "linux")]
pub const EAGAIN: c_int = 11;
/// The same value as [`EAGAIN`] on every supported target.
pub const EWOULDBLOCK: c_int = EAGAIN;

pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;
#[cfg(target_os = "macos")]
pub const O_NONBLOCK: c_int = 0x0004;
#[cfg(target_os = "linux")]
pub const O_NONBLOCK: c_int = 0o4000;

mod sys {
    use std::ffi::{c_char, c_int, c_void};

    unsafe extern "C" {
        pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        pub fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        pub fn strtod(nptr: *const c_char, endptr: *mut *mut c_char) -> f64;
        #[cfg(target_os = "macos")]
        pub fn __error() -> *mut c_int;
        #[cfg(target_os = "linux")]
        pub fn __errno_location() -> *mut c_int;
    }
}

fn errno_location() -> *mut c_int {
    // SAFETY: both functions take no arguments and return the calling
    // thread's errno slot, which lives as long as the thread.
    unsafe {
        #[cfg(target_os = "macos")]
        let location = sys::__error();
        #[cfg(target_os = "linux")]
        let location = sys::__errno_location();
        location
    }
}

/// The calling thread's `errno`.
pub fn errno() -> c_int {
    // SAFETY: the pointer is the thread's own errno slot, valid for reads.
    unsafe { *errno_location() }
}

/// Sets the calling thread's `errno`.
pub fn set_errno(value: c_int) {
    // SAFETY: the pointer is the thread's own errno slot, valid for writes.
    unsafe { *errno_location() = value }
}

/// What the C library's `strtod` made of a string.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strtod {
    pub value: f64,
    /// Bytes consumed; zero when no number was recognized (`end == text`).
    pub consumed: usize,
    /// Whether `errno` was `ERANGE` afterwards, `errno` having been cleared.
    pub erange: bool,
}

/// The C library's own `strtod`, so the grammar a numeric option accepts —
/// whitespace, sign, hexadecimal, exponents, `inf`, `nan` — is exactly the
/// reference's. The process never calls `setlocale`, so as in the C program
/// this is the "C" locale.
pub fn strtod(text: &CStr) -> Strtod {
    let mut end: *mut c_char = std::ptr::null_mut();
    set_errno(0);
    // SAFETY: `text` is NUL terminated and outlives the call; strtod reads up
    // to the terminator and stores into `end` a pointer into that same string.
    let value = unsafe { sys::strtod(text.as_ptr(), &mut end) };
    let erange = errno() == ERANGE;
    // SAFETY: strtod sets `end` to `text` itself or to a position within it,
    // so both pointers are derived from the same allocation.
    let consumed = unsafe { end.cast_const().offset_from(text.as_ptr()) };
    Strtod { value, consumed: consumed as usize, erange }
}

/// One `write(2)`: may be short, and reports `EINTR`/`EAGAIN` as errors for
/// the caller to handle exactly as the C does.
pub fn write(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    // SAFETY: `buf` is a live slice, so the pointer is valid for `buf.len()`
    // readable bytes for the duration of the call; write(2) does not retain it.
    let written = unsafe { sys::write(fd, buf.as_ptr().cast::<c_void>(), buf.len()) };
    if written < 0 { Err(io::Error::last_os_error()) } else { Ok(written as usize) }
}

/// One `read(2)` into `buf`; zero is end of file.
pub fn read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: `buf` is a live, exclusively borrowed slice, valid for
    // `buf.len()` writable bytes; read(2) writes at most that many.
    let got = unsafe { sys::read(fd, buf.as_mut_ptr().cast::<c_void>(), buf.len()) };
    if got < 0 { Err(io::Error::last_os_error()) } else { Ok(got as usize) }
}

/// `fcntl(fd, F_GETFL)`.
pub fn status_flags(fd: RawFd) -> io::Result<c_int> {
    // SAFETY: F_GETFL takes no third argument and only reads descriptor state.
    let flags = unsafe { sys::fcntl(fd, F_GETFL) };
    if flags < 0 { Err(io::Error::last_os_error()) } else { Ok(flags) }
}

/// `fcntl(fd, F_SETFL, flags)`.
pub fn set_status_flags(fd: RawFd, flags: c_int) -> io::Result<()> {
    // SAFETY: F_SETFL takes one `int` argument, passed with the variadic
    // calling convention the declaration above selects.
    let result = unsafe { sys::fcntl(fd, F_SETFL, flags) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
