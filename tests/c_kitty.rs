//! The cbirds `tests/kitty_graphics_test.c` suite, translated test for test:
//! every `test_*` its `main` calls, with the same fixtures, assertions and
//! bounds. The C's pipes are `std::io::pipe()`; its `fcntl` calls go through
//! `rbirds::platform`.

#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::fd::AsRawFd;

use rbirds::platform;
use rbirds::render::kitty::{KittyError, KittyGraphics, PAYLOAD_MAX, Placement};

#[test]
fn test_small_upload() {
    let png = [0x00, 0xff, 0x10];
    let expected = b"\x1b_Ga=t,q=2,f=100,I=7,m=0;AP8Q\x1b\\";

    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).unwrap();
    assert_eq!(graphics.upload_png(7, &png), Ok(()));
    assert_eq!(graphics.len(), expected.len());
    assert_eq!(graphics.buffer(), expected);
}

#[test]
fn test_chunked_upload() {
    let input_length = PAYLOAD_MAX * 3 / 4 + 1;
    let png = vec![0u8; input_length];
    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).unwrap();
    assert_eq!(graphics.upload_png(42, &png), Ok(()));

    let first_prefix = b"\x1b_Ga=t,q=2,f=100,I=42,m=1;";
    let last_prefix = b"\x1b_Gm=0,q=2;";
    let buffer = graphics.buffer();
    assert!(buffer.starts_with(first_prefix));

    let first_payload = &buffer[first_prefix.len()..];
    let first_end = first_payload.windows(2).position(|w| w == b"\x1b\\").expect("chunk end");
    assert_eq!(first_end, PAYLOAD_MAX);
    assert!(first_payload[..first_end].iter().all(|&byte| byte == b'A'));

    let last = &first_payload[first_end + 2..];
    assert!(last.starts_with(last_prefix));
    let last = &last[last_prefix.len()..];
    // "AA==" then the terminator, and that is the end of the buffer.
    assert_eq!(last, b"AA==\x1b\\");
}

#[test]
fn test_placement_and_deletion() {
    let expected: &[u8] = b"\x1b_Ga=d,d=a\x1b\\\
        \x1b[3;5H\x1b_Ga=p,I=9,q=2,p=12,X=3,Y=6,z=-1,C=1\x1b\\\
        \x1b[3;5H\x1b_Ga=p,I=9,q=2,X=3,Y=6,C=1\x1b\\\
        \x1b_Ga=d,d=n,I=9,p=12,q=2\x1b\\\
        \x1b_Ga=d,d=N,I=9\x1b\\";
    let mut placement = Placement {
        image_id: 9,
        placement_id: 12,
        row: 2,
        column: 4,
        x_offset: 3,
        y_offset: 6,
        z_index: -1,
    };

    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).unwrap();
    assert_eq!(graphics.delete_all_placements(), Ok(()));
    assert_eq!(graphics.place(&placement), Ok(()));
    placement.placement_id = 0;
    placement.z_index = 0;
    assert_eq!(graphics.place(&placement), Ok(()));
    assert_eq!(graphics.delete_placement(9, 12), Ok(()));
    assert_eq!(graphics.delete_image(9), Ok(()));
    assert_eq!(graphics.len(), expected.len());
    assert_eq!(graphics.buffer(), expected);
}

#[test]
fn test_synchronized_update() {
    let expected = b"\x1b[?2026h\x1b[?2026l";
    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).unwrap();
    assert_eq!(graphics.begin_synchronized_update(), Ok(()));
    assert_eq!(graphics.end_synchronized_update(), Ok(()));
    assert_eq!(graphics.len(), expected.len());
    assert_eq!(graphics.buffer(), expected);
}

#[test]
fn test_write_text() {
    let expected = b"\x1b[24;1Hbar\x1b[12;5H\x1b[7mx\x1b[0m";
    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).unwrap();
    // Rows and columns are zero based on the way in, one based on the wire.
    assert_eq!(graphics.write_text(23, 0, b"bar"), Ok(()));
    // Escape sequences ride inside the text untouched.
    assert_eq!(graphics.write_text(11, 4, b"\x1b[7mx\x1b[0m"), Ok(()));
    assert_eq!(graphics.len(), expected.len());
    assert_eq!(graphics.buffer(), expected);
}

