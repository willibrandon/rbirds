//! Differential tests of `rbirds::image::gif` against the pinned C `gif.c`.
//!
//! The same frames go through `tools/oracle/gif_oracle.c` (the reference's
//! gif_open, gif_add_frame and gif_close) and through [`GifWriter`]; every
//! status, the byte and frame counts, and the files themselves must be
//! identical. Failing files are kept under `target/scratch` for diagnosis.

mod support;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rbirds::image::Image;
use rbirds::image::gif::{GifError, GifWriter};
use support::oracle;

fn gif_oracle() -> Option<PathBuf> {
    oracle::build("gif_oracle", "gif_oracle.c", &["gif.c", "png.c"])
}

/// A fixed-seed generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}

/// The C's gif_status_t numbering.
fn code(status: Result<(), GifError>) -> i32 {
    match status {
        Ok(()) => 0,
        Err(GifError::Argument) => 1,
        Err(GifError::Memory) => 2,
        Err(GifError::Io) => 3,
    }
}

/// Frame content.
#[derive(Clone, Copy, Debug)]
enum Paint {
    /// One colour.
    Flat([u8; 4]),
    /// Every channel random: far more colours than the table holds.
    Noise,
    /// A smooth ramp over many buckets.
    Gradient,
    /// The C test's moving checker, at this step.
    Checker(i32),
    /// Few colours, varying alpha, which the writer ignores.
    Alpha,
}

fn paint(width: i32, height: i32, how: Paint, rng: &mut Rng) -> Image {
    // Built directly rather than through Image::alloc, whose PNG limits (16384
    // a side) are narrower than a GIF's 65535.
    let mut frame = Image { width, height, pixels: vec![0; width as usize * height as usize * 4] };
    for y in 0..height {
        for x in 0..width {
            let at = (y as usize * width as usize + x as usize) * 4;
            let p = &mut frame.pixels[at..at + 4];
            match how {
                Paint::Flat(colour) => p.copy_from_slice(&colour),
                Paint::Noise => {
                    for v in p.iter_mut() {
                        *v = rng.byte();
                    }
                }
                Paint::Gradient => p.copy_from_slice(&[
                    (x * 255 / width.max(2)) as u8,
                    (y * 255 / height.max(2)) as u8,
                    ((x + y) * 7) as u8,
                    255,
                ]),
                Paint::Checker(step) => {
                    let on = ((x + step * 3) / 6 + y / 5) % 2 == 1;
                    p.copy_from_slice(if on { &[240, 90, 40, 255] } else { &[20, 20, 30, 255] });
                }
                Paint::Alpha => {
                    let base = [((x / 4) * 60) as u8, 100, ((y / 3) * 40) as u8];
                    p[..3].copy_from_slice(&base);
                    p[3] = rng.byte();
                }
            }
        }
    }
    frame
}

/// A frame record for the oracle: the empty image goes as pixels NULL.
fn frame_records(frames: &[Image]) -> Vec<u8> {
    let mut records = Vec::new();
    for frame in frames {
        records.extend_from_slice(&frame.width.to_le_bytes());
        records.extend_from_slice(&frame.height.to_le_bytes());
        records.push(u8::from(!frame.pixels.is_empty()));
        records.extend_from_slice(&frame.pixels);
    }
    records
}

