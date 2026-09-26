//! Unit tests of the safe wrappers against the real C library. The ABI
//! layout comparison with the native headers, and the strerror comparison
//! with C's own strerror, are in tests/abi.rs; the signal path runs in
//! subprocesses in tests/pty_reference.rs.

use super::*;
use std::ffi::CString;
use std::io::{PipeReader, PipeWriter};
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

fn pipe() -> (PipeReader, PipeWriter) {
    std::io::pipe().expect("pipe")
}

#[test]
fn write_then_read_through_a_pipe() {
    let (reader, writer) = pipe();
    assert_eq!(write(writer.as_raw_fd(), b"flock").unwrap(), 5);
    let mut buffer = [0u8; 16];
    assert_eq!(read(reader.as_raw_fd(), &mut buffer).unwrap(), 5);
    assert_eq!(&buffer[..5], b"flock");
    drop(writer);
    assert_eq!(read(reader.as_raw_fd(), &mut buffer).unwrap(), 0, "end of file");
}

#[test]
fn nonblocking_pipe_reports_eagain_both_ways() {
    let (reader, writer) = pipe();
    let before = status_flags(writer.as_raw_fd()).unwrap();
    assert_eq!(before & O_NONBLOCK, 0);
    set_status_flags(writer.as_raw_fd(), before | O_NONBLOCK).unwrap();
    assert_ne!(status_flags(writer.as_raw_fd()).unwrap() & O_NONBLOCK, 0);
    let chunk = [b'x'; 4096];
    let mut total = 0usize;
    let error = loop {
        match write(writer.as_raw_fd(), &chunk) {
            Ok(n) => total += n,
            Err(e) => break e,
        }
        assert!(total < 64 << 20, "a pipe never filled up");
    };
    assert_eq!(error.raw_os_error(), Some(EAGAIN));
    assert_eq!(EWOULDBLOCK, EAGAIN);
    set_nonblocking(reader.as_raw_fd(), true).unwrap();
    let mut buffer = vec![0u8; total + 1];
    let mut drained = 0usize;
    let error = loop {
        match read(reader.as_raw_fd(), &mut buffer) {
            Ok(n) => drained += n,
            Err(e) => break e,
        }
    };
    assert_eq!(drained, total);
    assert_eq!(error.raw_os_error(), Some(EAGAIN));
    set_nonblocking(writer.as_raw_fd(), false).unwrap();
    // Only O_NONBLOCK: Darwin also reports its internal FWASWRITTEN (0x10000)
    // once the descriptor has been written to.
    assert_eq!(status_flags(writer.as_raw_fd()).unwrap() & O_NONBLOCK, 0);
}

#[test]
fn closed_reader_is_epipe() {
    // The test harness ignores SIGPIPE, as rbirds and cbirds do.
    let (reader, writer) = pipe();
    drop(reader);
    let error = write(writer.as_raw_fd(), b"x").unwrap_err();
    assert_eq!(error.raw_os_error(), Some(EPIPE));
}

#[test]
fn bad_descriptor_is_ebadf() {
    let error = read(-1, &mut [0u8; 1]).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(EBADF));
    assert!(status_flags(-1).is_err());
    assert!(!is_terminal(-1));
}

#[test]
fn poll_reports_readiness() {
    let (reader, writer) = pipe();
    let mut fds =
        [PollFd::new(reader.as_raw_fd(), POLLIN), PollFd::new(writer.as_raw_fd(), POLLOUT)];
    assert_eq!(poll(&mut fds[..1], 0).unwrap(), 0, "nothing to read yet");
    assert_eq!(fds[0].revents, 0);
    write(writer.as_raw_fd(), b"!").unwrap();
    assert_eq!(poll(&mut fds, 1000).unwrap(), 2);
    assert_ne!(fds[0].revents & POLLIN, 0);
    assert_ne!(fds[1].revents & POLLOUT, 0);
    drop(writer);
    let mut fds = [PollFd::new(reader.as_raw_fd(), POLLIN)];
    assert_eq!(poll(&mut fds, 1000).unwrap(), 1);
    assert_ne!(fds[0].revents & (POLLIN | POLLHUP), 0);
    let started = Instant::now();
    let mut none: [PollFd; 0] = [];
    assert_eq!(poll(&mut none, 20).unwrap(), 0);
    assert!(started.elapsed() >= Duration::from_millis(15));
}

