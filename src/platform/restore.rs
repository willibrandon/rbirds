//! Terminal ownership and the emergency restore path: boids.c's
//! `saved_termios`, `terminal_is_raw`, `terminal_restored`, `alt_screen_is_on`,
//! `sprites_uploaded`, `write_all`, `enter_terminal`, `enter_alt_screen` and
//! `restore_terminal`.
//!
//! The state is process-global, as in the C, because the signal handler has
//! no other way to reach it. The terminal state the handler touches is here and is
//! async-signal-safe: lock-free atomics, `tcsetattr`, `write`, `errno`. No
//! allocation, locking, formatting or unwinding happens on this path.
//! macOS shared-image cleanup is separately audited in `shared_image.rs`.
//!
//! The saved attributes are kept field by field in atomics rather than as a
//! `Termios` behind an `UnsafeCell`. The protocol is the same: they are
//! written only before `terminal_is_raw` is set (Release), and read by the
//! handler only after it observes the flag (Acquire). With atomics, though,
//! even a violation of it (a second [`mark_raw_acquired`] racing a handler on
//! another thread) cannot be a data race: at worst the handler restores a
//! mixture of two saved states. Padding is never copied, because the value is
//! rebuilt by field.
//!
//! Tests share one process, so only subprocess tests (tests/pty_reference.rs)
//! exercise this path for real; in-process tests may only use
//! [`reset_terminal_state_for_tests`] around flag checks.

use super::os::{AtomicTcflag, NCCS, TCSAFLUSH, Termios};
use super::termios::{tcgetattr, tcsetattr};
use super::{EINTR, STDIN_FILENO, STDOUT_FILENO, errno, sys};
use std::ffi::c_void;
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

// The handler relies on these atomics being lock-free (a lock could be held by
// the very code the signal interrupted).
#[cfg(not(all(target_has_atomic = "8", target_has_atomic = "32", target_has_atomic = "64")))]
compile_error!("the signal path needs lock-free 8-, 32- and 64-bit atomics");

/// `\e[?1049h`: switch to the alternate screen.
pub const ALT_SCREEN_ON: &[u8] = b"\x1b[?1049h";
/// `\e[?1049l`: back to the main screen.
pub const ALT_SCREEN_OFF: &[u8] = b"\x1b[?1049l";
/// `\e[?25l`.
pub const CURSOR_HIDE: &[u8] = b"\x1b[?25l";
/// `\e[?25h`.
pub const CURSOR_SHOW: &[u8] = b"\x1b[?25h";
/// `\e[?2026l`: end any synchronized update left open.
pub const SYNC_UPDATE_END: &[u8] = b"\x1b[?2026l";
/// Any-event mouse tracking plus SGR coordinates.
pub const MOUSE_REPORT_ON: &[u8] = b"\x1b[?1003h\x1b[?1006h";
/// The reverse of [`MOUSE_REPORT_ON`], in reverse order.
pub const MOUSE_REPORT_OFF: &[u8] = b"\x1b[?1006l\x1b[?1003l";
/// Kitty: free every image left without a placement.
pub const KITTY_FREE_IMAGES: &[u8] = b"\x1b_Ga=d,d=A,q=2\x1b\\";

static TERMINAL_IS_RAW: AtomicBool = AtomicBool::new(false);
static TERMINAL_RESTORED: AtomicBool = AtomicBool::new(false);
static ALT_SCREEN_IS_ON: AtomicBool = AtomicBool::new(false);
static SPRITES_UPLOADED: AtomicBool = AtomicBool::new(false);
static SIXEL_MODE: AtomicU8 = AtomicU8::new(0);
static SAVED_TERMIOS: SavedTermios = SavedTermios::new();

/// `saved_termios`, one atomic per field.
struct SavedTermios {
    flags: [AtomicTcflag; 4],
    speeds: [AtomicTcflag; 2],
    cc: [AtomicU8; NCCS],
    #[cfg(target_os = "linux")]
    line: AtomicU8,
}

impl SavedTermios {
    const fn new() -> SavedTermios {
        SavedTermios {
            flags: [const { AtomicTcflag::new(0) }; 4],
            speeds: [const { AtomicTcflag::new(0) }; 2],
            cc: [const { AtomicU8::new(0) }; NCCS],
            #[cfg(target_os = "linux")]
            line: AtomicU8::new(0),
        }
    }

