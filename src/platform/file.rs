//! Writing a whole file as `fopen`/`fwrite`/`fclose` do, with the failure of
//! the final `close` reported: a full or networked disk may only say so then,
//! and `std::fs::File` discards that error on drop.

use std::ffi::{OsStr, c_int};
use std::io::{self, Write};
use std::os::fd::{IntoRawFd, OwnedFd};

unsafe extern "C" {
    fn close(fd: c_int) -> c_int;
}

/// `fopen(path, "wb")`, `fwrite` of all of `data`, `fclose`: created with
/// mode 0666 less the umask, truncated, and any failure — open, write or
/// close — returned.
pub fn write_file(path: &OsStr, data: &[u8]) -> io::Result<()> {
    let mut file =
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
    let written = file.write_all(data);
    let fd = OwnedFd::from(file).into_raw_fd();
    // SAFETY: `fd` was just released from an owned descriptor, so this is the
    // one and only close of it.
    let closed = unsafe { close(fd) };
    written?;
    if closed < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_file_round_trips_and_failures_are_reported() {
        let dir = std::env::temp_dir().join(format!("rbirds-write-file.{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.bin");
        write_file(path.as_os_str(), b"birds").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"birds");
        assert!(write_file(dir.join("missing/x").as_os_str(), b"x").is_err());
        if std::path::Path::new("/dev/full").exists() {
            assert!(write_file(OsStr::new("/dev/full"), &[0; 8192]).is_err());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
