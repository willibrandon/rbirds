//! The terminal as a resource: raw mode, the alternate screen, the questions
//! asked of it at startup, and putting everything back. Translated from
//! cbirds `boids.c` (`enter_terminal`, `enter_alt_screen`, `terminal_query`,
//! `learn_the_theme`, `update_screen_dimensions`, `wait_for_terminal_io`).
//!
//! Ownership is a state machine held by the platform layer's emergency state,
//! which the signal handler also reads: untouched, raw mode acquired,
//! alternate screen entered, sprites possibly uploaded, restored. Restoring
//! is idempotent. [`Terminal`] restores on drop as the fallback for every exit
//! path, a panic included; the process never exits while one is alive.

#![forbid(unsafe_code)]

use std::io;

use crate::palette::{self, Rgb, Theme};
use crate::platform::{self, PollFd, STDIN_FILENO, STDOUT_FILENO};
use crate::simulation::Sim;

/// The terminal, taken. Dropping it puts it back.
#[derive(Debug)]
pub struct Terminal {
    _private: (),
}

impl Terminal {
    /// `enter_terminal`: raw mode on standard input, the saved attributes
    /// recorded before the raw flag is, so a signal can always restore them.
    pub fn enter() -> io::Result<Terminal> {
        platform::enter_terminal()?;
        Ok(Terminal { _private: () })
    }

    /// `enter_alt_screen`.
    pub fn enter_alt_screen(&mut self) {
        platform::enter_alt_screen();
    }

    /// Even a failed upload may have left some images behind.
    pub fn mark_sprites_uploaded(&mut self) {
        platform::mark_sprites_uploaded();
    }

    /// `restore_terminal`: once, whatever calls it.
    pub fn restore(&mut self) {
        platform::restore_terminal();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}

/// `write_all`: to standard output, retrying EINTR, silently giving up on any
/// other failure.
pub fn write_all(bytes: &[u8]) {
    platform::write_all_quietly(STDOUT_FILENO, bytes);
}

/// `terminal_query`: sends a request and collects the reply until a
/// terminator, a full buffer or the deadline. `reply_size` is the C buffer's
/// size, NUL included.
pub fn terminal_query(request: &[u8], reply_size: usize, milliseconds: i64) -> Vec<u8> {
    query_until(request, reply_size, milliseconds, None)
}

fn query_until(
    request: &[u8],
    reply_size: usize,
    milliseconds: i64,
    terminator: Option<u8>,
) -> Vec<u8> {
    let mut reply = Vec::new();
    if reply_size == 0 {
        return reply;
    }
    write_all(request);
    let start = platform::monotonic_now();
    let mut buffer = vec![0_u8; reply_size];
    loop {
        let now = platform::monotonic_now();
        let spent = (now.tv_sec - start.tv_sec) * 1000 + (now.tv_nsec - start.tv_nsec) / 1_000_000;
        if spent >= milliseconds {
            break;
        }
        let mut wait = [PollFd::new(STDIN_FILENO, platform::POLLIN)];
        match platform::poll(&mut wait, (milliseconds - spent) as i32) {
            Err(error) if error.raw_os_error() == Some(platform::EINTR) => continue,
            Err(_) | Ok(0) => break,
            Ok(_) => {}
        }
        let room = reply_size - 1 - reply.len();
        let got = match platform::read(STDIN_FILENO, &mut buffer[..room]) {
            Ok(got) if got > 0 => got,
            _ => break,
        };
        reply.extend_from_slice(&buffer[..got]);
        // Every reply this program asks for ends one of these three ways,
        // read as the C reads them: two by length, one as a C string.
        let as_c_string = match reply.iter().position(|&b| b == 0) {
            Some(end) => &reply[..end],
            None => &reply[..],
        };
        if terminator.is_some_and(|end| reply.contains(&end))
            || (terminator.is_none()
                && (reply.contains(&0x07)
                    || as_c_string.windows(2).any(|w| w == b"\x1b\\")
                    || reply.contains(&b'c')))
        {
            break;
        }
        if reply.len() + 1 >= reply_size {
            break;
        }
    }
    reply
}

/// Geometry and presentation requirements negotiated with a Sixel terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SixelTerminal {
    pub cell_size: (u16, u16),
    pub erase_before_frame: bool,
}

/// Only probe when Sixel is explicitly requested. The ordinary renderer's
/// startup traffic remains identical to the reference.
pub fn prepare_sixel() -> io::Result<SixelTerminal> {
    let capabilities = query_until(b"\x1b[c", 256, 250, Some(b'c'));
    if !has_sixel(&capabilities) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "terminal did not advertise Sixel; use Windows Terminal 1.22+ or --render braille",
        ));
    }
    let reply = query_until(b"\x1b[16t", 128, 250, Some(b't'));
    let cell = parse_cell_size(&reply);
    // Some Unix terminals (including iTerm2) advertise Sixel and provide
    // exact pixel dimensions through TIOCGWINSZ, but do not implement CSI 16 t.
    // Prefer the explicit reply: Windows Terminal can use virtual pixels that
    // differ from the console font metrics. Never guess from those metrics.
    #[cfg(unix)]
    let cell = cell.or_else(|| cell_size_from_window(platform::window_size_or_zero(STDOUT_FILENO)));
    let cell = cell.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "terminal did not report its graphics cell size (CSI 16 t)",
        )
    })?;
    // Ask the terminal itself, including through SSH, rather than relying on
    // TERM_PROGRAM from the local shell. Unimplemented XTVERSION is harmless.
    let erase_before_frame = query_is_iterm2();
    let mode = query_until(b"\x1b[?80$p", 128, 100, Some(b'y'));
    let was_enabled = mode.windows(9).any(|s| s == b"\x1b[?80;1$y")
        || mode.windows(9).any(|s| s == b"\x1b[?80;3$y");
    platform::enable_sixel_mode(was_enabled);
    Ok(SixelTerminal { cell_size: cell, erase_before_frame })
}