#[test]
fn poll_flags_an_unopened_descriptor_and_skips_negative_ones() {
    // Far above anything a test process opens (and above the default
    // descriptor limits), so it is certainly not open.
    let mut fds = [PollFd::new(1 << 20, POLLIN), PollFd::new(-1, POLLIN)];
    assert_eq!(poll(&mut fds, 0).unwrap(), 1);
    assert_eq!(fds[0].revents, POLLNVAL);
    assert_eq!(fds[1].revents, 0, "a negative descriptor is ignored");
}

#[test]
fn strerror_gives_the_c_library_text() {
    assert_eq!(strerror(EINTR), b"Interrupted system call");
    assert_eq!(strerror(EIO), b"Input/output error");
    assert_eq!(strerror(ENOTTY), b"Inappropriate ioctl for device");
    assert_eq!(strerror(EPIPE), b"Broken pipe");
    assert_eq!(strerror(EAGAIN), b"Resource temporarily unavailable");
    #[cfg(target_os = "macos")]
    assert_eq!(strerror(123_456), b"Unknown error: 123456");
    #[cfg(target_os = "linux")]
    assert_eq!(strerror(123_456), b"Unknown error 123456");
}

#[test]
fn perror_message_has_perrors_format() {
    assert_eq!(
        perror_message(b"Can't enable raw mode", ENOTTY),
        b"Can't enable raw mode: Inappropriate ioctl for device\n"
    );
    assert_eq!(perror_message(b"", EPIPE), b"Broken pipe\n");
}

#[test]
fn errno_round_trips() {
    set_errno(ERANGE);
    assert_eq!(errno(), ERANGE);
    set_errno(0);
    assert_eq!(errno(), 0);
}

fn strtod_of(text: &str) -> Strtod {
    strtod(&CString::new(text).unwrap())
}

#[test]
fn strtod_accepts_the_c_grammar() {
    let cases: &[(&str, f64, usize, bool)] = &[
        ("12", 12.0, 2, false),
        ("  \t+7.5", 7.5, 7, false),
        ("-0", -0.0, 2, false),
        ("1e3", 1000.0, 3, false),
        ("0x1p4", 16.0, 5, false),
        ("0X10", 16.0, 4, false),
        ("12abc", 12.0, 2, false),
        ("1e", 1.0, 1, false),
        (".5", 0.5, 2, false),
        ("inf", f64::INFINITY, 3, false),
        ("-Infinity", f64::NEG_INFINITY, 9, false),
        ("1e999", f64::INFINITY, 5, true),
        ("abc", 0.0, 0, false),
        ("", 0.0, 0, false),
    ];
    for &(text, value, consumed, erange) in cases {
        let got = strtod_of(text);
        assert_eq!(got.value.to_bits(), value.to_bits(), "value of {text:?}");
        assert_eq!(got.consumed, consumed, "consumed of {text:?}");
        assert_eq!(got.erange, erange, "ERANGE of {text:?}");
    }
    let nan = strtod_of("nan(1)x");
    assert!(nan.value.is_nan());
    assert_eq!(nan.consumed, 6);
    let tiny = strtod_of("1e-400");
    assert!(tiny.erange, "underflow reports ERANGE");
    assert_eq!(tiny.consumed, 6);
}

