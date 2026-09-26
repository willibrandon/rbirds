//! `poll(2)`.

use super::os::nfds_t;
use super::sys;
use std::ffi::{c_int, c_short};
use std::io;

/// `struct pollfd`: `{int fd; short events; short revents;}` on every
/// supported target.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollFd {
    pub fd: c_int,
    /// Requested events: [`POLLIN`](super::POLLIN), [`POLLOUT`](super::POLLOUT).
    pub events: c_short,
    /// Returned events, which may add [`POLLERR`](super::POLLERR),
    /// [`POLLHUP`](super::POLLHUP) and [`POLLNVAL`](super::POLLNVAL).
    pub revents: c_short,
}

impl PollFd {
    pub fn new(fd: c_int, events: c_short) -> PollFd {
        PollFd { fd, events, revents: 0 }
    }
}

/// `poll(fds, len, timeout_ms)`: the number of ready descriptors, zero on
/// timeout. A negative timeout waits indefinitely. `EINTR` is returned as an
/// error, for the caller to retry as the C does.
pub fn poll(fds: &mut [PollFd], timeout_ms: c_int) -> io::Result<c_int> {
    let count =
        nfds_t::try_from(fds.len()).map_err(|_| io::Error::from_raw_os_error(super::os::EINVAL))?;
    // SAFETY: `fds` is a live, exclusively borrowed slice of `count` `PollFd`s
    // with the target's `struct pollfd` layout (ABI probe); poll writes only
    // their `revents` and keeps no pointer after returning.
    let ready = unsafe { sys::poll(fds.as_mut_ptr(), count, timeout_ms) };
    if ready < 0 { Err(io::Error::last_os_error()) } else { Ok(ready) }
}