pub fn query_is_iterm2() -> bool {
    is_iterm2(&query_until(b"\x1b[>q", 128, 100, Some(b'\\')))
}

/// XTVERSION: DCS >| terminal-name version ST.
pub fn is_iterm2(reply: &[u8]) -> bool {
    const PREFIX: &[u8] = b"\x1bP>|iTerm2 ";
    reply
        .windows(PREFIX.len())
        .position(|s| s == PREFIX)
        .is_some_and(|start| reply[start + PREFIX.len()..].windows(2).any(|s| s == b"\x1b\\"))
}

/// Call only in the empty alternate screen. Some iTerm profiles make block
/// characters double-width, leaving gaps in the backdrop. A missing or
/// unexpected cursor report keeps the full-raster path. Hide and erase the
/// probe inside a synchronized update so it cannot flash before the intro.
pub fn sixel_backdrop_is_single_width() -> bool {
    let reply = query_until("\x1b[?2026h\x1b[H\x1b[8m█\x1b[6n".as_bytes(), 128, 100, Some(b'R'));
    write_all(b"\x1b[0m\x1b[2J\x1b[H\x1b[?2026l");
    reply == b"\x1b[1;2R"
}

pub fn has_sixel(reply: &[u8]) -> bool {
    reply
        .windows(3)
        .position(|s| s == b"\x1b[?")
        .and_then(|at| {
            let body = &reply[at + 3..];
            body.iter()
                .position(|&b| b == b'c')
                .map(|end| body[..end].split(|&b| b == b';').skip(1).any(|s| s == b"4"))
        })
        .unwrap_or(false)
}

/// CSI 6 ; cell-height ; cell-width t. These may be virtual pixels, as in WT.
pub fn parse_cell_size(reply: &[u8]) -> Option<(u16, u16)> {
    let at = reply.windows(4).position(|s| s == b"\x1b[6;")?;
    let body = &reply[at + 4..];
    let end = body.iter().position(|&b| b == b't')?;
    let text = std::str::from_utf8(&body[..end]).ok()?;
    let (height, width) = text.split_once(';')?;
    let (width, height) = (width.parse::<u16>().ok()?, height.parse::<u16>().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// Accept native pixel dimensions only when they describe whole, nonzero cells.
pub fn cell_size_from_window(size: platform::WinSize) -> Option<(u16, u16)> {
    if size.col == 0
        || size.row == 0
        || !size.xpixel.is_multiple_of(size.col)
        || !size.ypixel.is_multiple_of(size.row)
    {
        return None;
    }
    let cell = (size.xpixel / size.col, size.ypixel / size.row);
    (cell.0 > 0 && cell.1 > 0).then_some(cell)
}

pub fn graphics_window(mut size: platform::WinSize, cell: Option<(u16, u16)>) -> platform::WinSize {
    if let Some((width, height)) = cell {
        size.xpixel = size.col.saturating_mul(width);
        size.ypixel = size.row.saturating_mul(height);
    }
    size
}

/// `ask_colour`.
pub fn ask_colour(request: &[u8]) -> Option<Rgb> {
    let reply = terminal_query(request, 128, 60);
    if reply.is_empty() {
        return None;
    }
    palette::parse_osc_colour(&reply)
}

/// `learn_the_theme`: the background first, then entries one to six.
pub fn learn_the_theme(theme: &mut Theme) -> bool {
    // No background: fade towards black, which is the common case.
    let background = ask_colour(b"\x1b]11;?\x1b\\").unwrap_or([0, 0, 0]);
    let mut answers = [None; 6];
    for (k, answer) in answers.iter_mut().enumerate() {
        let request = format!("\x1b]4;{};?\x1b\\", k + 1);
        *answer = ask_colour(request.as_bytes());
    }
    palette::learn_from_answers(theme, background, &answers)
}

/// `update_screen_dimensions`: what the terminal says, zero where it will not.
pub fn update_screen_dimensions(sim: &mut Sim) {
    apply_window_size(sim, platform::window_size_or_zero(STDOUT_FILENO));
}

/// The derivation half of `update_screen_dimensions`, for a size already read.
pub fn apply_window_size(sim: &mut Sim, size: platform::WinSize) {
    sim.apply_screen_size(
        i32::from(size.col),
        i32::from(size.row),
        i32::from(size.xpixel),
        i32::from(size.ypixel),
    );
}

/// `wait_for_terminal_io`: until a key can be read or output written.
pub fn wait_for_terminal_io() -> io::Result<()> {
    let mut descriptors = [
        PollFd::new(STDIN_FILENO, platform::POLLIN),
        PollFd::new(STDOUT_FILENO, platform::POLLOUT),
    ];
    loop {
        match platform::poll(&mut descriptors, -1) {
            Err(error) if error.raw_os_error() == Some(platform::EINTR) => continue,
            Err(error) => return Err(error),
            Ok(_) => return Ok(()),
        }
    }
}