#[test]
fn clocks_move_forward_and_sleep_sleeps() {
    let before = monotonic_now();
    assert!(before.tv_nsec >= 0 && before.tv_nsec < 1_000_000_000);
    let started = Instant::now();
    nanosleep(&Timespec { tv_sec: 0, tv_nsec: 2_000_000 }).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(2));
    let after = monotonic_now();
    assert!(after > before);
    let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
        as i64;
    assert!((time_now() - wall).abs() <= 2);
    assert!(clock_gettime(CLOCK_MONOTONIC).is_ok());
    let bad = nanosleep(&Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 }).unwrap_err();
    assert_eq!(bad.raw_os_error(), Some(EINVAL));
}

fn test_pty() -> pty::Pty {
    pty::open_pty(&WinSize { row: 24, col: 80, xpixel: 0, ypixel: 0 }).expect("open a pty")
}

#[test]
fn pty_opens_with_close_on_exec_and_is_a_terminal() {
    let pty = test_pty();
    assert!(is_terminal(pty.slave.as_raw_fd()));
    assert!(pty.slave_path.exists(), "{}", pty.slave_path.display());
    for fd in [pty.master.as_raw_fd(), pty.slave.as_raw_fd()] {
        assert_ne!(descriptor_flags(fd).unwrap() & FD_CLOEXEC, 0);
    }
    let (reader, _writer) = pipe();
    assert!(!is_terminal(reader.as_raw_fd()));
    let error = tcgetattr(reader.as_raw_fd()).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(ENOTTY));
}

#[test]
fn window_size_set_on_the_master_is_seen_on_the_slave() {
    let pty = test_pty();
    let slave = pty.slave.as_raw_fd();
    assert_eq!(window_size(slave).unwrap(), WinSize { row: 24, col: 80, xpixel: 0, ypixel: 0 });
    let size = WinSize { row: 5, col: 10, xpixel: 123, ypixel: 456 };
    set_window_size(pty.master.as_raw_fd(), &size).unwrap();
    assert_eq!(window_size(slave).unwrap(), size);
    assert_eq!(window_size_or_zero(slave), size);
    let (reader, _writer) = pipe();
    assert!(window_size(reader.as_raw_fd()).is_err());
    assert_eq!(window_size_or_zero(reader.as_raw_fd()), WinSize::default());
}

