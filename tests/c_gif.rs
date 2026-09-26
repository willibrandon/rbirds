//! cbirds `tests/gif_test.c`, translated one test for one test.
//!
//! The C test carries its own GIF reader, written from the specification
//! rather than borrowed from the encoder; it is translated here with it, so
//! the Rust writer is still checked by a decoder that owes nothing to it.
//!
//! The C writes into one `mkdtemp` directory and fails if any test leaves a
//! file behind. Rust runs the tests in parallel, so each one gets a directory
//! of its own, created the same exclusive way, and must be able to remove it
//! empty when it is done.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use rbirds::image::Image;
use rbirds::image::gif::{GifError, GifWriter, gif_status_string};

/// A directory of the run's own for the files a test writes: a fixed name
/// collides with a second run, and may already be something else, a symlink
/// included, that a truncating open would then write through.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Scratch {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir();
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("rbirds_gif_test.{}.{test}.{n}", std::process::id()));
            // create_dir fails when the name exists, as mkdtemp's does.
            if fs::create_dir(&path).is_ok() {
                return Scratch { path };
            }
            assert!(n < 1000, "cannot create a scratch directory under {}", base.display());
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// `rmdir(scratch) == 0`: fails if the test left anything behind.
    fn finish(self) {
        fs::remove_dir(&self.path).expect("every test removes what it wrote");
    }
}

/// What the reader found (`reading_t`).
struct Reading {
    width: i32,
    height: i32,
    frames: i32,
    loops: i32,
    delay: i32,
    palette: [[u8; 3]; 256],
    /// Of the last frame read.
    indices: Vec<u8>,
    pixels: usize,
}

/// `read_lzw`. The C keeps its string table in static storage; every entry
/// it reads was written earlier in the same call, so a fresh table per call
/// is the same reader.
fn read_lzw(data: &[u8], min_code: i32, out: &mut [u8], expected: usize) -> bool {
    const MAX: usize = 4096;
    let mut entry = vec![[0u8; 64]; MAX];
    let mut entry_length = vec![0i32; MAX];
    let clear = 1i32 << min_code;
    let end = clear + 1;
    let mut width = min_code + 1;
    let mut next = clear + 2;
    let mut previous = -1i32;
    let length = data.len();
    let (mut at, mut bit) = (0usize, 0usize);
    let total = length * 8;

    for i in 0..clear as usize {
        entry[i][0] = i as u8;
        entry_length[i] = 1;
    }
    while bit + width as usize <= total {
        let byte = bit / 8;
        let offset = (bit % 8) as u32;
        let mut chunk = u32::from(data[byte]);
        if byte + 1 < length {
            chunk |= u32::from(data[byte + 1]) << 8;
        }
        if byte + 2 < length {
            chunk |= u32::from(data[byte + 2]) << 16;
        }
        let code = ((chunk >> offset) & ((1u32 << width) - 1)) as i32;
        bit += width as usize;

        if code == clear {
            width = min_code + 1;
            next = clear + 2;
            previous = -1;
            continue;
        }
        if code == end {
            break;
        }

        let source = if code < next && (code < clear || entry_length[code as usize] > 0) {
            code as usize
        } else if previous >= 0 {
            previous as usize
        } else {
            return false;
        };
        let source_length = entry_length[source] as usize;
        if at + source_length > expected {
            return false;
        }
        out[at..at + source_length].copy_from_slice(&entry[source][..source_length]);
        let written_at = at;
        at += source_length;
        // The self referring code repeats its own first byte.
        if code >= next && previous >= 0 {
            if at >= expected {
                return false;
            }
            out[at] = entry[source][0];
            at += 1;
        }

        if previous >= 0 && (next as usize) < MAX {
            let mut take = entry_length[previous as usize] as usize;
            if take > 63 {
                take = 63;
            }
            let copied = entry[previous as usize];
            entry[next as usize][..take].copy_from_slice(&copied[..take]);
            entry[next as usize][take] = out[written_at];
            entry_length[next as usize] = take as i32 + 1;
            next += 1;
            if next == (1 << width) && width < 12 {
                width += 1;
            }
        }
        previous = if code < next { code } else { next - 1 };
        if code >= clear + 2 && code >= next {
            previous = next - 1;
        }
    }
    at == expected
}

