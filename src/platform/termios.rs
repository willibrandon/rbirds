//! Terminal attributes: `tcgetattr`, `tcsetattr`, and exactly the changes
//! boids.c `enter_terminal` makes to them.

use super::os::{
    _POSIX_VDISABLE, BRKINT, CS8, CSIZE, ECHO, ICANON, ICRNL, IEXTEN, INPCK, ISTRIP, IXON, OPOST,
    Termios, VMIN, VSUSP, VTIME, cc_t, tcflag_t,
};
use super::sys;
use std::ffi::c_int;
use std::io;
use std::os::fd::RawFd;

/// The input flags `enter_terminal` clears.
const RAW_CLEARED_IFLAG: tcflag_t = BRKINT | ICRNL | INPCK | ISTRIP | IXON;
/// The local flags `enter_terminal` clears.
const RAW_CLEARED_LFLAG: tcflag_t = ECHO | ICANON | IEXTEN;

/// `tcgetattr(fd, &termios)`.
pub fn tcgetattr(fd: RawFd) -> io::Result<Termios> {
    let mut termios = Termios::default();
    // SAFETY: `termios` is a live, exclusively borrowed `Termios`, whose layout
    // is the target's `struct termios` (checked by the ABI probe); tcgetattr
    // writes one such structure through the pointer and keeps no reference.
    let result = unsafe { sys::tcgetattr(fd, &mut termios) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(termios) }
}

/// `tcsetattr(fd, when, termios)`; `when` is [`TCSANOW`](super::TCSANOW) or
/// [`TCSAFLUSH`](super::TCSAFLUSH). Async-signal-safe.
pub fn tcsetattr(fd: RawFd, when: c_int, termios: &Termios) -> io::Result<()> {
    // SAFETY: `termios` is a live `Termios` with the target's layout; tcsetattr
    // only reads it during the call.
    let result = unsafe { sys::tcsetattr(fd, when, termios) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

impl Termios {
    /// These attributes with exactly boids.c `enter_terminal`'s changes:
    ///
    /// ```text
    /// c_iflag &= ~(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    /// c_oflag &= ~OPOST;
    /// c_cflag |= CS8;
    /// c_lflag &= ~(ECHO | ICANON | IEXTEN);
    /// c_cc[VSUSP] = _POSIX_VDISABLE;
    /// c_cc[VMIN] = 0;
    /// c_cc[VTIME] = 0;
    /// ```
    ///
    /// Everything else — `ISIG` included, so ^C still signals — is kept.
    pub fn raw_mode(&self) -> Termios {
        let mut raw = *self;
        raw.c_iflag &= !RAW_CLEARED_IFLAG;
        raw.c_oflag &= !OPOST;
        raw.c_cflag |= CS8;
        raw.c_lflag &= !RAW_CLEARED_LFLAG;
        raw.c_cc[VSUSP] = _POSIX_VDISABLE;
        raw.c_cc[VMIN] = 0;
        raw.c_cc[VTIME] = 0;
        raw
    }

    /// The parts of these attributes that `enter_terminal` changes, for
    /// focused comparisons and readable test failures. (`==` on [`Termios`]
    /// compares every named field and never structure padding.)
    pub fn raw_mode_view(&self) -> RawModeView {
        RawModeView {
            iflag: self.c_iflag & RAW_CLEARED_IFLAG,
            oflag: self.c_oflag & OPOST,
            csize: self.c_cflag & CSIZE,
            lflag: self.c_lflag & RAW_CLEARED_LFLAG,
            vsusp: self.c_cc[VSUSP],
            vmin: self.c_cc[VMIN],
            vtime: self.c_cc[VTIME],
        }
    }
}

/// The attribute bits and control characters `enter_terminal` touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawModeView {
    /// `c_iflag & (BRKINT | ICRNL | INPCK | ISTRIP | IXON)`.
    pub iflag: tcflag_t,
    /// `c_oflag & OPOST`.
    pub oflag: tcflag_t,
    /// `c_cflag & CSIZE` (`CS8` is every `CSIZE` bit on both OSes).
    pub csize: tcflag_t,
    /// `c_lflag & (ECHO | ICANON | IEXTEN)`.
    pub lflag: tcflag_t,
    pub vsusp: cc_t,
    pub vmin: cc_t,
    pub vtime: cc_t,
}