#[test]
fn raw_mode_changes_exactly_what_enter_terminal_changes() {
    let pty = test_pty();
    let slave = pty.slave.as_raw_fd();
    let mut cooked = tcgetattr(slave).unwrap();
    // Start from every bit enter_terminal clears, and a narrow character size.
    cooked.c_iflag |= BRKINT | ICRNL | INPCK | ISTRIP | IXON;
    cooked.c_oflag |= OPOST;
    cooked.c_cflag &= !CSIZE;
    cooked.c_lflag |= ECHO | ICANON | IEXTEN | ISIG;
    cooked.c_cc[VMIN] = 3;
    cooked.c_cc[VTIME] = 7;
    tcsetattr(slave, TCSANOW, &cooked).unwrap();
    let cooked = tcgetattr(slave).unwrap();

    let raw = cooked.raw_mode();
    let view = raw.raw_mode_view();
    assert_eq!(
        view,
        RawModeView {
            iflag: 0,
            oflag: 0,
            csize: CS8,
            lflag: 0,
            vsusp: _POSIX_VDISABLE,
            vmin: 0,
            vtime: 0,
        }
    );
    // Nothing else moved: undo exactly those changes and compare.
    let mut undone = raw;
    undone.c_iflag |= cooked.c_iflag & (BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    undone.c_oflag |= cooked.c_oflag & OPOST;
    undone.c_cflag = (undone.c_cflag & !CSIZE) | (cooked.c_cflag & CSIZE);
    undone.c_lflag |= cooked.c_lflag & (ECHO | ICANON | IEXTEN);
    undone.c_cc[VSUSP] = cooked.c_cc[VSUSP];
    undone.c_cc[VMIN] = cooked.c_cc[VMIN];
    undone.c_cc[VTIME] = cooked.c_cc[VTIME];
    assert_eq!(undone, cooked);
    assert_ne!(raw.c_lflag & ISIG, 0, "signals keep working");

    // And the kernel keeps exactly what was set, both ways.
    tcsetattr(slave, TCSAFLUSH, &raw).unwrap();
    assert_eq!(tcgetattr(slave).unwrap(), raw);
    tcsetattr(slave, TCSAFLUSH, &cooked).unwrap();
    assert_eq!(tcgetattr(slave).unwrap(), cooked);
}

#[test]
fn raw_mode_passes_bytes_through_unchanged() {
    let pty = test_pty();
    let slave = pty.slave.as_raw_fd();
    let master = pty.master.as_raw_fd();
    let cooked = tcgetattr(slave).unwrap();
    tcsetattr(slave, TCSAFLUSH, &cooked.raw_mode()).unwrap();
    // No echo, no CR translation, no line buffering on input...
    write(master, b"a\rb").unwrap();
    set_nonblocking(slave, true).unwrap();
    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while got.len() < 3 && Instant::now() < deadline {
        let mut fds = [PollFd::new(slave, POLLIN)];
        if poll(&mut fds, 100).unwrap() == 1 {
            let mut buffer = [0u8; 16];
            if let Ok(n) = read(slave, &mut buffer) {
                got.extend_from_slice(&buffer[..n]);
            }
        }
    }
    assert_eq!(got, b"a\rb");
    // ... and no NL to CRNL on output.
    write(slave, b"x\ny").unwrap();
    set_nonblocking(master, true).unwrap();
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while out.len() < 3 && Instant::now() < deadline {
        let mut fds = [PollFd::new(master, POLLIN)];
        if poll(&mut fds, 100).unwrap() == 1 {
            let mut buffer = [0u8; 16];
            if let Ok(n) = read(master, &mut buffer) {
                out.extend_from_slice(&buffer[..n]);
            }
        }
    }
    assert_eq!(out, b"x\ny");
}

#[test]
fn write_all_quietly_writes_everything_and_stops_on_errors() {
    let (reader, writer) = pipe();
    let data: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
    let reader_fd: OwnedFd = reader.into();
    let collector = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match read(reader_fd.as_raw_fd(), &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => all.extend_from_slice(&buffer[..n]),
            }
        }
        all
    });
    write_all_quietly(writer.as_raw_fd(), &data);
    drop(writer);
    assert_eq!(collector.join().unwrap(), data);
    // A closed reader: EPIPE, given up silently rather than looping.
    let (reader, writer) = pipe();
    drop(reader);
    write_all_quietly(writer.as_raw_fd(), b"lost");
    write_all_quietly(-1, b"lost too");
}

/// The only in-process test of the global terminal state; it touches no
/// terminal, since neither raw mode nor the alternate screen is marked.
#[test]
fn restore_state_flags_follow_the_c() {
    reset_terminal_state_for_tests();
    assert!(!is_restored() && !terminal_is_raw() && !alt_screen_is_on() && !sprites_uploaded());
    restore_terminal();
    assert!(is_restored());
    restore_terminal();
    assert!(is_restored());
    reset_terminal_state_for_tests();
    mark_sprites_uploaded();
    assert!(sprites_uploaded() && !is_restored());
    reset_terminal_state_for_tests();
    assert!(!sprites_uploaded());
}

#[test]
fn abi_report_is_well_formed() {
    let report = abi::rust_layout_report();
    for line in report.lines() {
        let fields: Vec<&str> = line.splitn(3, ' ').collect();
        assert_eq!(fields.len(), 3, "{line}");
        assert!(
            ["size", "align", "signed", "offset", "const", "fn"].contains(&fields[0]),
            "{line}"
        );
    }
    assert!(report.contains(&format!("size termios {}\n", std::mem::size_of::<Termios>())));
}