/// `read_gif`: handles exactly what gif.c emits, one global table, no
/// interlacing, no local tables. Like the C it reads into a zeroed buffer of
/// four megabytes, so a read past the end of a short file sees zeros.
fn read_gif(path: &Path) -> Option<Reading> {
    let mut file = vec![0u8; 1 << 22];
    let bytes = fs::read(path).ok()?;
    let length = bytes.len().min(file.len());
    file[..length].copy_from_slice(&bytes[..length]);
    if length < 14 || &file[..6] != b"GIF89a" {
        return None;
    }

    let mut out = Reading {
        width: i32::from(file[6]) | (i32::from(file[7]) << 8),
        height: i32::from(file[8]) | (i32::from(file[9]) << 8),
        frames: 0,
        loops: -1,
        delay: 0,
        palette: [[0; 3]; 256],
        indices: Vec::new(),
        pixels: 0,
    };
    if file[10] & 0x80 == 0 {
        return None;
    }
    let colours = 2 << (file[10] & 7);
    if colours != 256 {
        return None;
    }
    for (i, colour) in out.palette.iter_mut().enumerate() {
        colour.copy_from_slice(&file[13 + 3 * i..13 + 3 * i + 3]);
    }

    out.pixels = out.width as usize * out.height as usize;
    out.indices = vec![0u8; out.pixels];

    let mut at = 13 + 768;
    let mut payload = vec![0u8; 1 << 22];
    while at < length && file[at] != 0x3b {
        if file[at] == 0x21 {
            let label = file[at + 1];
            at += 2;
            if label == 0xf9 {
                out.delay = i32::from(file[at + 2]) | (i32::from(file[at + 3]) << 8);
                at += 1 + usize::from(file[at]);
                at += 1;
            } else if label == 0xff {
                let n = usize::from(file[at]);
                let netscape = &file[at + 1..at + 9] == b"NETSCAPE";
                at += 1 + n;
                if netscape {
                    out.loops = i32::from(file[at + 2]) | (i32::from(file[at + 3]) << 8);
                }
                while file[at] != 0 {
                    at += 1 + usize::from(file[at]);
                }
                at += 1;
            } else {
                while file[at] != 0 {
                    at += 1 + usize::from(file[at]);
                }
                at += 1;
            }
            continue;
        }
        if file[at] != 0x2c {
            return None;
        }
        at += 10;
        let min_code = i32::from(file[at]);
        at += 1;
        let mut payload_length = 0usize;
        while file[at] != 0 {
            let n = usize::from(file[at]);
            payload[payload_length..payload_length + n].copy_from_slice(&file[at + 1..at + 1 + n]);
            payload_length += n;
            at += 1 + n;
        }
        at += 1;
        let pixels = out.pixels;
        if !read_lzw(&payload[..payload_length], min_code, &mut out.indices, pixels) {
            return None;
        }
        out.frames += 1;
    }
    Some(out)
}

fn paint(frame: &mut Image, step: i32) {
    for y in 0..frame.height {
        for x in 0..frame.width {
            let at = (y as usize * frame.width as usize + x as usize) * 4;
            let p = &mut frame.pixels[at..at + 4];
            let on = ((x + step * 3) / 6 + y / 5) % 2 != 0;
            p[0] = if on { 240 } else { 20 };
            p[1] = if on { 90 } else { 20 };
            p[2] = if on { 40 } else { 30 };
            p[3] = 255;
        }
    }
}

