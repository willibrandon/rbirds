//! Differential tests of `render::kitty` against the pinned C
//! `kitty_graphics.c`.
//!
//! Each scenario is a script of calls (`tools/oracle/kitty_oracle.c` documents
//! the format). The C oracle runs it against the reference and prints a
//! transcript: every status, the queue's length and capacity after each call,
//! the `errno` left by every I/O failure and would-block, the descriptor's
//! `O_NONBLOCK` after every flush, and the queued bytes on request. The same
//! script runs here against the Rust translation and must print exactly the
//! same transcript; bytes flushed to standard output must also be identical.

// The shared oracle harness is not this suite's to change, and as committed it
// is neither rustfmt-clean nor free of clippy's collapsible_if; keep both
// checks to this crate's own code.
#[rustfmt::skip]
#[allow(clippy::collapsible_if)]
mod support;

use std::fmt::Write as _;
use std::io::{PipeReader, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

use rbirds::platform;
use rbirds::render::kitty::{KittyError, KittyGraphics, Placement, kitty_graphics_status_string};

#[derive(Clone, Debug)]
enum Op {
    Upload(u32, Vec<u8>),
    Place(Placement),
    DeletePlacement(u32, u32),
    DeleteAll,
    DeleteImage(u32),
    Text(i32, i32, Vec<u8>),
    Raw(Vec<u8>),
    Begin,
    End,
    Clear,
    Flush,
    FlushNonblocking,
    Drain(usize),
    /// Sets or clears O_NONBLOCK on the output descriptor from outside.
    Nonblock(bool),
    Dump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Stdout,
    Bad,
    Epipe,
    Full,
}

impl Mode {
    fn arg(self) -> &'static str {
        match self {
            Mode::Stdout => "stdout",
            Mode::Bad => "bad",
            Mode::Epipe => "epipe",
            Mode::Full => "full",
        }
    }
}

fn blob(out: &mut Vec<u8>, head: String, bytes: &[u8]) {
    out.extend_from_slice(format!("{head} {}\n", bytes.len()).as_bytes());
    out.extend_from_slice(bytes);
    out.push(b'\n');
}

fn script(ops: &[Op]) -> Vec<u8> {
    let mut out = Vec::new();
    for op in ops {
        let line = match op {
            Op::Upload(id, png) => {
                blob(&mut out, format!("U {id}"), png);
                continue;
            }
            Op::Place(p) => format!(
                "P {} {} {} {} {} {} {}\n",
                p.image_id, p.placement_id, p.row, p.column, p.x_offset, p.y_offset, p.z_index
            ),
            Op::DeletePlacement(image, placement) => format!("D {image} {placement}\n"),
            Op::DeleteAll => "A\n".to_owned(),
            Op::DeleteImage(image) => format!("X {image}\n"),
            Op::Text(row, column, text) => {
                blob(&mut out, format!("T {row} {column}"), text);
                continue;
            }
            Op::Raw(bytes) => {
                blob(&mut out, "W".to_owned(), bytes);
                continue;
            }
            Op::Begin => "B\n".to_owned(),
            Op::End => "E\n".to_owned(),
            Op::Clear => "L\n".to_owned(),
            Op::Flush => "F\n".to_owned(),
            Op::FlushNonblocking => "N\n".to_owned(),
            Op::Drain(n) => format!("R {n}\n"),
            Op::Nonblock(on) => format!("O {}\n", i32::from(*on)),
            Op::Dump => "Q\n".to_owned(),
        };
        out.extend_from_slice(line.as_bytes());
    }
    out
}

fn escaped(out: &mut String, bytes: &[u8]) {
    for &c in bytes {
        if (0x20..0x7f).contains(&c) && c != b'\\' {
            out.push(c as char);
        } else {
            let _ = write!(out, "\\x{c:02x}");
        }
    }
}

/// The output descriptor the oracle's mode stands for, set up the same way.
struct Output {
    fd: RawFd,
    drain: Option<PipeReader>,
    collector: Option<std::thread::JoinHandle<Vec<u8>>>,
    _writer: Option<std::io::PipeWriter>,
}