/// Runs the oracle with raw OS arguments, so non-UTF-8 paths get through.
fn run_c(
    exe: &Path,
    path: &OsStr,
    width: i32,
    height: i32,
    delay: i32,
    frames: &[Image],
) -> String {
    let mut child = Command::new(exe)
        .arg(path)
        .args([width.to_string(), height.to_string(), delay.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", exe.display()));
    let mut input = child.stdin.take().expect("stdin");
    let data = frame_records(frames);
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let output = child.wait_with_output().expect("oracle output");
    let _ = writer.join();
    assert!(
        output.status.success(),
        "gif_oracle failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("oracle prints text")
}

/// The same sequence through the Rust writer, reported the same way.
fn run_rust(path: &Path, width: i32, height: i32, delay: i32, frames: &[Image]) -> String {
    let mut out = String::new();
    let mut writer = match GifWriter::open(path, width, height, delay) {
        Ok(writer) => writer,
        Err(error) => return format!("open {}\n", code(Err(error))),
    };
    out.push_str("open 0\n");
    for frame in frames {
        out.push_str(&format!("frame {}\n", code(writer.add_frame(frame))));
    }
    let closed = writer.close();
    out.push_str(&format!("close {} {} {}\n", code(closed.status), closed.bytes, closed.frames));
    out
}

struct Case {
    label: String,
    width: i32,
    height: i32,
    delay: i32,
    frames: Vec<Image>,
}

/// Runs a case through both writers into files of their own and insists on
/// the same report and the same bytes. Returns the report.
fn compare(exe: &Path, scratch: &oracle::Scratch, n: usize, case: &Case) -> String {
    let c_path = scratch.file(&format!("{n}.c.gif"));
    let rust_path = scratch.file(&format!("{n}.rust.gif"));
    let c = run_c(exe, c_path.as_os_str(), case.width, case.height, case.delay, &case.frames);
    let rust = run_rust(&rust_path, case.width, case.height, case.delay, &case.frames);
    assert_eq!(c, rust, "{}: reports differ", case.label);
    let c_bytes = fs::read(&c_path).ok();
    let rust_bytes = fs::read(&rust_path).ok();
    if c_bytes != rust_bytes {
        let at = match (&c_bytes, &rust_bytes) {
            (Some(a), Some(b)) => a.iter().zip(b).position(|(x, y)| x != y),
            _ => None,
        };
        panic!(
            "{}: files differ (C {:?} bytes, Rust {:?}, first difference at {at:?}); kept in {}",
            case.label,
            c_bytes.as_ref().map(Vec::len),
            rust_bytes.as_ref().map(Vec::len),
            scratch.path.display()
        );
    }
    c
}

/// Same frames through C and Rust: byte-identical files over sizes, delays,
/// frame counts, palette-overflowing, flat, gradient and alpha content, and
/// frames that need colours the first frame's table lacks.
#[test]
fn gif_files_match_c() {
    let Some(exe) = gif_oracle() else { return };
    let scratch = oracle::Scratch::new("gif_oracle.files");
    let mut rng = Rng::new(4);
    let mut cases = Vec::new();
    let mut add =
        |label: &str, width: i32, height: i32, delay: i32, paints: &[Paint], rng: &mut Rng| {
            let frames = paints.iter().map(|&p| paint(width, height, p, rng)).collect();
            cases.push(Case { label: label.to_owned(), width, height, delay, frames });
        };

    add("1x1 noise", 1, 1, 5, &[Paint::Noise], &mut rng);
    add("1x7 three frames", 1, 7, 3, &[Paint::Noise, Paint::Gradient, Paint::Noise], &mut rng);
    add(
        "7x1 three frames",
        7,
        1,
        2,
        &[Paint::Gradient, Paint::Noise, Paint::Flat([1, 2, 3, 4])],
        &mut rng,
    );
    add("16x16 delay 0", 16, 16, 0, &[Paint::Gradient, Paint::Gradient], &mut rng);
    add("16x16 delay -7", 16, 16, -7, &[Paint::Noise], &mut rng);
    let checker: Vec<Paint> = (0..6).map(Paint::Checker).collect();
    add("64x40 the C test's animation", 64, 40, 5, &checker, &mut rng);
    add(
        "97x61 noise, delay 255",
        97,
        61,
        255,
        &[Paint::Noise, Paint::Noise, Paint::Noise],
        &mut rng,
    );
    add(
        "200x150 noise, table resets, delay 256",
        200,
        150,
        256,
        &[Paint::Noise, Paint::Noise],
        &mut rng,
    );
    add("320x200 flat, four frames", 320, 200, 4, &[Paint::Flat([18, 18, 24, 255]); 4], &mut rng);
    add("50x50 gradient, delay 65535", 50, 50, 65535, &[Paint::Gradient], &mut rng);
    add("30x20 alpha, delay 65536", 30, 20, 65536, &[Paint::Alpha, Paint::Alpha], &mut rng);
    add(
        "40x30 later frames off the first table",
        40,
        30,
        7,
        &[Paint::Flat([200, 0, 0, 255]), Paint::Gradient, Paint::Noise, Paint::Checker(2)],
        &mut rng,
    );
    add("255x1", 255, 1, 1, &[Paint::Noise, Paint::Flat([0, 0, 0, 0])], &mut rng);
    add("1x255", 1, 255, 1, &[Paint::Gradient], &mut rng);
    add("65535x1 flat", 65535, 1, 10, &[Paint::Flat([9, 9, 9, 255])], &mut rng);
    add("300x1 black", 300, 1, 10, &[Paint::Flat([0, 0, 0, 255])], &mut rng);
    add("no frames", 12, 9, 5, &[], &mut rng);
    add("100000 delay", 3, 3, 100000, &[Paint::Checker(0)], &mut rng);

    for (n, case) in cases.iter().enumerate() {
        let report = compare(&exe, &scratch, n, case);
        let expected_frames = case.frames.len();
        assert!(
            report.ends_with(&format!(" {expected_frames}\n")) && report.contains("close 0 "),
            "{}: {report}",
            case.label
        );
    }
}

/// Refusals: bad sizes, paths that cannot be created, frames that do not
/// belong, all with the same status as the C, and the same file afterwards.
#[test]
fn gif_refusals_match_c() {
    let Some(exe) = gif_oracle() else { return };
    let scratch = oracle::Scratch::new("gif_oracle.refusals");
    let mut rng = Rng::new(5);

    for (width, height) in [(0, 4), (4, 0), (-1, 4), (4, -3), (65536, 1), (1, 65536), (i32::MIN, 1)]
    {
        let case = Case {
            label: format!("open {width}x{height}"),
            width,
            height,
            delay: 5,
            frames: Vec::new(),
        };
        let report = compare(&exe, &scratch, 0, &case);
        assert_eq!(report, "open 1\n", "{}", case.label);
        assert!(!scratch.file("0.c.gif").exists() && !scratch.file("0.rust.gif").exists());
    }

    // Paths that cannot be created: a missing directory, a directory, and
    // the empty path.
    let directory = scratch.file("a directory");
    fs::create_dir(&directory).expect("mkdir");
    let unwritable: [OsString; 3] = [
        scratch.file("missing/x.gif").into_os_string(),
        directory.into_os_string(),
        OsString::new(),
    ];
    let frames = vec![paint(4, 4, Paint::Noise, &mut rng)];
    for path in &unwritable {
        let c = run_c(&exe, path, 4, 4, 5, &frames);
        let rust = run_rust(Path::new(path), 4, 4, 5, &frames);
        assert_eq!(c, rust, "{path:?}");
        assert_eq!(c, "open 3\n", "{path:?}");
    }

    // A name that is not UTF-8. Whether the file system takes it is its own
    // business; both writers must get the same answer and the same bytes.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = scratch.path.join(OsStr::from_bytes(b"c_\xff\xfe.gif"));
        let rust_path = scratch.path.join(OsStr::from_bytes(b"rust_\xff\xfe.gif"));
        let c = run_c(&exe, c_path.as_os_str(), 4, 4, 5, &frames);
        let rust = run_rust(&rust_path, 4, 4, 5, &frames);
        assert_eq!(c, rust, "non-UTF-8 path");
        assert_eq!(fs::read(&c_path).ok(), fs::read(&rust_path).ok(), "non-UTF-8 path contents");
        eprintln!("non-UTF-8 path: {}", c.lines().next().unwrap_or(""));
    }

    // Frames that are not this animation's: the wrong size either way, no
    // pixels, no pixels at the right size. None of them stops the writer.
    let good = paint(8, 6, Paint::Gradient, &mut rng);
    let frames = vec![
        Image::empty(),
        paint(9, 6, Paint::Noise, &mut rng),
        paint(8, 7, Paint::Noise, &mut rng),
        Image { width: 8, height: 6, pixels: Vec::new() },
        good.clone(),
        paint(6, 8, Paint::Noise, &mut rng),
        good,
    ];
    let case =
        Case { label: "frames that do not belong".into(), width: 8, height: 6, delay: 5, frames };
    let report = compare(&exe, &scratch, 1, &case);
    assert_eq!(report.lines().filter(|l| *l == "frame 1").count(), 5, "{report}");
}