#[test]
fn test_round_trip() {
    const W: i32 = 64;
    const H: i32 = 40;
    const N: i32 = 6;
    let scratch = Scratch::new("round_trip");
    let path = scratch.file("round_trip.gif");

    let mut writer = GifWriter::open(&path, W, H, 5).expect("open");
    let mut frame = Image::alloc(W, H).expect("alloc");
    for f in 0..N {
        paint(&mut frame, f);
        assert_eq!(writer.add_frame(&frame), Ok(()));
    }
    let closed = writer.close();
    assert_eq!(closed.status, Ok(()));
    assert!(closed.frames == N && closed.bytes > 0);

    // Read it back with a reader written from the spec, not from the encoder.
    let read = read_gif(&path).expect("read_gif");
    assert!(read.width == W && read.height == H);
    assert_eq!(read.frames, N);
    assert_eq!(read.delay, 5);
    assert_eq!(read.loops, 0); // Forever.

    // The last frame's pixels, through the palette, must be what went in: two
    // colours in, two colours out, in the right places.
    paint(&mut frame, N - 1);
    for i in 0..read.pixels {
        let want = &frame.pixels[i * 4..i * 4 + 4];
        let got = read.palette[usize::from(read.indices[i])];
        for c in 0..3 {
            let (got, want) = (i32::from(got[c]), i32::from(want[c]));
            assert!(got > want - 8 && got < want + 8);
        }
    }
    fs::remove_file(&path).expect("remove");
    scratch.finish();
}

#[test]
fn test_refusals() {
    let scratch = Scratch::new("refusals");
    let path = scratch.file("refuse.gif");

    // gif_open(NULL, path, ...) == GIF_ERR_ARGUMENT: the writer is what open
    // returns, so there is no out pointer to be NULL.
    // gif_open(&writer, NULL, ...) == GIF_ERR_ARGUMENT: a &Path cannot be
    // NULL. What both refusals guarantee, that a refused open touches
    // nothing, is checked on the refusal Rust can express: the arguments are
    // judged before the file is created.
    assert_eq!(GifWriter::open(&path, 0, 4, 5).err(), Some(GifError::Argument));
    assert!(!path.exists());
    assert_eq!(
        GifWriter::open(Path::new("/nowhere/at/all/x.gif"), 4, 4, 5).err(),
        Some(GifError::Io)
    );

    let mut writer = GifWriter::open(&path, 8, 8, 5).expect("open");
    // gif_add_frame(writer, NULL): the empty image is the Rust spelling of a
    // frame with no pixels.
    assert_eq!(writer.add_frame(&Image::empty()), Err(GifError::Argument));
    // A frame of the wrong size is not this animation's frame.
    let frame = Image::alloc(9, 8).expect("alloc");
    assert_eq!(writer.add_frame(&frame), Err(GifError::Argument));
    drop(frame);
    assert_eq!(writer.close().status, Ok(()));
    fs::remove_file(&path).expect("remove");

    for status in [Ok(()), Err(GifError::Argument), Err(GifError::Memory), Err(GifError::Io)] {
        let text = gif_status_string(status);
        assert_ne!(text, "unknown error");
    }
    scratch.finish();
}

/// Long runs of one colour are what this program's frames are made of, so
/// they had better compress.
#[test]
fn test_flat_frames_compress() {
    const W: i32 = 320;
    const H: i32 = 200;
    const N: i32 = 4;
    let scratch = Scratch::new("flat_frames");
    let path = scratch.file("flat.gif");

    let mut writer = GifWriter::open(&path, W, H, 4).expect("open");
    let mut frame = Image::alloc(W, H).expect("alloc");
    for p in frame.pixels.chunks_exact_mut(4) {
        p.copy_from_slice(&[18, 18, 24, 255]);
    }
    for _ in 0..N {
        assert_eq!(writer.add_frame(&frame), Ok(()));
    }
    let closed = writer.close();
    assert_eq!(closed.status, Ok(()));
    // Four flat frames of 64000 pixels each, in well under a tenth of that.
    assert!(closed.bytes < W as usize * H as usize / 10);
    fs::remove_file(&path).expect("remove");
    scratch.finish();
}
