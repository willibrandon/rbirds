//! Clocks and sleeping: `clock_gettime`, `nanosleep`, `time`.

use super::os::{CLOCK_MONOTONIC, clockid_t, time_t};
use super::sys;
use std::ffi::c_long;
use std::io;

/// `struct timespec`: `{time_t tv_sec; long tv_nsec;}`, both 64-bit on every
/// supported target.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timespec {
    pub tv_sec: time_t,
    pub tv_nsec: c_long,
}

/// `clock_gettime(clock, &now)`.
pub fn clock_gettime(clock: clockid_t) -> io::Result<Timespec> {
    let mut now = Timespec::default();
    // SAFETY: `now` is a live, writable `Timespec` with the target's
    // `struct timespec` layout (ABI probe); the call writes it and returns.
    let result = unsafe { sys::clock_gettime(clock, &mut now) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(now) }
}

/// `clock_gettime(CLOCK_MONOTONIC, ...)`, as boids.c calls it everywhere,
/// ignoring the result. The call cannot fail for this clock and a valid
/// pointer on the supported targets; were it to, this returns zero rather
/// than the C's untouched (uninitialized or previous) structure.
pub fn monotonic_now() -> Timespec {
    clock_gettime(CLOCK_MONOTONIC).unwrap_or_default()
}

/// `nanosleep(delay, NULL)`. boids.c ignores both the result and the unslept
/// remainder, so an interrupted sleep simply ends early.
pub fn nanosleep(delay: &Timespec) -> io::Result<()> {
    // SAFETY: `delay` is a live `Timespec` only read during the call; a null
    // remainder pointer is permitted and means "do not report it".
    let result = unsafe { sys::nanosleep(delay, std::ptr::null_mut()) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// `time(NULL)`: seconds since the Unix epoch, the live run's default seed.
pub fn time_now() -> i64 {
    // SAFETY: a null argument is permitted and means "only return the value".
    unsafe { sys::time(std::ptr::null_mut()) }
}
