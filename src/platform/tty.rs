//! Window size (`TIOCGWINSZ`/`TIOCSWINSZ`) and `isatty`.

use super::os::{TIOCGWINSZ, TIOCSWINSZ};
use super::sys;
use std::io;
use std::os::fd::RawFd;

/// `struct winsize`: `ws_row`, `ws_col`, `ws_xpixel`, `ws_ypixel`, each an
/// `unsigned short`, on every supported target.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WinSize {
    /// `ws_row`: rows of cells.
    pub row: u16,
    /// `ws_col`: columns of cells.
    pub col: u16,
    /// `ws_xpixel`: width in pixels, zero when the terminal does not say.
    pub xpixel: u16,
    /// `ws_ypixel`: height in pixels, zero when the terminal does not say.
    pub ypixel: u16,
}

/// `ioctl(fd, TIOCGWINSZ, &size)`.
pub fn window_size(fd: RawFd) -> io::Result<WinSize> {
    let mut size = WinSize::default();
    // SAFETY: TIOCGWINSZ takes a `struct winsize *`; `size` is a live, writable
    // `WinSize` with that layout (ABI probe), and the kernel keeps no pointer.
    let result = unsafe { sys::ioctl(fd, TIOCGWINSZ, &mut size as *mut WinSize) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(size) }
}

/// boids.c `update_screen_dimensions`'s query: the size, or all zeros when the
/// ioctl fails (the C clears the structure before and after a failure).
pub fn window_size_or_zero(fd: RawFd) -> WinSize {
    window_size(fd).unwrap_or_default()
}

/// `ioctl(fd, TIOCSWINSZ, &size)`, as a terminal emulator does on its master.
pub fn set_window_size(fd: RawFd, size: &WinSize) -> io::Result<()> {
    // SAFETY: TIOCSWINSZ takes a `const struct winsize *`; `size` is a live
    // `WinSize` with that layout, only read during the call.
    let result = unsafe { sys::ioctl(fd, TIOCSWINSZ, size as *const WinSize) };
    if result < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// `isatty(fd) == 1`.
pub fn is_terminal(fd: RawFd) -> bool {
    // SAFETY: isatty takes a descriptor by value and touches no memory of ours.
    unsafe { sys::isatty(fd) == 1 }
}
