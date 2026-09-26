//! Signals: boids.c `signal_handler` and `install_signal_handlers`, and
//! `kill` for the test harness.

use super::errors::exit_immediately;
use super::os::{
    SA_RESETHAND, SIG_DFL, SIG_IGN, SIGABRT, SIGBUS, SIGFPE, SIGHUP, SIGINT, SIGPIPE, SIGQUIT,
    SIGSEGV, SIGTERM, SigAction, pid_t,
};
use super::restore::restore_terminal;
use super::sys;
use std::ffi::c_int;
use std::io;

/// The signals boids.c catches, in its order.
const CAUGHT: [c_int; 8] = [SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGSEGV, SIGFPE, SIGBUS, SIGABRT];

/// boids.c `signal_handler`: `restore_terminal(); _exit(128 + signal_number);`.
///
/// Only async-signal-safe work happens here (see `restore.rs`). Nothing on the
/// path can panic: its only indexing is by constants into fixed-size arrays,
/// and its arithmetic wraps. `_exit` never returns, so no unwind can reach the
/// C caller (a panic would abort at this `extern "C"` boundary anyway).
extern "C" fn signal_handler(signal_number: c_int) {
    restore_terminal();
    exit_immediately(signal_number.wrapping_add(128));
}

/// boids.c `install_signal_handlers`, exactly: a zeroed `struct sigaction`
/// with `signal_handler`, `sa_flags = (int)SA_RESETHAND` and an empty mask,
/// installed for SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGSEGV, SIGFPE, SIGBUS and
/// SIGABRT; then SIGPIPE set to `SIG_IGN` with `sa_flags = 0`, so a vanished
/// reader is a write error (`EPIPE`) rather than death. Results are ignored,
/// as in the C.
///
/// As in the C, the SIGSEGV and SIGBUS handlers replace any installed before
/// (Rust's stack-overflow reporter among them) and do not run on an
/// alternate stack.
pub fn install_signal_handlers() {
    let handler: extern "C" fn(c_int) = signal_handler;
    let mut action =
        SigAction { sa_handler: handler as usize, sa_flags: SA_RESETHAND, ..SigAction::default() };
    // SAFETY: `action.sa_mask` is a live, writable `sigset_t` of the target's
    // layout (ABI probe); sigemptyset only clears it.
    unsafe { sys::sigemptyset(&mut action.sa_mask) };
    for signal in CAUGHT {
        // SAFETY: `action` is a fully initialized `struct sigaction` whose
        // handler is an `extern "C" fn(c_int)` that lives for the whole
        // program; sigaction copies it and a null old-action pointer is
        // permitted.
        unsafe { sys::sigaction(signal, &action, std::ptr::null_mut()) };
    }
    action.sa_handler = SIG_IGN;
    action.sa_flags = 0;
    // SAFETY: as above, with the SIG_IGN disposition instead of a handler.
    unsafe { sys::sigaction(SIGPIPE, &action, std::ptr::null_mut()) };
}

/// `kill(pid, signal)`. Test support: the PTY harness signals the program
/// under test with it.
pub fn send_signal(pid: pid_t, signal: c_int) -> io::Result<()> {
    // SAFETY: kill takes two integers by value and touches no memory of ours.
    let result = unsafe { sys::kill(pid, signal) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// SIGPIPE back to its default action, as a C program starts. Rust's runtime
/// ignores SIGPIPE before `main`; cbirds does not, so outside the live run
/// (which installs `SIG_IGN` itself) a vanished reader kills the process
/// exactly as it kills the reference (`rbirds --help | true`, for example).
pub fn default_sigpipe() {
    let mut action = SigAction { sa_handler: SIG_DFL, sa_flags: 0, ..SigAction::default() };
    // SAFETY: `action.sa_mask` is a live, writable `sigset_t` of the target's
    // layout (ABI probe); sigemptyset only clears it.
    unsafe { sys::sigemptyset(&mut action.sa_mask) };
    // SAFETY: a fully initialized `struct sigaction` with the default
    // disposition; a null old-action pointer is permitted.
    unsafe { sys::sigaction(SIGPIPE, &action, std::ptr::null_mut()) };
}