#[cfg(unix)]
#[test]
fn test_flush() {
    let png = [0u8];
    let mut output = [0u8; 128];
    let (mut reader, writer) = std::io::pipe().expect("pipe");

    let mut graphics = KittyGraphics::new(writer.as_raw_fd()).unwrap();
    assert_eq!(graphics.upload_png(1, &png), Ok(()));
    let expected_length = graphics.len();
    assert_eq!(graphics.flush(), Ok(()));
    assert_eq!(graphics.len(), 0);
    assert_eq!(reader.read(&mut output).expect("read"), expected_length);
    drop(graphics);
    drop(writer);
}

#[cfg(unix)]
#[test]
fn test_nonblocking_flush_backpressure() {
    let fill = [0u8; 4096];
    let mut drain = [0u8; 8192];
    let png = [0u8];
    let (mut reader, writer) = std::io::pipe().expect("pipe");
    let fd = writer.as_raw_fd();

    let flags = platform::status_flags(fd).expect("F_GETFL");
    assert!(flags >= 0);
    platform::set_status_flags(fd, flags | platform::O_NONBLOCK).expect("F_SETFL");
    let refusal = loop {
        match platform::write(fd, &fill) {
            Ok(written) if written > 0 => {}
            Ok(_) => panic!("write returned 0"),
            Err(error) => break error,
        }
    };
    let code = refusal.raw_os_error();
    assert!(code == Some(platform::EAGAIN) || code == Some(platform::EWOULDBLOCK));
    platform::set_status_flags(fd, flags).expect("F_SETFL");
    // Read again, now that the pipe has been written to: macOS reports private
    // status bits in F_GETFL (a descriptor that was written says so), which no
    // F_SETFL can clear, so the flags before the first write are not the ones
    // to compare with. What the flush owes the caller is O_NONBLOCK as it was.
    let flags = platform::status_flags(fd).expect("F_GETFL");
    assert!(flags >= 0 && flags & platform::O_NONBLOCK == 0);

    let mut graphics = KittyGraphics::new(fd).unwrap();
    assert_eq!(graphics.upload_png(1, &png), Ok(()));
    let expected_length = graphics.len();
    assert_eq!(graphics.flush_nonblocking(), Err(KittyError::Again));
    assert_eq!(graphics.len(), expected_length);
    assert_eq!(platform::status_flags(fd).expect("F_GETFL"), flags);
    assert_eq!(platform::status_flags(fd).expect("F_GETFL") & platform::O_NONBLOCK, 0);

    assert!(reader.read(&mut drain).expect("read") > 0);
    assert_eq!(graphics.flush_nonblocking(), Ok(()));
    assert_eq!(graphics.len(), 0);
    assert_eq!(platform::status_flags(fd).expect("F_GETFL"), flags);
    drop(graphics);
    drop(writer);
}

/// The C's NULL-context calls (`kitty_graphics_init(NULL, ..)`,
/// `kitty_graphics_write_text(NULL, ..)`) and its NULL text
/// (`kitty_graphics_write_text(&graphics, 0, 0, NULL)`) have no Rust
/// counterpart: a receiver or slice cannot be null. Every other refusal is
/// kept.
#[test]
fn test_invalid_arguments() {
    let placement = Placement::default();
    assert_eq!(KittyGraphics::new(-1).err(), Some(KittyError::Argument));
    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).unwrap();
    assert_eq!(graphics.upload_png(0, &[]), Err(KittyError::Argument));
    assert_eq!(graphics.place(&placement), Err(KittyError::Argument));
    assert_eq!(graphics.delete_placement(0, 0), Err(KittyError::Argument));
    assert_eq!(graphics.write_text(-1, 0, b"x"), Err(KittyError::Argument));
    assert_eq!(graphics.write_text(0, -1, b"x"), Err(KittyError::Argument));
    // Nothing refused was queued.
    assert!(graphics.is_empty());
}
