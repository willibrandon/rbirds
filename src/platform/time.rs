//! Clocks and sleeping: `clock_gettime`, `nanosleep`, `time`.

use super::os::{CLOCK_MONOTONIC, clockid_t, time_t};
use super::sys;
use std::ffi::c_long;
use std::io;
use std::time::Duration;

/// A blocking frame timer. Darwin's ordinary sleeps can coalesce by several
/// milliseconds even when rendering finishes early. A critical kqueue timer
/// narrows that leeway without spinning or raising the thread's priority.
#[derive(Default)]
pub struct FrameSleeper {
    #[cfg(target_os = "macos")]
    timer: Option<std::os::fd::OwnedFd>,
}

impl FrameSleeper {
    pub fn new() -> Self {
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::FromRawFd;
            // SAFETY: kqueue takes no arguments and returns a fresh descriptor.
            let fd = unsafe { sys::kqueue() };
            let timer = if fd < 0 {
                None
            } else {
                // SAFETY: ownership of this fresh descriptor transfers here.
                Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
            };
            Self { timer }
        }
        #[cfg(not(target_os = "macos"))]
        Self::default()
    }

    pub fn sleep(&mut self, delay: Duration) {
        if delay.is_zero() {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(timer) = &self.timer {
            use super::os::*;
            use std::os::fd::AsRawFd;
            let request = Kevent64 {
                ident: 1,
                filter: EVFILT_TIMER,
                flags: EV_ADD | EV_ONESHOT,
                fflags: NOTE_NSECONDS | NOTE_CRITICAL,
                data: delay.as_nanos().min(i64::MAX as u128) as i64,
                ..Kevent64::default()
            };
            let mut event = Kevent64::default();
            // SAFETY: live kqueue, one correctly laid out event in/out, and
            // null timeout to block until the one-shot timer or a signal.
            let result = unsafe {
                sys::kevent64(timer.as_raw_fd(), &request, 1, &mut event, 1, 0, std::ptr::null())
            };
            if result == 1 && event.flags & EV_ERROR == 0
                || result < 0 && io::Error::last_os_error().raw_os_error() == Some(super::EINTR)
            {
                return;
            }
            // A resource or API failure falls back to the ordinary sleep.
            self.timer = None;
        }
        let _ = nanosleep(&Timespec {
            tv_sec: delay.as_secs() as i64,
            tv_nsec: i64::from(delay.subsec_nanos()),
        });
    }
}

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

/// A named, system-wide timebase for joining traces from separate processes.
pub const MEASUREMENT_CLOCK: &str = "clock_gettime(CLOCK_MONOTONIC)";

pub fn measurement_clock_ns() -> io::Result<u64> {
    let now = clock_gettime(CLOCK_MONOTONIC)?;
    let nanos = i128::from(now.tv_sec) * 1_000_000_000 + i128::from(now.tv_nsec);
    u64::try_from(nanos).map_err(io::Error::other)
}

/// User and kernel CPU time across all threads, independent of wall-clock waits.
pub fn process_cpu_time() -> io::Result<std::time::Duration> {
    let time = clock_gettime(super::os::CLOCK_PROCESS_CPUTIME_ID)?;
    Ok(std::time::Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
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
