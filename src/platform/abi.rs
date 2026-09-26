//! The Rust half of the ABI check (docs/DESIGN.md §6).
//!
//! [`rust_layout_report`] prints, from the Rust bindings, the same lines
//! `tools/oracle/abi_probe.c` prints from the native headers, in the same
//! order: sizes, alignments and field offsets of every `#[repr(C)]` structure
//! passed to the C library, width and signedness of every scalar type in a
//! foreign signature, every constant's value, and the C prototype each
//! foreign declaration was written against. tests/abi.rs compiles the probe
//! for the target under test and requires the two reports to be identical.

use super::os::{self, SigAction, Termios, cc_t, sigset_t};
use super::poll::PollFd;
use super::time::Timespec;
use super::tty::WinSize;
use super::{
    EAGAIN, EINTR, EIO, EPIPE, ERANGE, EWOULDBLOCK, F_GETFL, F_SETFL, O_NONBLOCK, STDERR_FILENO,
    STDIN_FILENO, STDOUT_FILENO, sys,
};
use std::ffi::{c_char, c_int, c_long, c_short, c_uint, c_ulong, c_void};
use std::fmt::Write;
use std::mem::{align_of, offset_of, size_of};

/// Whether an integer type is signed, as `((type)-1 < 0)` tells in C.
trait Scalar {
    const SIGNED: bool;
}

macro_rules! scalar_types {
    ($($t:ty => $signed:expr),*) => { $(impl Scalar for $t { const SIGNED: bool = $signed; })* };
}
scalar_types!(i8 => true, i16 => true, i32 => true, i64 => true, isize => true,
              u8 => false, u16 => false, u32 => false, u64 => false, usize => false);

struct Report(String);

impl Report {
    fn line(&mut self, args: std::fmt::Arguments<'_>) {
        // Writing to a String cannot fail.
        let _ = self.0.write_fmt(args);
        self.0.push('\n');
    }

    fn scalar<T: Scalar>(&mut self, name: &str) {
        self.line(format_args!("size {name} {}", size_of::<T>()));
        self.line(format_args!("align {name} {}", align_of::<T>()));
        self.line(format_args!("signed {name} {}", u8::from(T::SIGNED)));
    }

    fn size<T>(&mut self, name: &str) {
        self.line(format_args!("size {name} {}", size_of::<T>()));
    }

    fn align<T>(&mut self, name: &str) {
        self.line(format_args!("align {name} {}", align_of::<T>()));
    }

    fn layout<T>(&mut self, name: &str) {
        self.size::<T>(name);
        self.align::<T>(name);
    }

    fn offset(&mut self, name: &str, offset: usize) {
        self.line(format_args!("offset {name} {offset}"));
    }

    fn field_size(&mut self, name: &str, size: usize) {
        self.line(format_args!("size {name} {size}"));
    }

    fn constant(&mut self, name: &str, value: i128) {
        self.line(format_args!("const {name} {value}"));
    }
}

