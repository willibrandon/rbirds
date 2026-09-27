//! The Kitty graphics protocol and the one pending-output buffer every renderer
//! writes through, translated from cbirds `kitty_graphics.c`.
//!
//! Commands are queued into a buffer and written by [`KittyGraphics::flush`]
//! or [`KittyGraphics::flush_nonblocking`]; written bytes are removed and an
//! unsent suffix is kept, so a frame is never discarded, repeated or reordered
//! under backpressure. PNG data is Base64 encoded and split into
//! protocol-sized chunks internally.

#![forbid(unsafe_code)]

use crate::platform::RawFd;
use std::fmt;
use std::io;

use crate::platform;

/// `KITTY_GRAPHICS_PAYLOAD_MAX`: Base64 bytes a chunk.
pub const PAYLOAD_MAX: usize = 4096;

const PNG_FORMAT: i32 = 100;
/// 3072 input bytes encode to exactly 4096 Base64 bytes.
const RAW_CHUNK_MAX: usize = PAYLOAD_MAX * 3 / 4;
const INITIAL_CAPACITY: usize = 4096;

const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `kitty_graphics_status_t` without `KITTY_GRAPHICS_OK`, which is `Ok(..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KittyError {
    Argument,
    Memory,
    /// `KITTY_GRAPHICS_ERR_IO`, with the `errno` the C leaves behind for it
    /// (the thread's `errno` is left the same way).
    Io(i32),
    /// `KITTY_GRAPHICS_AGAIN`: a nonblocking flush found no room; the unsent
    /// suffix is still queued.
    Again,
}

impl KittyError {
    /// `kitty_graphics_status_string` for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            KittyError::Argument => "invalid argument",
            KittyError::Memory => "out of memory",
            KittyError::Io(_) => "output error",
            KittyError::Again => "output would block",
        }
    }
}

impl fmt::Display for KittyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for KittyError {}

/// `kitty_graphics_status_string` over a whole status, `KITTY_GRAPHICS_OK`
/// included.
pub fn kitty_graphics_status_string(status: Result<(), KittyError>) -> &'static str {
    match status {
        Ok(()) => "ok",
        Err(error) => error.as_str(),
    }
}

/// `kitty_graphics_placement_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Placement {
    pub image_id: u32,
    pub placement_id: u32,
    /// Zero-based terminal row.
    pub row: i32,
    /// Zero-based terminal column.
    pub column: i32,
    /// Pixel offset within the terminal cell.
    pub x_offset: i32,
    pub y_offset: i32,
    pub z_index: i32,
}

/// `kitty_graphics_t`. The context does not own `output_fd` and never closes
/// it.
#[derive(Debug)]
pub struct KittyGraphics {
    output_fd: RawFd,
    buffer: Vec<u8>,
    /// The C `capacity`, NUL slot included, which decides when `buffer` grows.
    capacity: usize,
}

/// One `snprintf` of a command with numeric fields: every one built here fits
/// in 128 bytes.
struct Line {
    bytes: [u8; 128],
    length: usize,
}

impl Line {
    fn new() -> Line {
        Line { bytes: [0; 128], length: 0 }
    }

    fn text(&mut self, text: &[u8]) -> &mut Line {
        self.bytes[self.length..self.length + text.len()].copy_from_slice(text);
        self.length += text.len();
        self
    }

    /// `%u` / `PRIu32`.
    fn uint(&mut self, value: u32) -> &mut Line {
        let mut digits = [0u8; 10];
        let mut at = digits.len();
        let mut rest = value;
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        self.text(&digits[at..])
    }