fn open(mode: Mode) -> Output {
    match mode {
        Mode::Stdout => {
            let (mut reader, writer) = std::io::pipe().expect("pipe");
            let collector = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).expect("collect flushed bytes");
                bytes
            });
            Output {
                fd: writer.as_raw_fd(),
                drain: None,
                collector: Some(collector),
                _writer: Some(writer),
            }
        }
        Mode::Bad => Output { fd: i32::MAX, drain: None, collector: None, _writer: None },
        Mode::Epipe => {
            let (reader, writer) = std::io::pipe().expect("pipe");
            drop(reader);
            Output { fd: writer.as_raw_fd(), drain: None, collector: None, _writer: Some(writer) }
        }
        Mode::Full => {
            let (reader, writer) = std::io::pipe().expect("pipe");
            let fd = writer.as_raw_fd();
            let fill = [0u8; 4096];
            let flags = platform::status_flags(fd).expect("F_GETFL");
            platform::set_status_flags(fd, flags | platform::O_NONBLOCK).expect("F_SETFL");
            let refusal = loop {
                match platform::write(fd, &fill) {
                    Ok(n) if n > 0 => {}
                    Ok(_) => panic!("write returned 0"),
                    Err(error) => break error,
                }
            };
            assert_eq!(refusal.raw_os_error(), Some(platform::EAGAIN));
            platform::set_status_flags(fd, flags).expect("F_SETFL");
            Output { fd, drain: Some(reader), collector: None, _writer: Some(writer) }
        }
    }
}

/// Runs the script against the Rust translation, printing what the C oracle
/// prints; returns the transcript and the bytes flushed in stdout mode.
fn transcript(mode: Mode, ops: &[Op]) -> (String, Vec<u8>) {
    let mut output = open(mode);
    let mut graphics = KittyGraphics::new(output.fd).expect("init");
    let mut out = String::new();
    let mut drained = vec![0u8; 1 << 17];
    for op in ops {
        let mut flushed = false;
        let (letter, status) = match op {
            Op::Upload(id, png) => ('U', graphics.upload_png(*id, png)),
            Op::Place(placement) => ('P', graphics.place(placement)),
            Op::DeletePlacement(image, placement) => {
                ('D', graphics.delete_placement(*image, *placement))
            }
            Op::DeleteAll => ('A', graphics.delete_all_placements()),
            Op::DeleteImage(image) => ('X', graphics.delete_image(*image)),
            Op::Text(row, column, text) => ('T', graphics.write_text(*row, *column, text)),
            Op::Raw(bytes) => ('W', graphics.write_raw(bytes)),
            Op::Begin => ('B', graphics.begin_synchronized_update()),
            Op::End => ('E', graphics.end_synchronized_update()),
            Op::Clear => {
                graphics.clear();
                ('L', Ok(()))
            }
            Op::Flush => {
                flushed = true;
                ('F', graphics.flush())
            }
            Op::FlushNonblocking => {
                flushed = true;
                ('N', graphics.flush_nonblocking())
            }
            Op::Drain(n) => {
                let reader = output.drain.as_mut().expect("drain needs the full pipe");
                let got = reader.read(&mut drained[..*n]).map(|n| n as i64).unwrap_or(-1);
                let _ = writeln!(out, "read {got}");
                continue;
            }
            Op::Nonblock(on) => {
                let flags = platform::status_flags(output.fd).expect("F_GETFL");
                let flags =
                    if *on { flags | platform::O_NONBLOCK } else { flags & !platform::O_NONBLOCK };
                platform::set_status_flags(output.fd, flags).expect("F_SETFL");
                continue;
            }
            Op::Dump => {
                out.push_str("buffer ");
                escaped(&mut out, graphics.buffer());
                out.push('\n');
                continue;
            }
        };
        // The thread's errno, read before anything else can disturb it.
        let error = platform::errno();
        let _ = write!(
            out,
            "{letter} {} {} {}",
            kitty_graphics_status_string(status),
            graphics.len(),
            graphics.capacity()
        );
        match status {
            Err(KittyError::Io(code)) => {
                assert_eq!(code, error, "Io carries the errno the call left");
                let _ = write!(out, " errno={code}");
            }
            Err(KittyError::Again) => {
                let _ = write!(out, " errno={error}");
            }
            _ => {}
        }
        if flushed {
            let nonblock = match platform::status_flags(output.fd) {
                Ok(flags) => i32::from(flags & platform::O_NONBLOCK != 0),
                Err(_) => -1,
            };
            let _ = write!(out, " nonblock={nonblock}");
        }
        out.push('\n');
    }
    drop(graphics);
    drop(output._writer.take());
    let flushed = output.collector.take().map(|c| c.join().expect("collector")).unwrap_or_default();
    (out, flushed)
}

