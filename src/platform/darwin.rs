//! Darwin (macOS) types and constants, for aarch64 and x86_64 alike.
//!
//! Transcribed from the macOS SDK headers the reference compiles against
//! (`<sys/termios.h>`, `<sys/ttycom.h>`, `<sys/signal.h>`, `<poll.h>`,
//! `<time.h>`, `<fcntl.h>`) and checked on both architectures by
//! `tools/oracle/abi_probe.c` through tests/abi.rs. Nothing here is assumed
//! from another Unix: every value is the Darwin one.

#![allow(non_camel_case_types)]

use std::ffi::{c_int, c_short, c_uint, c_ulong};

/// `unsigned long`.
pub type tcflag_t = c_ulong;
/// `unsigned char`.
pub type cc_t = u8;
/// `unsigned long`.
pub type speed_t = c_ulong;
/// `long` (`__darwin_time_t`).
pub type time_t = i64;
/// An enumeration whose enumerators are all non-negative, which Apple clang
/// lays out as `unsigned int`.
pub type clockid_t = c_uint;
/// `unsigned int`.
pub type nfds_t = c_uint;
/// `int` (`__int32_t`).
pub type pid_t = i32;
/// `__uint32_t`: one bit per signal.
pub type sigset_t = u32;

pub const NCCS: usize = 20;

/// `struct termios`: four flag words, `NCCS` control characters, then the
/// two speeds, with four bytes of padding after `c_cc` on both architectures.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Termios {
    pub c_iflag: tcflag_t,
    pub c_oflag: tcflag_t,
    pub c_cflag: tcflag_t,
    pub c_lflag: tcflag_t,
    pub c_cc: [cc_t; NCCS],
    pub c_ispeed: speed_t,
    pub c_ospeed: speed_t,
}

/// The lock-free atomic as wide as `tcflag_t` and `speed_t`, for the copy of
/// the saved attributes the signal handler may read (see `restore.rs`).
pub type AtomicTcflag = std::sync::atomic::AtomicU64;

pub const VSUSP: usize = 10;
pub const VMIN: usize = 16;
pub const VTIME: usize = 17;
pub const _POSIX_VDISABLE: cc_t = 0xff;

pub const BRKINT: tcflag_t = 0x0000_0002;
pub const INPCK: tcflag_t = 0x0000_0010;
pub const ISTRIP: tcflag_t = 0x0000_0020;
pub const ICRNL: tcflag_t = 0x0000_0100;
pub const IXON: tcflag_t = 0x0000_0200;
pub const OPOST: tcflag_t = 0x0000_0001;
pub const CSIZE: tcflag_t = 0x0000_0300;
pub const CS8: tcflag_t = 0x0000_0300;
pub const ECHO: tcflag_t = 0x0000_0008;
pub const ISIG: tcflag_t = 0x0000_0080;
pub const ICANON: tcflag_t = 0x0000_0100;
pub const IEXTEN: tcflag_t = 0x0000_0400;

pub const TCSANOW: c_int = 0;
pub const TCSAFLUSH: c_int = 2;

/// `_IOR('t', 104, struct winsize)`.
pub const TIOCGWINSZ: c_ulong = 0x4008_7468;
/// `_IOW('t', 103, struct winsize)`.
pub const TIOCSWINSZ: c_ulong = 0x8008_7467;

pub const POLLIN: c_short = 0x0001;
pub const POLLOUT: c_short = 0x0004;
pub const POLLERR: c_short = 0x0008;
pub const POLLHUP: c_short = 0x0010;
pub const POLLNVAL: c_short = 0x0020;

pub const CLOCK_MONOTONIC: clockid_t = 6;
pub const CLOCK_PROCESS_CPUTIME_ID: clockid_t = 12;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Kevent64 {
    pub ident: u64,
    pub filter: i16,
    pub flags: u16,
    pub fflags: u32,
    pub data: i64,
    pub udata: u64,
    pub ext: [u64; 2],
}
pub const EVFILT_TIMER: i16 = -7;
pub const EV_ADD: u16 = 0x0001;
pub const EV_ONESHOT: u16 = 0x0010;
pub const EV_ERROR: u16 = 0x4000;
pub const NOTE_NSECONDS: u32 = 0x0004;
pub const NOTE_CRITICAL: u32 = 0x0020;

/// The `struct sigaction` that libSystem's `sigaction()` takes (not the
/// kernel's `struct __sigaction`, which adds a trampoline): the handler union,
/// the mask, the flags.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SigAction {
    /// `union __sigaction_u`: a handler address, or `SIG_IGN`/`SIG_DFL`.
    pub sa_handler: usize,
    pub sa_mask: sigset_t,
    pub sa_flags: c_int,
}

pub const SA_RESETHAND: c_int = 0x0004;
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGQUIT: c_int = 3;
pub const SIGABRT: c_int = 6;
pub const SIGFPE: c_int = 8;
pub const SIGKILL: c_int = 9;
pub const SIGBUS: c_int = 10;
pub const SIGSEGV: c_int = 11;
pub const SIGPIPE: c_int = 13;
pub const SIGTERM: c_int = 15;

pub const EBADF: c_int = 9;
pub const EINVAL: c_int = 22;
pub const ENOTTY: c_int = 25;

pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const FD_CLOEXEC: c_int = 1;
pub const O_RDWR: c_int = 0x0002;
pub const O_CLOEXEC: c_int = 0x0100_0000;
pub const O_NOCTTY: c_int = 0x0002_0000;
