//! The operating-system boundary: the only module permitted `unsafe`.
//!
//! Every declaration here is an unchecked contract with the C library of a
//! supported target (docs/DESIGN.md §6). Types and constants are written out
//! per OS and architecture rather than assumed to be the same Unix, and are
//! checked against the native headers by the ABI probe in `tools/oracle/`.
//! Safe wrappers return `std::io::Error` carrying the raw `errno`, so callers
//! can print the C library's own message for them.
//!
//! Layout of the boundary:
//!
//! - `darwin.rs` / `linux.rs`: the target's C types, structures and constants.
//! - `sys` (below): every foreign function, declared once, each paired with the
//!   C prototype it assumes; [`abi::rust_layout_report`] prints both halves for
//!   comparison with `tools/oracle/abi_probe.c`.
//! - `termios.rs`, `tty.rs`, `poll.rs`, `time.rs`, `errors.rs`: safe wrappers.
//! - `restore.rs`, `signals.rs`: the terminal-ownership flags, boids.c's
//!   `restore_terminal`, and its signal handler (the async-signal-safe path).
//! - [`pty`]: pseudoterminals, as test support only.

#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;
pub use std::os::fd::RawFd;
pub use std::os::unix::ffi::OsStrExt;

pub fn os_string_from_bytes(bytes: &[u8]) -> std::ffi::OsString {
    std::ffi::OsStr::from_bytes(bytes).to_os_string()
}

pub fn exit_requested() -> bool {
    false
}

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

#[cfg(target_os = "macos")]
mod darwin;
#[cfg(target_os = "macos")]
use darwin as os;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

pub mod abi;
mod errors;
mod file;
mod poll;
pub mod pty;
mod restore;
mod scan;
mod signals;
mod termios;
mod time;
mod trig;
mod tty;

#[cfg(test)]
mod tests;

pub use errors::{exit_immediately, perror_message, strerror};
pub use file::write_file;
#[cfg(target_os = "linux")]
pub use os::TIOCGPTN;
pub use os::{
    _POSIX_VDISABLE, BRKINT, CLOCK_MONOTONIC, CS8, CSIZE, EBADF, ECHO, EINVAL, ENOTTY, F_GETFD,
    F_SETFD, FD_CLOEXEC, ICANON, ICRNL, IEXTEN, INPCK, ISIG, ISTRIP, IXON, NCCS, O_CLOEXEC,
    O_NOCTTY, O_RDWR, OPOST, POLLERR, POLLHUP, POLLIN, POLLNVAL, POLLOUT, SA_RESETHAND, SIG_DFL,
    SIG_IGN, SIGABRT, SIGBUS, SIGFPE, SIGHUP, SIGINT, SIGKILL, SIGPIPE, SIGQUIT, SIGSEGV, SIGTERM,
    TCSAFLUSH, TCSANOW, TIOCGWINSZ, TIOCSWINSZ, Termios, VMIN, VSUSP, VTIME, cc_t, clockid_t,
    nfds_t, pid_t, speed_t, tcflag_t, time_t,
};
pub use poll::{PollFd, poll};
pub use restore::{
    ALT_SCREEN_OFF, ALT_SCREEN_ON, CURSOR_HIDE, CURSOR_SHOW, KITTY_FREE_IMAGES, MOUSE_REPORT_OFF,
    MOUSE_REPORT_ON, SYNC_UPDATE_END, alt_screen_is_on, enable_sixel_mode, enter_alt_screen,
    enter_terminal, is_restored, mark_alt_screen_on, mark_raw_acquired, mark_sprites_uploaded,
    reset_terminal_state_for_tests, restore_terminal, sprites_uploaded, terminal_is_raw,
    write_all_quietly,
};
pub use scan::scan_osc_rgb;
pub use signals::{default_sigpipe, install_signal_handlers, send_signal};
pub use termios::{RawModeView, tcgetattr, tcsetattr};
pub use time::{Timespec, clock_gettime, monotonic_now, nanosleep, time_now};
pub use trig::sin_cos;
pub use tty::{WinSize, is_terminal, set_window_size, window_size, window_size_or_zero};

pub const STDIN_FILENO: RawFd = 0;
pub const STDOUT_FILENO: RawFd = 1;
pub const STDERR_FILENO: RawFd = 2;

pub const EINTR: c_int = 4;
pub const EIO: c_int = 5;
pub const EPIPE: c_int = 32;
pub const ERANGE: c_int = 34;
pub const ENOMEM: c_int = 12;
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

