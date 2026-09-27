//! Real console API tests run in a hidden child console. No test changes the
//! user's console modes or races another test's process-global terminal state.
use super::*;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetStdHandle(which: u32, handle: Handle) -> i32;
    fn WriteConsoleInputW(
        handle: Handle,
        records: *const InputRecord,
        count: u32,
        written: *mut u32,
    ) -> i32;
    fn SetConsoleScreenBufferSize(handle: Handle, size: Coord) -> i32;
    fn SetConsoleWindowInfo(handle: Handle, absolute: i32, rect: *const Rect) -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
    fn ReadConsoleOutputCharacterW(
        handle: Handle,
        text: *mut u16,
        count: u32,
        at: Coord,
        read: *mut u32,
    ) -> i32;
}

fn child(case: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "platform::tests::hidden_console_child", "--ignored", "--nocapture"])
        .env("RBIRDS_CONSOLE_TEST", case)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW: new, invisible console.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            panic!("hidden console test {case} timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{case}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn native_console_frames_and_unicode_snapshot_restore_modes() {
    child("frames");
}
#[test]
fn native_console_input_resize_and_panic_restore_modes() {
    child("input");
}
#[test]
fn native_console_ctrl_break_exits_and_restores_modes() {
    child("control");
}
#[test]
fn native_output_backpressure_preserves_bytes_and_services_input() {
    child("backpressure");
}
#[test]
fn native_blocked_output_can_be_cancelled() {
    child("cancel-write");
}

#[test]
fn native_sixel_queries_read_console_replies_and_reject_unsupported_terminals() {
    child("sixel");
    child("sixel-unsupported");
}

#[test]
fn native_text_output_preserves_unicode_without_changing_codepage() {
    child("text");
}

fn modes() -> (u32, u32, u32, u32) {
    let (mut input, mut output) = (0, 0);
    // SAFETY: standard console handles and writable DWORDs.
    unsafe {
        check(GetConsoleMode(handle(0).unwrap(), &mut input)).unwrap();
        check(GetConsoleMode(handle(1).unwrap(), &mut output)).unwrap();
        (input, output, GetConsoleCP(), GetConsoleOutputCP())
    }
}
fn inject(records: &[InputRecord]) {
    let mut written = 0;
    // SAFETY: records has exactly the specified number of initialized records.
    check(unsafe {
        WriteConsoleInputW(handle(0).unwrap(), records.as_ptr(), records.len() as u32, &mut written)
    })
    .unwrap();
    assert_eq!(written as usize, records.len());
}
fn key(character: u16) -> InputRecord {
    InputRecord {
        kind: 1,
        event: Event { key: KeyEvent { down: 1, repeat: 1, character, ..KeyEvent::default() } },
    }
}