fn oracle() -> Option<std::path::PathBuf> {
    support::oracle::build("kitty_oracle", "kitty_oracle.c", &["kitty_graphics.c"])
}

fn first_divergence(what: &str, expected: &[u8], actual: &[u8]) -> String {
    let at = expected
        .iter()
        .zip(actual)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    let line = expected[..at].iter().filter(|&&b| b == b'\n').count() + 1;
    let window = |bytes: &[u8]| {
        let (start, end) = (at.saturating_sub(40), (at + 40).min(bytes.len()));
        let mut shown = String::new();
        escaped(&mut shown, &bytes[start.min(end)..end]);
        shown
    };
    format!(
        "{what}: first divergence at byte {at} (line {line}; lengths C {} Rust {})\n  C:    {}\n  Rust: {}",
        expected.len(),
        actual.len(),
        window(expected),
        window(actual)
    )
}

fn check(exe: &Path, name: &str, mode: Mode, ops: &[Op]) {
    let input = script(ops);
    let c = support::oracle::run(exe, &[mode.arg()], &input);
    let (rust, rust_flushed) = transcript(mode, ops);
    let transcript_matches = c.stderr == rust.as_bytes();
    let flushed_matches = c.stdout == rust_flushed;
    if transcript_matches && flushed_matches {
        return;
    }
    let scratch = support::oracle::Scratch::new(&format!("kitty_oracle-{name}"));
    std::fs::write(scratch.file("script"), &input).unwrap();
    std::fs::write(scratch.file("c.txt"), &c.stderr).unwrap();
    std::fs::write(scratch.file("rust.txt"), &rust).unwrap();
    std::fs::write(scratch.file("c.out"), &c.stdout).unwrap();
    std::fs::write(scratch.file("rust.out"), &rust_flushed).unwrap();
    if !transcript_matches {
        panic!("scenario {name}: {}", first_divergence("transcript", &c.stderr, rust.as_bytes()));
    }
    panic!("scenario {name}: {}", first_divergence("flushed bytes", &c.stdout, &rust_flushed));
}

// --- Corpus -------------------------------------------------------------------

/// SplitMix64: a fixed, seedable source for the corpus.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| self.next() as u8).collect()
    }
}

const UPLOAD_LENGTHS: [usize; 20] = [
    1, 2, 3, 4, 5, 6, 7, 100, 3071, 3072, 3073, 3074, 3075, 4096, 6143, 6144, 6145, 9216, 10000,
    20000,
];

fn placements() -> Vec<Placement> {
    let mut all = Vec::new();
    let ids = [0u32, 1, 9, 65536, u32::MAX];
    let placement_ids = [0u32, 1, 12, u32::MAX];
    let z_indexes = [0, -1, 1, -1_000_000_000, i32::MIN, i32::MAX];
    let positions = [0, 1, 2, 23, 999, i32::MAX - 1, i32::MAX, -1, i32::MIN];
    let offsets = [0, 3, 6, 4095, i32::MAX, -1, i32::MIN];
    for (i, &image_id) in ids.iter().enumerate() {
        for (j, &placement_id) in placement_ids.iter().enumerate() {
            for (k, &z_index) in z_indexes.iter().enumerate() {
                // Walk the positions and offsets with the other indexes, so
                // every value meets every kind of command.
                let n = i + j * 5 + k * 20;
                all.push(Placement {
                    image_id,
                    placement_id,
                    row: positions[n % positions.len()],
                    column: positions[(n / 3) % positions.len()],
                    x_offset: offsets[n % offsets.len()],
                    y_offset: offsets[(n / 2) % offsets.len()],
                    z_index,
                });
            }
        }
    }
    for &row in &positions {
        for &column in &positions {
            all.push(Placement {
                image_id: 3,
                placement_id: 4,
                row,
                column,
                x_offset: 1,
                y_offset: 2,
                z_index: 5,
            });
        }
    }
    for &x_offset in &offsets {
        for &y_offset in &offsets {
            all.push(Placement {
                image_id: 3,
                placement_id: 0,
                row: 7,
                column: 8,
                x_offset,
                y_offset,
                z_index: 0,
            });
        }
    }
    all
}