/// Declares foreign functions together with the C prototype each assumes.
///
/// The prototype text is what `tools/oracle/abi_probe.c` prints for a
/// function only when the native header declares it with exactly that type
/// (via `_Generic`), so the ABI test fails if a header's prototype differs
/// from the one the Rust declaration beside it was written against. Rust
/// parameter types are the target aliases from `darwin.rs`/`linux.rs`, whose
/// widths and signedness the same report checks.
macro_rules! foreign {
    ($(
        [$c_name:literal, $c_type:literal]
        $(#[cfg($cfg:meta)])?
        $(#[link_name = $link:literal])?
        fn $name:ident($($args:tt)*) $(-> $ret:ty)?;
    )*) => {
        unsafe extern "C" {
            $(
                $(#[cfg($cfg)])?
                $(#[link_name = $link])?
                pub fn $name($($args)*) $(-> $ret)?;
            )*
        }

        /// `(C name, assumed C prototype)` for every declared function, in
        /// declaration order (the order the probe prints them).
        // Pushed one by one so each entry can carry its declaration's cfg.
        #[allow(clippy::vec_init_then_push)]
        pub fn signatures() -> Vec<(&'static str, &'static str)> {
            let mut list = Vec::new();
            $(
                $(#[cfg($cfg)])?
                list.push(($c_name, $c_type));
            )*
            list
        }
    };
}

/// Every foreign function the platform layer calls.
///
/// Symbol names: on 64-bit Darwin the `__DARWIN_ALIAS` suffixes are empty, so
/// each function links under its plain name; on glibc the XSI `strerror_r`
/// is exported as `__xpg_strerror_r` (plain `strerror_r` is the GNU variant
/// returning `char *`).
mod sys {
    use super::os::{SigAction, Termios, clockid_t, nfds_t, pid_t, sigset_t, time_t};
    use super::poll::PollFd;
    use super::time::Timespec;
    use std::ffi::{c_char, c_int, c_ulong, c_void};

    foreign! {
        ["read", "ssize_t(int, void *, size_t)"]
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        ["write", "ssize_t(int, const void *, size_t)"]
        fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        ["fcntl", "int(int, int, ...)"]
        fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
        ["strtod", "double(const char *, char **)"]
        fn strtod(nptr: *const c_char, endptr: *mut *mut c_char) -> f64;
        ["tcgetattr", "int(int, struct termios *)"]
        fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
        ["tcsetattr", "int(int, int, const struct termios *)"]
        fn tcsetattr(fd: c_int, action: c_int, termios: *const Termios) -> c_int;
        ["ioctl", "int(int, unsigned long, ...)"]
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
        ["poll", "int(struct pollfd *, nfds_t, int)"]
        fn poll(fds: *mut PollFd, nfds: nfds_t, timeout: c_int) -> c_int;
        ["clock_gettime", "int(clockid_t, struct timespec *)"]
        fn clock_gettime(clock: clockid_t, now: *mut Timespec) -> c_int;
        ["nanosleep", "int(const struct timespec *, struct timespec *)"]
        fn nanosleep(delay: *const Timespec, remaining: *mut Timespec) -> c_int;
        ["time", "time_t(time_t *)"]
        fn time(out: *mut time_t) -> time_t;
        ["strerror_r", "int(int, char *, size_t)"]
        #[cfg(target_os = "macos")]
        fn strerror_r(errnum: c_int, buf: *mut c_char, len: usize) -> c_int;
        ["strerror_r", "int(int, char *, size_t)"]
        #[cfg(target_os = "linux")]
        #[link_name = "__xpg_strerror_r"]
        fn strerror_r(errnum: c_int, buf: *mut c_char, len: usize) -> c_int;
        ["isatty", "int(int)"]
        fn isatty(fd: c_int) -> c_int;
        ["_exit", "void(int)"]
        fn _exit(code: c_int) -> !;
        ["sigaction", "int(int, const struct sigaction *, struct sigaction *)"]
        fn sigaction(signal: c_int, action: *const SigAction, old: *mut SigAction) -> c_int;
        ["sigemptyset", "int(sigset_t *)"]
        fn sigemptyset(set: *mut sigset_t) -> c_int;
        ["kill", "int(pid_t, int)"]
        fn kill(pid: pid_t, signal: c_int) -> c_int;
        ["posix_openpt", "int(int)"]
        fn posix_openpt(flags: c_int) -> c_int;
        ["grantpt", "int(int)"]
        fn grantpt(fd: c_int) -> c_int;
        ["unlockpt", "int(int)"]
        fn unlockpt(fd: c_int) -> c_int;
        ["ptsname_r", "int(int, char *, size_t)"]
        #[cfg(target_os = "macos")]
        fn ptsname_r(fd: c_int, buf: *mut c_char, len: usize) -> c_int;
        ["__error", "int *(void)"]
        #[cfg(target_os = "macos")]
        fn __error() -> *mut c_int;
        ["__errno_location", "int *(void)"]
        #[cfg(target_os = "linux")]
        fn __errno_location() -> *mut c_int;
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

/// The calling thread's `errno`. Async-signal-safe.
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

/// The C library's own `strtod`, so a numeric option accepts the same syntax
/// as in the reference: whitespace, sign, hexadecimal, exponents, `inf` and
/// `nan`. The process never calls `setlocale`, so as in the C program
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

/// `fcntl(fd, F_GETFD)`: the descriptor flags (`FD_CLOEXEC`).
pub fn descriptor_flags(fd: RawFd) -> io::Result<c_int> {
    // SAFETY: F_GETFD takes no third argument and only reads descriptor state.
    let flags = unsafe { sys::fcntl(fd, F_GETFD) };
    if flags < 0 { Err(io::Error::last_os_error()) } else { Ok(flags) }
}

/// `fcntl(fd, F_SETFD, flags)`.
pub fn set_descriptor_flags(fd: RawFd, flags: c_int) -> io::Result<()> {
    // SAFETY: F_SETFD takes one `int` argument, passed with the variadic
    // calling convention the declaration above selects.
    let result = unsafe { sys::fcntl(fd, F_SETFD, flags) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Sets or clears `O_NONBLOCK` in the status flags of `fd`'s open file
/// description (shared by every descriptor duplicated from it), keeping the
/// other flags.
pub fn set_nonblocking(fd: RawFd, nonblocking: bool) -> io::Result<()> {
    let flags = status_flags(fd)?;
    let wanted = if nonblocking { flags | O_NONBLOCK } else { flags & !O_NONBLOCK };
    if wanted == flags { Ok(()) } else { set_status_flags(fd, wanted) }
}