    fn store(&self, termios: &Termios) {
        let flags = [termios.c_iflag, termios.c_oflag, termios.c_cflag, termios.c_lflag];
        for (slot, value) in self.flags.iter().zip(flags) {
            slot.store(value, Ordering::Relaxed);
        }
        self.speeds[0].store(termios.c_ispeed, Ordering::Relaxed);
        self.speeds[1].store(termios.c_ospeed, Ordering::Relaxed);
        for (slot, &value) in self.cc.iter().zip(termios.c_cc.iter()) {
            slot.store(value, Ordering::Relaxed);
        }
        #[cfg(target_os = "linux")]
        self.line.store(termios.c_line, Ordering::Relaxed);
    }

    /// Async-signal-safe: atomic loads into a local only.
    fn load(&self) -> Termios {
        let mut termios = Termios {
            c_iflag: self.flags[0].load(Ordering::Relaxed),
            c_oflag: self.flags[1].load(Ordering::Relaxed),
            c_cflag: self.flags[2].load(Ordering::Relaxed),
            c_lflag: self.flags[3].load(Ordering::Relaxed),
            c_ispeed: self.speeds[0].load(Ordering::Relaxed),
            c_ospeed: self.speeds[1].load(Ordering::Relaxed),
            ..Termios::default()
        };
        for (value, slot) in termios.c_cc.iter_mut().zip(self.cc.iter()) {
            *value = slot.load(Ordering::Relaxed);
        }
        #[cfg(target_os = "linux")]
        {
            termios.c_line = self.line.load(Ordering::Relaxed);
        }
        termios
    }
}

/// boids.c `write_all`: writes `data` to `fd`, retrying `EINTR` and short
/// writes, and silently giving up on any other error or a zero-length write.
/// Async-signal-safe (no allocation, no `io::Error`).
pub fn write_all_quietly(fd: RawFd, data: &[u8]) {
    let mut rest = data;
    while !rest.is_empty() {
        // SAFETY: `rest` is a live slice, valid for `rest.len()` readable
        // bytes during the call; write(2) does not retain the pointer.
        let written = unsafe { sys::write(fd, rest.as_ptr().cast::<c_void>(), rest.len()) };
        if written < 0 {
            if errno() == EINTR {
                continue;
            }
            return;
        }
        if written == 0 {
            return;
        }
        rest = rest.get(written as usize..).unwrap_or_default();
    }
}

/// Records that raw mode is on: stores `saved` (the attributes to put back)
/// and then sets `terminal_is_raw` with Release ordering, so a signal handler
/// that sees the flag sees the whole of `saved`. Call it once, right after
/// the `tcsetattr` that made the terminal raw succeeded.
pub fn mark_raw_acquired(saved: Termios) {
    SAVED_TERMIOS.store(&saved);
    TERMINAL_IS_RAW.store(true, Ordering::Release);
}

/// Records `alt_screen_is_on = 1`: from now on a restore also shows the
/// cursor, stops mouse reports, ends synchronized updates and leaves the
/// alternate screen.
pub fn mark_alt_screen_on() {
    ALT_SCREEN_IS_ON.store(true, Ordering::SeqCst);
}

/// Records `sprites_uploaded = 1`: a restore also frees Kitty images. The C
/// sets it before uploading, since a failed upload may have left some behind.
pub fn mark_sprites_uploaded() {
    SPRITES_UPLOADED.store(true, Ordering::SeqCst);
}

pub fn enable_sixel_mode(was_enabled: bool) {
    SIXEL_MODE.store(if was_enabled { 1 } else { 2 }, Ordering::SeqCst);
    write_all_quietly(STDOUT_FILENO, b"\x1b[?80h");
}

/// Whether [`restore_terminal`] has run (`terminal_restored`).
pub fn is_restored() -> bool {
    TERMINAL_RESTORED.load(Ordering::SeqCst)
}

/// `terminal_is_raw`: raw mode acquired and not yet restored.
pub fn terminal_is_raw() -> bool {
    TERMINAL_IS_RAW.load(Ordering::Acquire)
}

/// `alt_screen_is_on`.
pub fn alt_screen_is_on() -> bool {
    ALT_SCREEN_IS_ON.load(Ordering::SeqCst)
}