    /// `%d`.
    fn int(&mut self, value: i32) -> &mut Line {
        if value < 0 {
            self.text(b"-");
        }
        self.uint(value.unsigned_abs())
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

fn base64_encode_chunk(input: &[u8], output: &mut [u8; PAYLOAD_MAX]) -> usize {
    let length = input.len();
    let (mut i, mut out) = (0, 0);
    while i < length {
        let a = u32::from(input[i]);
        i += 1;
        let b = if i < length {
            i += 1;
            u32::from(input[i - 1])
        } else {
            0
        };
        let c = if i < length {
            i += 1;
            u32::from(input[i - 1])
        } else {
            0
        };
        let triple = (a << 16) | (b << 8) | c;
        output[out] = BASE64_CHARS[((triple >> 18) & 63) as usize];
        output[out + 1] = BASE64_CHARS[((triple >> 12) & 63) as usize];
        output[out + 2] = BASE64_CHARS[((triple >> 6) & 63) as usize];
        output[out + 3] = BASE64_CHARS[(triple & 63) as usize];
        out += 4;
    }
    if length % 3 == 1 {
        output[out - 2] = b'=';
    }
    if !length.is_multiple_of(3) {
        output[out - 1] = b'=';
    }
    out
}

/// The `errno` an OS call left, as the C reads it.
fn os_errno(error: &io::Error) -> i32 {
    error.raw_os_error().unwrap_or_else(platform::errno)
}

impl KittyGraphics {
    /// `kitty_graphics_init`: an empty queue for `output_fd`, which must not be
    /// negative.
    pub fn new(output_fd: RawFd) -> Result<KittyGraphics, KittyError> {
        if output_fd < 0 {
            return Err(KittyError::Argument);
        }
        Ok(KittyGraphics { output_fd, buffer: Vec::new(), capacity: 0 })
    }

    /// The descriptor commands are written to.
    pub fn output_fd(&self) -> RawFd {
        self.output_fd
    }

    /// Queued bytes (the C `length`).
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Forgets everything queued, keeping the storage (`graphics.length = 0`).
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// The queued bytes.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// The C `capacity`, NUL slot included: 0 until something is queued, then
    /// 4096 doubling as needed.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Every call checks the descriptor as the C does, although `new` never
    /// accepts a negative one.
    fn unusable(&self) -> bool {
        self.output_fd < 0
    }

    fn reserve(&mut self, extra: usize) -> Result<(), KittyError> {
        let length = self.buffer.len();
        if length == usize::MAX || extra > usize::MAX - length - 1 {
            return Err(KittyError::Memory);
        }
        let needed = length + extra + 1;
        if needed <= self.capacity {
            return Ok(());
        }
        let mut capacity = if self.capacity != 0 { self.capacity } else { INITIAL_CAPACITY };
        while capacity < needed {
            if capacity > usize::MAX / 2 {
                capacity = needed;
                break;
            }
            capacity *= 2;
        }
        self.buffer.try_reserve_exact(capacity - length).map_err(|_| KittyError::Memory)?;
        self.capacity = capacity;
        Ok(())
    }

    fn append_bytes(&mut self, data: &[u8]) -> Result<(), KittyError> {
        self.reserve(data.len())?;
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// `append_format` for a command whose formatted length is already known
    /// to fit an `int`: one reservation of exactly its length, then the bytes.
    fn append_line(&mut self, line: &Line) -> Result<(), KittyError> {
        self.append_bytes(line.as_bytes())
    }

    /// `kitty_graphics_upload_png`: transmits `png` as image `image_id`,
    /// Base64 encoded in chunks of at most [`PAYLOAD_MAX`] bytes. On failure
    /// nothing of the upload stays queued.
    pub fn upload_png(&mut self, image_id: u32, png: &[u8]) -> Result<(), KittyError> {
        if self.unusable() || image_id == 0 || png.is_empty() {
            return Err(KittyError::Argument);
        }

        let original_length = self.buffer.len();
        let mut offset = 0;
        let mut first = true;
        while offset < png.len() {
            let raw_length = (png.len() - offset).min(RAW_CHUNK_MAX);
            let more = i32::from(offset + raw_length < png.len());

            let mut payload = [0u8; PAYLOAD_MAX];
            let payload_length =
                base64_encode_chunk(&png[offset..offset + raw_length], &mut payload);
            let mut line = Line::new();
            if first {
                line.text(b"\x1b_Ga=t,q=2,f=").int(PNG_FORMAT).text(b",I=").uint(image_id);
                line.text(b",m=").int(more).text(b";");
                first = false;
            } else {
                line.text(b"\x1b_Gm=").int(more).text(b",q=2;");
            }
            let mut status = self.append_line(&line);
            if status.is_ok() {
                status = self.append_bytes(&payload[..payload_length]);
            }
            if status.is_ok() {
                status = self.append_bytes(b"\x1b\\");
            }
            if let Err(error) = status {
                self.buffer.truncate(original_length);
                return Err(error);
            }
            offset += raw_length;
        }
        Ok(())
    }

    /// `kitty_graphics_place`: puts image `image_id` at a zero-based cell and
    /// pixel offset. Negative positions or offsets are refused; the placement
    /// id and z index are sent only when not zero. The one-based cursor
    /// position wraps as the canonical C build's `int` addition does.
    pub fn place(&mut self, placement: &Placement) -> Result<(), KittyError> {
        if self.unusable()
            || placement.image_id == 0
            || placement.row < 0
            || placement.column < 0
            || placement.x_offset < 0
            || placement.y_offset < 0
        {
            return Err(KittyError::Argument);
        }

        let mut line = Line::new();
        line.text(b"\x1b[").int(placement.row.wrapping_add(1));
        line.text(b";").int(placement.column.wrapping_add(1));
        line.text(b"H\x1b_Ga=p,I=").uint(placement.image_id).text(b",q=2");
        if placement.placement_id != 0 {
            line.text(b",p=").uint(placement.placement_id);
        }
        line.text(b",X=").int(placement.x_offset).text(b",Y=").int(placement.y_offset);
        if placement.z_index != 0 {
            line.text(b",z=").int(placement.z_index);
        }
        line.text(b",C=1\x1b\\");
        self.append_line(&line)
    }

    pub(crate) fn place_region(
        &mut self,
        placement: &Placement,
        region: [i32; 4],
    ) -> Result<(), KittyError> {
        let original = self.buffer.len();
        self.place(placement)?;
        self.buffer.truncate(self.buffer.len() - 2);
        let mut line = Line::new();
        if region[0] != 0 {
            line.text(b",x=").int(region[0]);
        }
        if region[1] != 0 {
            line.text(b",y=").int(region[1]);
        }
        if region[2] != 0 {
            line.text(b",w=").int(region[2]);
        }
        line.text(b",h=").int(region[3]).text(b"\x1b\\");
        if let Err(error) = self.append_line(&line) {
            self.buffer.truncate(original);
            return Err(error);
        }
        Ok(())
    }

    /// `kitty_graphics_delete_placement`.
    pub fn delete_placement(&mut self, image_id: u32, placement_id: u32) -> Result<(), KittyError> {
        if self.unusable() || image_id == 0 || placement_id == 0 {
            return Err(KittyError::Argument);
        }
        let mut line = Line::new();
        line.text(b"\x1b_Ga=d,d=n,I=").uint(image_id).text(b",p=").uint(placement_id);
        line.text(b",q=2\x1b\\");
        self.append_line(&line)
    }

    /// `kitty_graphics_delete_all_placements`.
    pub fn delete_all_placements(&mut self) -> Result<(), KittyError> {
        if self.unusable() {
            return Err(KittyError::Argument);
        }
        self.append_bytes(b"\x1b_Ga=d,d=a\x1b\\")
    }

    /// `kitty_graphics_delete_image`: `N` selects the newest image with the
    /// supplied image number and frees its data.
    pub fn delete_image(&mut self, image_id: u32) -> Result<(), KittyError> {
        if self.unusable() || image_id == 0 {
            return Err(KittyError::Argument);
        }
        let mut line = Line::new();
        line.text(b"\x1b_Ga=d,d=N,I=").uint(image_id).text(b"\x1b\\");
        self.append_line(&line)
    }

    /// `kitty_graphics_write_text`: queues terminal text at a zero-based cell
    /// position. It shares the buffer with the graphics commands, so it reaches
    /// the screen inside the current synchronized update and through the same
    /// flow-controlled flush. Any escape sequence the caller needs travels
    /// inside `text`.
    ///
    /// `text` is the C string: it ends at its first NUL byte, if any, as the
    /// C's `%s` does, and a result longer than `INT_MAX` is refused as the C
    /// `vsnprintf` refuses it.
    pub fn write_text(&mut self, row: i32, column: i32, text: &[u8]) -> Result<(), KittyError> {
        if self.unusable() || row < 0 || column < 0 {
            return Err(KittyError::Argument);
        }
        let text = match text.iter().position(|&byte| byte == 0) {
            Some(end) => &text[..end],
            None => text,
        };
        let mut line = Line::new();
        line.text(b"\x1b[").int(row.wrapping_add(1)).text(b";").int(column.wrapping_add(1));
        line.text(b"H");
        let length = line.length + text.len();
        if length > i32::MAX as usize {
            return Err(KittyError::Argument);
        }
        self.reserve(length)?;
        self.buffer.extend_from_slice(line.as_bytes());
        self.buffer.extend_from_slice(text);
        Ok(())
    }

    /// `kitty_graphics_write_raw`: queues bytes as they are, such as escape
    /// text some other renderer has already built. The buffer is the one
    /// output sink, whatever is being drawn with.
    pub fn write_raw(&mut self, bytes: &[u8]) -> Result<(), KittyError> {
        if self.unusable() {
            return Err(KittyError::Argument);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        self.append_bytes(bytes)
    }

    /// Opens a frame with DEC synchronized-update mode.
    pub fn begin_synchronized_update(&mut self) -> Result<(), KittyError> {
        if self.unusable() {
            return Err(KittyError::Argument);
        }
        self.append_bytes(b"\x1b[?2026h")
    }

    /// Closes a frame opened by [`KittyGraphics::begin_synchronized_update`].
    pub fn end_synchronized_update(&mut self) -> Result<(), KittyError> {
        if self.unusable() {
            return Err(KittyError::Argument);
        }
        self.append_bytes(b"\x1b[?2026l")
    }

    fn discard_written_prefix(&mut self, written: usize) {
        if written == 0 {
            return;
        }
        self.buffer.drain(..written);
    }

    fn flush_buffer(&mut self, nonblocking: bool) -> Result<(), KittyError> {
        if self.unusable() {
            return Err(KittyError::Argument);
        }

        let mut written = 0;
        while written < self.buffer.len() {
            #[cfg(not(windows))]
            let result = platform::write(self.output_fd, &self.buffer[written..]);
            #[cfg(windows)]
            let result = if nonblocking {
                platform::write_nonblocking(self.output_fd, &self.buffer[written..])
            } else {
                platform::write(self.output_fd, &self.buffer[written..])
            };
            match result {
                Err(error) => {
                    let code = os_errno(&error);
                    if code == platform::EINTR {
                        continue;
                    }
                    self.discard_written_prefix(written);
                    if nonblocking && (code == platform::EAGAIN || code == platform::EWOULDBLOCK) {
                        return Err(KittyError::Again);
                    }
                    return Err(KittyError::Io(code));
                }
                Ok(0) => {
                    platform::set_errno(platform::EIO);
                    self.discard_written_prefix(written);
                    return Err(KittyError::Io(platform::EIO));
                }
                Ok(count) => written += count,
            }
        }
        self.buffer.clear();
        Ok(())
    }

    /// `kitty_graphics_flush`: writes everything queued, waiting for the
    /// descriptor as it is. Written bytes are removed; on an error the unsent
    /// suffix stays queued.
    pub fn flush(&mut self) -> Result<(), KittyError> {
        self.flush_buffer(false)
    }

    /// `kitty_graphics_flush_nonblocking`: writes without waiting for output
    /// capacity and preserves any unsent suffix, returning
    /// [`KittyError::Again`] when the descriptor is full. `O_NONBLOCK` is set
    /// for the writes only if it was clear, then restored, and `errno` is left
    /// as the writes left it, as in the C.
    pub fn flush_nonblocking(&mut self) -> Result<(), KittyError> {
        if self.unusable() {
            return Err(KittyError::Argument);
        }

        #[cfg(windows)]
        return self.flush_buffer(true);

        #[cfg(not(windows))]
        {
            let flags = platform::status_flags(self.output_fd)
                .map_err(|error| KittyError::Io(os_errno(&error)))?;
            let changed_flags = flags & platform::O_NONBLOCK == 0;
            if changed_flags {
                platform::set_status_flags(self.output_fd, flags | platform::O_NONBLOCK)
                    .map_err(|error| KittyError::Io(os_errno(&error)))?;
            }

            let status = self.flush_buffer(true);
            let write_errno = platform::errno();
            if changed_flags {
                platform::set_status_flags(self.output_fd, flags)
                    .map_err(|error| KittyError::Io(os_errno(&error)))?;
            }
            platform::set_errno(write_errno);
            status
        }
    }
}