/// The report, one `\n`-terminated line per fact, identical to the probe's
/// output when the bindings are right for this target.
pub fn rust_layout_report() -> String {
    let mut r = Report(String::new());

    // Scalar types in the bindings' signatures.
    r.scalar::<c_char>("char");
    r.scalar::<c_short>("short");
    r.scalar::<c_int>("int");
    r.scalar::<c_uint>("unsigned int");
    r.scalar::<c_long>("long");
    r.scalar::<c_ulong>("unsigned long");
    r.scalar::<usize>("size_t");
    r.scalar::<isize>("ssize_t");
    r.scalar::<os::pid_t>("pid_t");
    r.scalar::<os::time_t>("time_t");
    r.scalar::<os::clockid_t>("clockid_t");
    r.scalar::<os::nfds_t>("nfds_t");
    r.scalar::<os::tcflag_t>("tcflag_t");
    r.scalar::<os::cc_t>("cc_t");
    r.scalar::<os::speed_t>("speed_t");
    r.layout::<*const c_void>("pointer");

    // struct termios
    r.layout::<Termios>("termios");
    r.offset("termios.c_iflag", offset_of!(Termios, c_iflag));
    r.offset("termios.c_oflag", offset_of!(Termios, c_oflag));
    r.offset("termios.c_cflag", offset_of!(Termios, c_cflag));
    r.offset("termios.c_lflag", offset_of!(Termios, c_lflag));
    #[cfg(target_os = "linux")]
    r.offset("termios.c_line", offset_of!(Termios, c_line));
    r.offset("termios.c_cc", offset_of!(Termios, c_cc));
    r.field_size("termios.c_cc", size_of::<[cc_t; os::NCCS]>());
    r.offset("termios.c_ispeed", offset_of!(Termios, c_ispeed));
    r.offset("termios.c_ospeed", offset_of!(Termios, c_ospeed));
    r.constant("NCCS", os::NCCS as i128);
    r.constant("VMIN", os::VMIN as i128);
    r.constant("VTIME", os::VTIME as i128);
    r.constant("VSUSP", os::VSUSP as i128);
    r.constant("(cc_t)_POSIX_VDISABLE", os::_POSIX_VDISABLE.into());
    for (name, value) in [
        ("BRKINT", os::BRKINT),
        ("ICRNL", os::ICRNL),
        ("INPCK", os::INPCK),
        ("ISTRIP", os::ISTRIP),
        ("IXON", os::IXON),
        ("OPOST", os::OPOST),
        ("CSIZE", os::CSIZE),
        ("CS8", os::CS8),
        ("ECHO", os::ECHO),
        ("ICANON", os::ICANON),
        ("IEXTEN", os::IEXTEN),
        ("ISIG", os::ISIG),
    ] {
        r.constant(name, value.into());
    }
    r.constant("TCSANOW", os::TCSANOW.into());
    r.constant("TCSAFLUSH", os::TCSAFLUSH.into());

    // struct winsize and its requests
    r.layout::<WinSize>("winsize");
    r.offset("winsize.ws_row", offset_of!(WinSize, row));
    r.offset("winsize.ws_col", offset_of!(WinSize, col));
    r.offset("winsize.ws_xpixel", offset_of!(WinSize, xpixel));
    r.offset("winsize.ws_ypixel", offset_of!(WinSize, ypixel));
    r.constant("(unsigned long)TIOCGWINSZ", os::TIOCGWINSZ.into());
    r.constant("(unsigned long)TIOCSWINSZ", os::TIOCSWINSZ.into());
    #[cfg(target_os = "linux")]
    r.constant("(unsigned long)TIOCGPTN", os::TIOCGPTN.into());

    // struct pollfd
    r.layout::<PollFd>("pollfd");
    r.offset("pollfd.fd", offset_of!(PollFd, fd));
    r.offset("pollfd.events", offset_of!(PollFd, events));
    r.offset("pollfd.revents", offset_of!(PollFd, revents));
    r.field_size("pollfd.events", size_of::<c_short>());
    for (name, value) in [
        ("POLLIN", os::POLLIN),
        ("POLLOUT", os::POLLOUT),
        ("POLLERR", os::POLLERR),
        ("POLLHUP", os::POLLHUP),
        ("POLLNVAL", os::POLLNVAL),
    ] {
        r.constant(name, value.into());
    }

    // struct timespec and the clock
    r.layout::<Timespec>("timespec");
    r.offset("timespec.tv_sec", offset_of!(Timespec, tv_sec));
    r.offset("timespec.tv_nsec", offset_of!(Timespec, tv_nsec));
    r.field_size("timespec.tv_nsec", size_of::<c_long>());
    r.constant("CLOCK_MONOTONIC", os::CLOCK_MONOTONIC.into());
    r.constant("CLOCK_PROCESS_CPUTIME_ID", os::CLOCK_PROCESS_CPUTIME_ID.into());
    #[cfg(target_os = "macos")]
    {
        use os::Kevent64;
        r.layout::<Kevent64>("kevent64_s");
        r.offset("kevent64_s.ident", offset_of!(Kevent64, ident));
        r.offset("kevent64_s.filter", offset_of!(Kevent64, filter));
        r.offset("kevent64_s.flags", offset_of!(Kevent64, flags));
        r.offset("kevent64_s.fflags", offset_of!(Kevent64, fflags));
        r.offset("kevent64_s.data", offset_of!(Kevent64, data));
        r.offset("kevent64_s.udata", offset_of!(Kevent64, udata));
        r.offset("kevent64_s.ext", offset_of!(Kevent64, ext));
        for (name, value) in [
            ("EVFILT_TIMER", i128::from(os::EVFILT_TIMER)),
            ("EV_ADD", os::EV_ADD.into()),
            ("EV_ONESHOT", os::EV_ONESHOT.into()),
            ("EV_ERROR", os::EV_ERROR.into()),
            ("NOTE_NSECONDS", os::NOTE_NSECONDS.into()),
            ("NOTE_CRITICAL", os::NOTE_CRITICAL.into()),
        ] {
            r.constant(name, value);
        }
    }

    // struct sigaction, as the sigaction() wrapper takes it
    r.layout::<sigset_t>("sigset_t");
    r.layout::<SigAction>("sigaction");
    r.offset("sigaction.sa_handler", offset_of!(SigAction, sa_handler));
    r.offset("sigaction.sa_mask", offset_of!(SigAction, sa_mask));
    r.offset("sigaction.sa_flags", offset_of!(SigAction, sa_flags));
    #[cfg(target_os = "linux")]
    r.offset("sigaction.sa_restorer", offset_of!(SigAction, sa_restorer));
    r.field_size("sigaction.sa_flags", size_of::<c_int>());
    r.constant("(int)SA_RESETHAND", os::SA_RESETHAND.into());
    r.constant("(intptr_t)SIG_IGN", os::SIG_IGN as i128);
    r.constant("(intptr_t)SIG_DFL", os::SIG_DFL as i128);
    for (name, value) in [
        ("SIGHUP", os::SIGHUP),
        ("SIGINT", os::SIGINT),
        ("SIGQUIT", os::SIGQUIT),
        ("SIGABRT", os::SIGABRT),
        ("SIGBUS", os::SIGBUS),
        ("SIGFPE", os::SIGFPE),
        ("SIGKILL", os::SIGKILL),
        ("SIGSEGV", os::SIGSEGV),
        ("SIGPIPE", os::SIGPIPE),
        ("SIGTERM", os::SIGTERM),
        // errno values and descriptor flags
        ("EINTR", EINTR),
        ("EIO", EIO),
        ("EBADF", os::EBADF),
        ("EAGAIN", EAGAIN),
        ("EWOULDBLOCK", EWOULDBLOCK),
        ("EINVAL", os::EINVAL),
        ("ENOTTY", os::ENOTTY),
        ("EPIPE", EPIPE),
        ("ERANGE", ERANGE),
        ("STDIN_FILENO", STDIN_FILENO),
        ("STDOUT_FILENO", STDOUT_FILENO),
        ("STDERR_FILENO", STDERR_FILENO),
        ("F_GETFL", F_GETFL),
        ("F_SETFL", F_SETFL),
        ("F_GETFD", os::F_GETFD),
        ("F_SETFD", os::F_SETFD),
        ("FD_CLOEXEC", os::FD_CLOEXEC),
        ("O_NONBLOCK", O_NONBLOCK),
        ("O_RDWR", os::O_RDWR),
        ("O_NOCTTY", os::O_NOCTTY),
        ("O_CLOEXEC", os::O_CLOEXEC),
    ] {
        r.constant(name, value.into());
    }

    // Prototypes the foreign declarations were written against.
    for (name, prototype) in sys::signatures() {
        r.line(format_args!("fn {name} {prototype}"));
    }
    r.0
}
