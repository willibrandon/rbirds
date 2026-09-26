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

pub const ALT_SCREEN_ON: &[u8] = b"\x1b[?1049h";
pub const CURSOR_HIDE: &[u8] = b"\x1b[?25l";
/// Any event tracking plus SGR coordinates.
pub const MOUSE_REPORT_ON: &[u8] = b"\x1b[?1003h\x1b[?1006h";

/// The terminal, taken. Dropping it puts it back.
#[derive(Debug)]
pub struct Terminal {
    _private: (),
}

impl Terminal {
    /// `enter_terminal`: raw mode on standard input, the saved attributes
    /// recorded before the raw flag is, so a signal can always restore them.
    pub fn enter() -> io::Result<Terminal> {
        let saved = platform::tcgetattr(STDIN_FILENO)?;
        let mut raw = saved;
        raw.make_raw_as_cbirds();
        platform::emergency::record_saved_termios(&saved);
        platform::tcsetattr(STDIN_FILENO, &raw)?;
        platform::emergency::mark_raw_acquired();
        Ok(Terminal { _private: () })
    }

    /// `enter_alt_screen`.
    pub fn enter_alt_screen(&mut self) {
        write_all(ALT_SCREEN_ON);
        write_all(CURSOR_HIDE);
        write_all(MOUSE_REPORT_ON);
        platform::emergency::mark_alt_screen_on();
    }

    /// Even a failed upload may have left some images behind.
    pub fn mark_sprites_uploaded(&mut self) {
        platform::emergency::mark_sprites_uploaded();
    }

    /// `restore_terminal`: once, whatever calls it.
    pub fn restore(&mut self) {
        platform::emergency::restore_terminal();
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
    let _ = platform::write_all(STDOUT_FILENO, bytes);
}

/// `terminal_query`: sends a request and collects the reply until a
/// terminator, a full buffer or the deadline. `reply_size` is the C buffer's
/// size, NUL included.
pub fn terminal_query(request: &[u8], reply_size: usize, milliseconds: i64) -> Vec<u8> {
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
        if reply.contains(&0x07)
            || as_c_string.windows(2).any(|w| w == b"\x1b\\")
            || reply.contains(&b'c')
        {
            break;
        }
        if reply.len() + 1 >= reply_size {
            break;
        }
    }
    reply
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
    apply_window_size(sim, platform::window_size(STDOUT_FILENO).unwrap_or_default());
}

/// The derivation half of `update_screen_dimensions`, for a size already read.
pub fn apply_window_size(sim: &mut Sim, size: platform::WinSize) {
    sim.apply_screen_size(
        i32::from(size.ws_col),
        i32::from(size.ws_row),
        i32::from(size.ws_xpixel),
        i32::from(size.ws_ypixel),
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
