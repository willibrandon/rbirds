//! Error text and immediate exit: `strerror`, `perror`'s format, `_exit`.

use super::{ERANGE, errno, sys};
use std::ffi::c_int;

/// The C library's message for `errnum`, exactly what `strerror` gives in the
/// "C" locale (neither program calls `setlocale`): for example
/// `Inappropriate ioctl for device`. Unknown numbers get the library's own
/// "Unknown error" text. Uses the thread-safe XSI `strerror_r` (glibc:
/// `__xpg_strerror_r`). Allocates, so it is not for signal handlers.
pub fn strerror(errnum: c_int) -> Vec<u8> {
    let mut capacity = 128;
    loop {
        let mut buffer = vec![0u8; capacity];
        // SAFETY: `buffer` is live and writable for `capacity` bytes, the
        // length passed; strerror_r writes at most that many bytes, including
        // the terminating NUL, and keeps no pointer.
        let result = unsafe { sys::strerror_r(errnum, buffer.as_mut_ptr().cast(), capacity) };
        // Both libraries return the error number directly; Darwin can also
        // return -1 with errno set.
        let too_small = result == ERANGE || (result == -1 && errno() == ERANGE);
        if too_small && capacity < 1 << 16 {
            capacity *= 4;
            continue;
        }
        // Success, or EINVAL for an unknown number, which still fills in the
        // "Unknown error" text strerror would return.
        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        buffer.truncate(end);
        return buffer;
    }
}

/// Exactly what `perror(prefix)` writes for `errnum`: `"<prefix>: <message>\n"`,
/// or `"<message>\n"` for an empty prefix, as both C libraries format it.
pub fn perror_message(prefix: &[u8], errnum: c_int) -> Vec<u8> {
    let message = strerror(errnum);
    let mut out = Vec::with_capacity(prefix.len() + message.len() + 3);
    if !prefix.is_empty() {
        out.extend_from_slice(prefix);
        out.extend_from_slice(b": ");
    }
    out.extend_from_slice(&message);
    out.push(b'\n');
    out
}

/// `_exit(code)`: ends the process at once, without atexit handlers, stdio
/// flushing or Rust destructors. Async-signal-safe; used by the signal handler.
pub fn exit_immediately(code: c_int) -> ! {
    // SAFETY: _exit takes an int by value, touches no memory of ours and does
    // not return.
    unsafe { sys::_exit(code) }
}