/// A long command stream written to standard output: uploads of every chunk
/// boundary, every placement form and refusal, deletions, text and raw bytes,
/// synchronized updates, clears, and both flushes.
fn stream(seed: u64) -> Vec<Op> {
    let mut rng = Rng(seed);
    let mut ops = vec![
        // Nothing to queue: no storage either.
        Op::Raw(Vec::new()),
        Op::Dump,
        Op::FlushNonblocking, // Nothing queued: nothing written.
        Op::Flush,
        Op::Dump,
        Op::Begin,
        Op::DeleteAll,
        Op::Place(Placement {
            image_id: 9,
            placement_id: 12,
            row: 2,
            column: 4,
            x_offset: 3,
            y_offset: 6,
            z_index: -1,
        }),
        Op::End,
        Op::Dump,
        Op::FlushNonblocking,
        Op::Dump,
    ];
    // Refused uploads queue nothing.
    ops.push(Op::Upload(0, rng.bytes(5)));
    ops.push(Op::Upload(1, Vec::new()));
    ops.push(Op::Upload(0, Vec::new()));
    let ids = [1u32, 7, 255, 65536, u32::MAX];
    for (i, &length) in UPLOAD_LENGTHS.iter().enumerate() {
        ops.push(Op::Upload(ids[i % ids.len()], rng.bytes(length)));
        ops.push(Op::Dump);
        if i % 4 == 3 {
            ops.push(Op::Flush);
        }
    }
    // Uniform data: every Base64 character, and runs of each padding case.
    ops.push(Op::Upload(2, (0..=255u8).collect()));
    ops.push(Op::Upload(3, vec![0xFF; 3073]));
    ops.push(Op::Dump);
    ops.push(Op::Flush);

    for (i, placement) in placements().iter().enumerate() {
        ops.push(Op::Place(*placement));
        if i % 50 == 49 {
            ops.push(Op::Dump);
            ops.push(Op::Flush);
        }
    }
    ops.push(Op::Dump);

    for (image, placement) in
        [(0, 0), (0, 1), (1, 0), (1, 1), (9, 12), (u32::MAX, u32::MAX), (65536, 7)]
    {
        ops.push(Op::DeletePlacement(image, placement));
    }
    for image in [0, 1, 9, u32::MAX] {
        ops.push(Op::DeleteImage(image));
    }
    ops.push(Op::DeleteAll);
    ops.push(Op::Dump);
    ops.push(Op::Flush);

    let texts: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"bar".to_vec(),
        b"\x1b[7mx\x1b[0m".to_vec(),
        b"\x1b[K".to_vec(),
        "h\u{e9}llo \u{28ff} \u{1fb00}".as_bytes().to_vec(),
        b"ab\0cd".to_vec(),
        b"\0".to_vec(),
        (1..=255u8).collect(),
        rng.bytes(5000).into_iter().map(|b| b | 1).collect(),
    ];
    let cells = [
        (0, 0),
        (23, 0),
        (11, 4),
        (-1, 0),
        (0, -1),
        (i32::MIN, 3),
        (i32::MAX, i32::MAX),
        (i32::MAX - 1, 0),
    ];
    for (i, text) in texts.iter().enumerate() {
        for (j, &(row, column)) in cells.iter().enumerate() {
            if (i + j) % 2 == 0 || text.len() < 10 {
                ops.push(Op::Text(row, column, text.clone()));
            }
        }
        ops.push(Op::Dump);
    }
    ops.push(Op::Flush);

    for raw in [
        Vec::new(),
        vec![0],
        b"\x1b[?25l".to_vec(),
        vec![0, 0, 0, 1],
        rng.bytes(10000),
        rng.bytes(70000),
    ] {
        ops.push(Op::Raw(raw));
        ops.push(Op::Dump);
    }
    ops.push(Op::Flush);

    // The benchmark's clear: capacity stays, the queue starts again.
    ops.push(Op::Upload(4, rng.bytes(9000)));
    ops.push(Op::Clear);
    ops.push(Op::Dump);
    ops.push(Op::Begin);
    ops.push(Op::Text(0, 0, b"after".to_vec()));
    ops.push(Op::End);
    ops.push(Op::Dump);
    ops.push(Op::Flush);
    ops.push(Op::Clear);
    ops.push(Op::Flush);
    ops
}

