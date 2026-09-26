//! The C program's standard streams, as `stdio` behaves: `stdout` is line
//! buffered on a terminal and fully buffered otherwise, and whatever is left
//! is written at exit; `stderr` is unbuffered, one write per message.
//!
//! The difference is observable when both streams reach one file
//! (`2>&1 > log`): the C writes a recording's diagnostic before its summary
//! there. Rust's own `stdout` is always line buffered, so it is not used.

#![forbid(unsafe_code)]

use crate::platform;

/// `stdout` as the C sees it.
#[derive(Debug)]
pub struct CStdout {
    buffer: Vec<u8>,
    line_buffered: bool,
    failed: bool,
}

impl CStdout {
    /// Buffered according to whether descriptor 1 is a terminal, as stdio
    /// decides on first use.
    pub fn new() -> CStdout {
        CStdout {
            buffer: Vec::new(),
            line_buffered: platform::isatty(platform::STDOUT_FILENO),
            failed: false,
        }
    }

    /// A stream that holds everything until [`CStdout::take`], for tests.
    pub fn captured() -> CStdout {
        CStdout { buffer: Vec::new(), line_buffered: false, failed: false }
    }

    /// `fputs`/`printf`: appended, and flushed now if line buffered and the
    /// text holds a newline.
    pub fn print(&mut self, text: &[u8]) {
        self.buffer.extend_from_slice(text);
        if self.line_buffered && text.contains(&b'\n') {
            self.flush();
        }
    }

    /// Writes what is buffered. A failure is remembered and never reported,
    /// as `exit` never reports one.
    pub fn flush(&mut self) {
        if !self.failed && !self.buffer.is_empty() {
            self.failed = platform::write_all(platform::STDOUT_FILENO, &self.buffer).is_err();
        }
        self.buffer.clear();
    }

    /// What has been printed and not yet written.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }
}

impl Default for CStdout {
    fn default() -> CStdout {
        CStdout::new()
    }
}

/// `fprintf(stderr, ...)`: one unbuffered write, errors ignored.
pub fn eprint(text: &[u8]) {
    let _ = platform::write_all(platform::STDERR_FILENO, text);
}

/// Concatenates byte pieces, for messages that splice in raw path bytes.
pub fn cat(pieces: &[&[u8]]) -> Vec<u8> {
    let mut text = Vec::with_capacity(pieces.iter().map(|p| p.len()).sum());
    for piece in pieces {
        text.extend_from_slice(piece);
    }
    text
}
