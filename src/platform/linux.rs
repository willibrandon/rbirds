//! GNU/Linux (glibc) types and constants, for aarch64 and x86_64 alike.
//!
//! These are glibc's user-space definitions (`<bits/termios-struct.h>`,
//! `<bits/types/struct_sigaction.h>`, `<asm-generic/ioctls.h>`, ...), which
//! both architectures share, and not the kernel's own `struct termios` or
//! `struct sigaction`: glibc converts between them. Checked on both
//! architectures by `tools/oracle/abi_probe.c` through tests/abi.rs.

#![allow(non_camel_case_types)]

use std::ffi::{c_int, c_short, c_uint, c_ulong};

/// `unsigned int`.
pub type tcflag_t = c_uint;
/// `unsigned char`.
pub type cc_t = u8;
/// `unsigned int`.
pub type speed_t = c_uint;
/// `long`.
pub type time_t = i64;
/// `int`.
pub type clockid_t = c_int;
/// `unsigned long int`.
pub type nfds_t = c_ulong;
/// `int`.
pub type pid_t = i32;

/// `__sigset_t`: 1024 signal bits, as glibc reserves room for.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct sigset_t {
    pub __val: [c_ulong; 16],
}

pub const NCCS: usize = 32;

/// glibc's `struct termios`: four flag words, the line discipline, `NCCS`
/// control characters, then (after three bytes of padding) the two speeds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Termios {
    pub c_iflag: tcflag_t,
    pub c_oflag: tcflag_t,
    pub c_cflag: tcflag_t,
    pub c_lflag: tcflag_t,
    pub c_line: cc_t,
    pub c_cc: [cc_t; NCCS],
    pub c_ispeed: speed_t,
    pub c_ospeed: speed_t,
}

/// The lock-free atomic as wide as `tcflag_t` and `speed_t`, for the copy of
/// the saved attributes the signal handler may read (see `restore.rs`).
pub type AtomicTcflag = std::sync::atomic::AtomicU32;

pub const VTIME: usize = 5;
pub const VMIN: usize = 6;
pub const VSUSP: usize = 10;
pub const _POSIX_VDISABLE: cc_t = 0;

pub const BRKINT: tcflag_t = 0o000_002;
pub const INPCK: tcflag_t = 0o000_020;
pub const ISTRIP: tcflag_t = 0o000_040;
pub const ICRNL: tcflag_t = 0o000_400;
pub const IXON: tcflag_t = 0o002_000;
pub const OPOST: tcflag_t = 0o000_001;
pub const CSIZE: tcflag_t = 0o000_060;
pub const CS8: tcflag_t = 0o000_060;
pub const ISIG: tcflag_t = 0o000_001;
pub const ICANON: tcflag_t = 0o000_002;
pub const ECHO: tcflag_t = 0o000_010;
pub const IEXTEN: tcflag_t = 0o100_000;

pub const TCSANOW: c_int = 0;
pub const TCSAFLUSH: c_int = 2;

pub const TIOCGWINSZ: c_ulong = 0x5413;
pub const TIOCSWINSZ: c_ulong = 0x5414;
/// `_IOR('T', 0x30, unsigned int)`: the pseudoterminal's number, which is how
/// glibc's own `ptsname_r` names the slave.
pub const TIOCGPTN: c_ulong = 0x8004_5430;

pub const POLLIN: c_short = 0x0001;
pub const POLLOUT: c_short = 0x0004;
pub const POLLERR: c_short = 0x0008;
pub const POLLHUP: c_short = 0x0010;
pub const POLLNVAL: c_short = 0x0020;

pub const CLOCK_MONOTONIC: clockid_t = 1;

/// glibc's `struct sigaction`: the handler union, the 128-byte mask, the
/// flags, then the restorer glibc fills in itself.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SigAction {
    /// `__sigaction_handler`: a handler address, or `SIG_IGN`/`SIG_DFL`.
    pub sa_handler: usize,
    pub sa_mask: sigset_t,
    pub sa_flags: c_int,
    pub sa_restorer: usize,
}

/// `0x80000000`, which boids.c stores as `(int)SA_RESETHAND`.
pub const SA_RESETHAND: c_int = 0x8000_0000_u32 as c_int;
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGQUIT: c_int = 3;
pub const SIGABRT: c_int = 6;
pub const SIGBUS: c_int = 7;
pub const SIGFPE: c_int = 8;
pub const SIGKILL: c_int = 9;
pub const SIGSEGV: c_int = 11;
pub const SIGPIPE: c_int = 13;
pub const SIGTERM: c_int = 15;

pub const EBADF: c_int = 9;
pub const EINVAL: c_int = 22;
pub const ENOTTY: c_int = 25;

pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const FD_CLOEXEC: c_int = 1;
pub const O_RDWR: c_int = 0o2;
pub const O_CLOEXEC: c_int = 0o2_000_000;
pub const O_NOCTTY: c_int = 0o400;