#[test]
fn a_command_stream_matches_the_reference_byte_for_byte() {
    let Some(exe) = oracle() else { return };
    for seed in [1, 2, 3] {
        check(&exe, &format!("stream-{seed}"), Mode::Stdout, &stream(seed));
    }
}

/// A descriptor nobody has open: both flushes fail with its errno and keep
/// everything queued.
#[test]
fn flushing_to_a_closed_descriptor_matches_the_reference() {
    let Some(exe) = oracle() else { return };
    let mut rng = Rng(5);
    let ops = vec![
        Op::Flush,
        Op::FlushNonblocking,
        Op::Upload(1, rng.bytes(4000)),
        Op::Text(0, 0, b"x".to_vec()),
        Op::Flush,
        Op::Dump,
        Op::FlushNonblocking,
        Op::Dump,
    ];
    check(&exe, "bad", Mode::Bad, &ops);
}

/// A reader that went away: EPIPE from both flushes, with O_NONBLOCK restored
/// after the nonblocking one.
#[test]
fn flushing_to_a_closed_pipe_matches_the_reference() {
    let Some(exe) = oracle() else { return };
    let mut rng = Rng(6);
    let ops = vec![
        Op::Flush,
        Op::Upload(1, rng.bytes(10)),
        Op::Flush,
        Op::Dump,
        Op::FlushNonblocking,
        Op::Dump,
        Op::Raw(rng.bytes(100_000)),
        Op::FlushNonblocking,
        Op::Flush,
    ];
    check(&exe, "epipe", Mode::Epipe, &ops);
}

/// Backpressure: a full pipe refuses a nonblocking flush and the queue is kept
/// whole; drained a little at a time, a large queue goes out in pieces, each
/// refusal keeping exactly the unsent suffix; O_NONBLOCK is back as it was
/// after every attempt.
#[test]
fn backpressure_matches_the_reference() {
    let Some(exe) = oracle() else { return };
    let mut rng = Rng(8);
    let mut ops = vec![
        Op::Upload(1, vec![0]),
        Op::FlushNonblocking,
        Op::Dump,
        Op::FlushNonblocking,
        Op::Drain(8192),
        Op::FlushNonblocking,
        Op::Dump,
        Op::Raw(rng.bytes(100_000)),
        Op::Upload(2, rng.bytes(7000)),
    ];
    // Every drain follows a refused flush, so the pipe is full and the read
    // cannot block; the queue outlasts all but the last of them.
    for drain in [0, 1000, 4096, 16384, 65536, 65536] {
        if drain > 0 {
            ops.push(Op::Drain(drain));
        }
        ops.push(Op::FlushNonblocking);
    }
    ops.push(Op::Dump);
    // Somebody else made the descriptor nonblocking: the blocking flush reports
    // EAGAIN as an output error, and the nonblocking one leaves the flag set.
    ops.push(Op::Upload(3, rng.bytes(70000)));
    ops.push(Op::Nonblock(true));
    ops.push(Op::Flush);
    ops.push(Op::Dump);
    ops.push(Op::FlushNonblocking);
    ops.push(Op::Drain(4096));
    ops.push(Op::Flush);
    ops.push(Op::Nonblock(false));
    ops.push(Op::FlushNonblocking);
    ops.push(Op::Dump);
    check(&exe, "full", Mode::Full, &ops);
}
