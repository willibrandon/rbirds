//! Pseudoterminals — **test support only; the application never uses this.**
//!
//! The PTY harness in tests/support/pty.rs runs the C reference and rbirds on
//! the slave side of one of these, playing the terminal on the master side.
//! It lives here only because opening a pseudoterminal needs `unsafe` calls,
//! which the crate confines to `platform`.
//!
//! The master is opened `O_RDWR | O_NOCTTY | O_CLOEXEC` (both C libraries
//! pass the flags to `open("/dev/ptmx")`), so it never becomes the test
//! process's controlling terminal and never leaks into a concurrently spawned
//! child. The slave is opened `O_NOCTTY` (and close-on-exec, by `std`); a
//! child gets it only through explicit `Stdio` redirection. No `setsid` is
//! done, so the slave is not the child's controlling terminal either: job
//! control signals (`SIGTTOU`, `SIGHUP` on hangup) never arise, and signals
//! reach the child only when the harness sends them.

#[cfg(target_os = "linux")]
use super::os::TIOCGPTN;
use super::os::{O_CLOEXEC, O_NOCTTY, O_RDWR};
use super::sys;
use super::tty::{WinSize, set_window_size};
use std::ffi::c_int;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// An open pseudoterminal pair. Dropping it closes both ends.
#[derive(Debug)]
pub struct Pty {
    /// The terminal's side: what the program writes is read here, and what is
    /// written here is the program's input.
    pub master: OwnedFd,
    /// The program's side, kept open by the harness so the terminal's
    /// attributes can be read before, during and after a run.
    pub slave: OwnedFd,
    /// The slave's device path, e.g. `/dev/ttys012` or `/dev/pts/3`.
    pub slave_path: PathBuf,
}

fn check(result: c_int) -> io::Result<c_int> {
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(result) }
}

/// Opens a pseudoterminal pair whose window size is `size`.
pub fn open_pty(size: &WinSize) -> io::Result<Pty> {
    // SAFETY: posix_openpt takes flags by value and returns a new descriptor
    // or -1; it touches no memory of ours.
    let raw = check(unsafe { sys::posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC) })?;
    // SAFETY: `raw` was just returned by posix_openpt, is open, and is owned
    // by nothing else, so the OwnedFd becomes its only owner.
    let master = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: grantpt and unlockpt take the master descriptor by value.
    check(unsafe { sys::grantpt(master.as_raw_fd()) })?;
    // SAFETY: as above.
    check(unsafe { sys::unlockpt(master.as_raw_fd()) })?;
    let slave_path = slave_name(&master)?;
    let slave =
        OpenOptions::new().read(true).write(true).custom_flags(O_NOCTTY).open(&slave_path)?;
    let pty = Pty { master, slave: OwnedFd::from(slave), slave_path };
    set_window_size(pty.master.as_raw_fd(), size)?;
    Ok(pty)
}

#[cfg(target_os = "macos")]
fn slave_name(master: &OwnedFd) -> io::Result<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = [0u8; 128];
    // SAFETY: `buffer` is live and writable for its full length, which is the
    // length passed; ptsname_r writes a NUL-terminated name within it.
    let result =
        unsafe { sys::ptsname_r(master.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let name = CStr::from_bytes_until_nul(&buffer)
        .map_err(|_| io::Error::other("ptsname_r returned an unterminated name"))?;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(name.to_bytes())))
}

#[cfg(target_os = "linux")]
fn slave_name(master: &OwnedFd) -> io::Result<PathBuf> {
    let mut number: std::ffi::c_uint = 0;
    // SAFETY: TIOCGPTN takes an `unsigned int *`; `number` is a live, writable
    // `c_uint`, and the kernel keeps no pointer.
    check(unsafe {
        sys::ioctl(master.as_raw_fd(), TIOCGPTN, &mut number as *mut std::ffi::c_uint)
    })?;
    Ok(PathBuf::from(format!("/dev/pts/{number}")))
}