#[test]
#[ignore = "subprocess fixture; invoked by the native_* tests"]
fn hidden_console_child() {
    let case = std::env::var("RBIRDS_CONSOLE_TEST").unwrap();
    let input = std::fs::OpenOptions::new().read(true).write(true).open("CONIN$").unwrap();
    let output = std::fs::OpenOptions::new().read(true).write(true).open("CONOUT$").unwrap();
    // SAFETY: Files own the handles throughout the test; SetStdHandle borrows.
    unsafe {
        check(SetStdHandle(-10i32 as u32, input.as_raw_handle())).unwrap();
        check(SetStdHandle(-11i32 as u32, output.as_raw_handle())).unwrap();
    }
    let before = modes();
    match case.as_str() {
        "text" => {
            let saved_stderr = handle(2).unwrap();
            // SAFETY: this child process owns its console.
            unsafe {
                check(SetConsoleOutputCP(437)).unwrap();
            }
            for stderr in [false, true] {
                let mut info = ScreenInfo::default();
                // SAFETY: info is a writable console-info structure.
                check(unsafe { GetConsoleScreenBufferInfo(output.as_raw_handle(), &mut info) })
                    .unwrap();
                let text = "café Ж";
                if stderr {
                    // SAFETY: both borrowed handles remain valid throughout this case.
                    check(unsafe { SetStdHandle(-12i32 as u32, output.as_raw_handle()) }).unwrap();
                    crate::stdio::eprint(text.as_bytes());
                    check(unsafe { SetStdHandle(-12i32 as u32, saved_stderr) }).unwrap();
                } else {
                    let mut stdout = crate::stdio::CStdout::new();
                    stdout.print(text.as_bytes());
                    stdout.flush();
                }
                let expected: Vec<u16> = text.encode_utf16().collect();
                let mut actual = vec![0; expected.len()];
                let mut read = 0;
                // SAFETY: actual holds count writable UTF-16 code units.
                check(unsafe {
                    ReadConsoleOutputCharacterW(
                        output.as_raw_handle(),
                        actual.as_mut_ptr(),
                        actual.len() as u32,
                        info.cursor,
                        &mut read,
                    )
                })
                .unwrap();
                assert_eq!(read as usize, expected.len());
                assert_eq!(actual, expected, "stderr={stderr}");
                assert_eq!(modes().3, 437, "text output must not change the console code page");
            }
            // SAFETY: restore the saved console code page.
            unsafe {
                check(SetConsoleOutputCP(before.3)).unwrap();
            }
        }
        "sixel" | "sixel-unsupported" => {
            use std::io::Read;
            let terminal = crate::terminal::Terminal::enter().unwrap();
            let (mut reader, writer) = std::io::pipe().unwrap();
            // SAFETY: the pipe writer is alive until the console handle is restored.
            check(unsafe { SetStdHandle(-11i32 as u32, writer.as_raw_handle()) }).unwrap();
            let supported = case == "sixel";
            let emulator = std::thread::spawn(move || {
                let mut transcript = Vec::new();
                let mut bytes = [0; 128];
                for (query, reply) in [
                    (
                        &b"\x1b[c"[..],
                        if supported { &b"\x1b[?64;4;22c"[..] } else { &b"\x1b[?64;22c"[..] },
                    ),
                    (&b"\x1b[16t"[..], &b"\x1b[6;20;10t"[..]),
                    (&b"\x1b[>q"[..], &b"\x1bP>|Windows Terminal\x1b\\"[..]),
                    (&b"\x1b[?80$p"[..], &b"\x1b[?80;2$y"[..]),
                    (&b"\x1b[?80h"[..], &b""[..]),
                ] {
                    let mut request = Vec::new();
                    while request.len() < query.len() {
                        let count = reader.read(&mut bytes).unwrap();
                        assert_ne!(count, 0, "query stream ended early");
                        request.extend_from_slice(&bytes[..count]);
                    }
                    assert_eq!(request, query);
                    transcript.extend_from_slice(&request);
                    if !reply.is_empty() {
                        inject(&reply.iter().map(|&byte| key(u16::from(byte))).collect::<Vec<_>>());
                    }
                    if !supported {
                        break;
                    }
                }
                transcript
            });
            let result = crate::terminal::prepare_sixel();
            if supported {
                assert_eq!(
                    result.unwrap(),
                    crate::terminal::SixelTerminal {
                        cell_size: (10, 20),
                        erase_before_frame: false,
                        can_position_images: true,
                    }
                );
                assert_eq!(emulator.join().unwrap(), b"\x1b[c\x1b[16t\x1b[>q\x1b[?80$p\x1b[?80h");
            } else {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
                assert_eq!(emulator.join().unwrap(), b"\x1b[c");
            }
            // SAFETY: the original output File still owns the console handle.
            check(unsafe { SetStdHandle(-11i32 as u32, output.as_raw_handle()) }).unwrap();
            drop(terminal);
        }
        "frames" => {
            let path = std::env::temp_dir().join(format!("rbirds-鳥-é-{}.png", std::process::id()));
            let mut argv: Vec<OsString> = [
                "rbirds",
                "--render",
                "blocks",
                "--frames",
                "2",
                "--birds",
                "10",
                "--palette",
                "ember",
                "--snapshot",
            ]
            .into_iter()
            .map(Into::into)
            .collect();
            argv.push(path.as_os_str().to_owned());
            assert_eq!(crate::app::main(argv), 0);
            let image = crate::image::png::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert!(image.width > 0 && image.height > 0);
            assert!(image.pixels.chunks_exact(4).any(|p| p[..3] != crate::sprites::PICTURE_GROUND));
            std::fs::remove_file(path).unwrap();
        }
        "input" => {
            let result = std::panic::catch_unwind(|| {
                let _terminal = crate::terminal::Terminal::enter().unwrap();
                let raw = modes();
                assert_eq!(raw.0 & (2 | 4 | 0x40), 0);
                assert_ne!(raw.1 & 4, 0);
                assert_eq!((raw.2, raw.3), (65001, 65001));
                inject(&[
                    key(b'p' as u16),
                    key(0xd83d),
                    key(0xdc26),
                    InputRecord {
                        kind: 1,
                        event: Event {
                            key: KeyEvent { down: 1, repeat: 1, key: 0x26, ..KeyEvent::default() },
                        },
                    },
                    InputRecord {
                        kind: 2,
                        event: Event {
                            mouse: MouseEvent {
                                position: Coord { x: 3, y: 4 },
                                buttons: 1,
                                controls: 0,
                                flags: 0,
                            },
                        },
                    },
                ]);
                let mut bytes = [0; 64];
                let n = read(0, &mut bytes).unwrap();
                assert_eq!(&bytes[..n], "p🐦\x1b[A\x1b[<0;4;5M".as_bytes());
                assert_eq!(read(0, &mut bytes).unwrap(), 0);
                // SAFETY: console handle, by-value COORD, and a live SMALL_RECT.
                unsafe {
                    check(SetConsoleScreenBufferSize(
                        output.as_raw_handle(),
                        Coord { x: 200, y: 100 },
                    ))
                    .unwrap();
                    check(SetConsoleWindowInfo(
                        output.as_raw_handle(),
                        1,
                        &Rect { left: 0, top: 0, right: 79, bottom: 23 },
                    ))
                    .unwrap();
                }
                let size = window_size(1).unwrap();
                assert_eq!((size.col, size.row), (80, 24));
                // Scrolled ten rows down its buffer, the window's top left is
                // still cell 1;1 to the program: buffer row 14 is window row 5.
                // SAFETY: console handle and a live SMALL_RECT inside the buffer.
                unsafe {
                    check(SetConsoleWindowInfo(
                        output.as_raw_handle(),
                        1,
                        &Rect { left: 0, top: 10, right: 79, bottom: 33 },
                    ))
                    .unwrap();
                }
                inject(&[InputRecord {
                    kind: 2,
                    event: Event {
                        mouse: MouseEvent {
                            position: Coord { x: 3, y: 14 },
                            buttons: 1,
                            controls: 0,
                            flags: 0,
                        },
                    },
                }]);
                let n = read(0, &mut bytes).unwrap();
                assert_eq!(&bytes[..n], b"\x1b[<0;4;5M");
                let size = window_size(1).unwrap();
                assert_eq!((size.col, size.row), (80, 24));
                panic!("controlled terminal panic");
            });
            let panic = result.expect_err("the controlled panic must unwind");
            assert_eq!(panic.downcast_ref::<&str>(), Some(&"controlled terminal panic"));
        }
        "control" => {
            let trigger = std::thread::spawn(|| {
                while !RAW.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                // SAFETY: CTRL_BREAK_EVENT to our private console group.
                check(unsafe { GenerateConsoleCtrlEvent(1, 0) }).unwrap();
            });
            assert_eq!(
                crate::app::main(
                    ["rbirds", "--render", "braille", "--birds", "10", "--palette", "ember"]
                        .into_iter()
                        .map(Into::into)
                        .collect()
                ),
                130
            );
            trigger.join().unwrap();
        }
        "backpressure" | "cancel-write" => {
            use std::io::Read;
            let terminal = crate::terminal::Terminal::enter().unwrap();
            let (mut reader, writer) = std::io::pipe().unwrap();
            // SAFETY: writer remains alive until standard output is restored.
            check(unsafe { SetStdHandle(-11i32 as u32, writer.as_raw_handle()) }).unwrap();
            let expected: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
            let mut graphics = crate::render::kitty::KittyGraphics::new(1).unwrap();
            graphics.write_raw(&expected).unwrap();
            assert_eq!(graphics.flush_nonblocking(), Err(crate::render::kitty::KittyError::Again));
            inject(&[key(b'q' as u16)]);
            let mut bytes = [0; 16];
            assert_eq!(read(0, &mut bytes).unwrap(), 1);
            assert_eq!(bytes[0], b'q');
            if case == "backpressure" {
                let drain = std::thread::spawn(move || {
                    let mut actual = vec![0; 200_000];
                    reader.read_exact(&mut actual).unwrap();
                    actual
                });
                let start = Instant::now();
                loop {
                    match graphics.flush_nonblocking() {
                        Ok(()) => break,
                        Err(crate::render::kitty::KittyError::Again) => {
                            let mut ready = [PollFd::new(1, POLLOUT)];
                            assert_eq!(poll(&mut ready, 2000).unwrap(), 1);
                        }
                        Err(error) => panic!("{error}"),
                    }
                    assert!(start.elapsed() < Duration::from_secs(5));
                }
                assert_eq!(drain.join().unwrap(), expected);
                assert_eq!(graphics.len(), 0);
            } else {
                assert!(!active_writer().unwrap().stop());
                // The cancelled write and the stopped writer both fail the
                // flush with EIO, rather than the EINTR it would retry forever.
                assert_eq!(
                    graphics.flush_nonblocking(),
                    Err(crate::render::kitty::KittyError::Io(EIO))
                );
                drop(reader);
            }
            // SAFETY: output File still owns this original console handle.
            check(unsafe { SetStdHandle(-11i32 as u32, output.as_raw_handle()) }).unwrap();
            drop(terminal);
        }
        _ => panic!("unknown case"),
    }
    assert_eq!(modes(), before, "console modes and code pages must be restored");
}