/// `sprites_uploaded`.
pub fn sprites_uploaded() -> bool {
    SPRITES_UPLOADED.load(Ordering::SeqCst)
}

/// Puts the process-global terminal state back to "untouched" so an
/// in-process test can start from it. Only for tests: the application never
/// calls it, and it must not race a signal handler or another test using the
/// state (tests that do must not run concurrently with each other).
pub fn reset_terminal_state_for_tests() {
    TERMINAL_IS_RAW.store(false, Ordering::SeqCst);
    TERMINAL_RESTORED.store(false, Ordering::SeqCst);
    ALT_SCREEN_IS_ON.store(false, Ordering::SeqCst);
    SPRITES_UPLOADED.store(false, Ordering::SeqCst);
    SIXEL_MODE.store(0, Ordering::SeqCst);
    SAVED_TERMIOS.store(&Termios::default());
}

/// boids.c `enter_terminal`: reads standard input's attributes, applies
/// [`Termios::raw_mode`] with `TCSAFLUSH`, and on success records the
/// original attributes with [`mark_raw_acquired`]. The error is the failing
/// call's `errno` (what `perror("Can't enable raw mode")` reports).
pub fn enter_terminal() -> io::Result<()> {
    let saved = tcgetattr(STDIN_FILENO)?;
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &saved.raw_mode())?;
    mark_raw_acquired(saved);
    Ok(())
}

/// boids.c `enter_alt_screen`: writes [`ALT_SCREEN_ON`], [`CURSOR_HIDE`] and
/// [`MOUSE_REPORT_ON`] to standard output, then marks the alternate screen on.
pub fn enter_alt_screen() {
    write_all_quietly(STDOUT_FILENO, ALT_SCREEN_ON);
    write_all_quietly(STDOUT_FILENO, CURSOR_HIDE);
    write_all_quietly(STDOUT_FILENO, MOUSE_REPORT_ON);
    mark_alt_screen_on();
}

/// boids.c `restore_terminal`, step for step. Idempotent: the first call marks
/// the terminal restored; later calls, including one from a signal handler
/// that interrupts the first, do nothing. Then, if raw mode was acquired, the
/// saved attributes go back with `tcsetattr(STDIN_FILENO, TCSAFLUSH, ...)`
/// (its result ignored). If the alternate screen was never entered it stops
/// there; otherwise it writes, each with [`write_all_quietly`] to standard
/// output: [`KITTY_FREE_IMAGES`] if sprites were uploaded, then
/// [`MOUSE_REPORT_OFF`], [`SYNC_UPDATE_END`], [`CURSOR_SHOW`],
/// [`ALT_SCREEN_OFF`]. Async-signal-safe.
pub fn restore_terminal() {
    #[cfg(target_os = "macos")]
    super::shared_image::cleanup();
    // A plain check then set, as the C's `sig_atomic_t` flag is, rather than a
    // swap: the window between them behaves exactly as the reference's does.
    if TERMINAL_RESTORED.load(Ordering::SeqCst) {
        return;
    }
    TERMINAL_RESTORED.store(true, Ordering::SeqCst);
    if SIXEL_MODE.swap(0, Ordering::SeqCst) == 2 {
        write_all_quietly(STDOUT_FILENO, b"\x1b[?80l");
    }
    if TERMINAL_IS_RAW.load(Ordering::Acquire) {
        let saved = SAVED_TERMIOS.load();
        // Ignored, as in the C: there is nothing better to do on this path.
        let _ = tcsetattr(STDIN_FILENO, TCSAFLUSH, &saved);
        TERMINAL_IS_RAW.store(false, Ordering::SeqCst);
    }
    if !ALT_SCREEN_IS_ON.load(Ordering::SeqCst) {
        return;
    }
    if SPRITES_UPLOADED.load(Ordering::SeqCst) {
        write_all_quietly(STDOUT_FILENO, KITTY_FREE_IMAGES);
    }
    write_all_quietly(STDOUT_FILENO, MOUSE_REPORT_OFF);
    write_all_quietly(STDOUT_FILENO, SYNC_UPDATE_END);
    write_all_quietly(STDOUT_FILENO, CURSOR_SHOW);
    write_all_quietly(STDOUT_FILENO, ALT_SCREEN_OFF);
}
