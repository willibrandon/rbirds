//! Differential tests of `rbirds::image::png` against the pinned C `png.c`.
//!
//! Every comparison is exact: statuses, decoded pixels, encoded bytes and
//! transformed pixels must be what the reference computes on this target,
//! compiled by `tests/support/oracle.rs` with the canonical flags and driven
//! by `tools/oracle/png_oracle.c`. Inputs come from fixed-seed generators, so
//! a failure reproduces; the failing input is kept under `target/scratch`.
//!
//! The PNG variants are built here by an encoder of the test's own (scanline
//! packing, all five filters, stored, fixed and dynamic Huffman DEFLATE blocks,
//! mixed within one stream), so the decoders meet streams neither codec's
//! encoder writes. Debug builds run a subset of the larger corpora; release
//! runs them whole.

// tests/support/oracle.rs, shared and not this suite's to change, has a
// collapsible `if` that clippy 1.96 reports.
#[allow(clippy::collapsible_if)]
mod support;

use std::f64::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

use rbirds::image::png::{self, TintMode};
use rbirds::image::{Image, PngError};
use support::oracle;

/*============================== Plumbing ===================================*/

fn png_oracle() -> Option<PathBuf> {
    oracle::build("png_oracle", "png_oracle.c", &["png.c"])
}

/// Whether the full corpora run: always in release, and in debug when
/// `RBIRDS_FULL_CORPUS=1` (a debug run of the full corpora takes about half a
/// minute). Otherwise debug runs a subset.
fn full() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("RBIRDS_FULL_CORPUS").is_some_and(|v| v == "1")
}

/// A fixed-seed generator (xorshift64*), so every corpus reproduces.
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

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

/// The C's png_status_t numbering.
fn code<T>(status: &Result<T, PngError>) -> u8 {
    match status {
        Ok(_) => 0,
        Err(PngError::Memory) => 1,
        Err(PngError::Argument) => 2,
        Err(PngError::Truncated) => 3,
        Err(PngError::Signature) => 4,
        Err(PngError::Chunk) => 5,
        Err(PngError::Crc) => 6,
        Err(PngError::Unsupported) => 7,
        Err(PngError::Deflate) => 8,
    }
}

/// Records for the oracle's stdin, in its little endian format.
#[derive(Default)]
struct Records(Vec<u8>);

impl Records {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn i32(&mut self, value: i32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn f64(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    fn file(&mut self, data: &[u8]) {
        self.u32(data.len() as u32);
        self.0.extend_from_slice(data);
    }
    /// An image record; the empty image is sent as pixels NULL.
    fn image(&mut self, image: &Image) {
        self.i32(image.width);
        self.i32(image.height);
        self.u8(u8::from(!image.pixels.is_empty()));
        self.0.extend_from_slice(&image.pixels);
    }
}

/// The oracle's stdout.
struct Reader {
    data: Vec<u8>,
    at: usize,
}

impl Reader {
    fn u8(&mut self) -> u8 {
        let value = self.data[self.at];
        self.at += 1;
        value
    }
    fn u32(&mut self) -> u32 {
        let bytes = self.bytes(4);
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn bytes(&mut self, length: usize) -> Vec<u8> {
        let bytes = self.data[self.at..self.at + length].to_vec();
        self.at += length;
        bytes
    }
    fn image(&mut self) -> Image {
        let width = self.i32();
        let height = self.i32();
        let present = self.u8() != 0;
        let pixels =
            if present { self.bytes(width as usize * height as usize * 4) } else { Vec::new() };
        Image { width, height, pixels }
    }
    /// A status, and the image that follows it on success.
    fn image_result(&mut self) -> (u8, Option<Image>) {
        let status = self.u8();
        (status, if status == 0 { Some(self.image()) } else { None })
    }
    fn finish(&self) {
        assert_eq!(self.at, self.data.len(), "oracle wrote more than was read back");
    }
}

fn run(exe: &Path, command: &str, records: &Records) -> Reader {
    let output = oracle::run(exe, &[command], &records.0);
    Reader { data: output.stdout, at: 0 }
}

/// Where two images first differ, for a failure message.
fn first_difference(c: &Image, rust: &Image) -> String {
    if (c.width, c.height) != (rust.width, rust.height) {
        return format!("C is {}x{}, Rust is {}x{}", c.width, c.height, rust.width, rust.height);
    }
    if c.pixels.len() != rust.pixels.len() {
        return format!("C has {} bytes, Rust {}", c.pixels.len(), rust.pixels.len());
    }
    for (i, (a, b)) in c.pixels.iter().zip(&rust.pixels).enumerate() {
        if a != b {
            let pixel = i / 4;
            let (x, y) = (pixel % c.width.max(1) as usize, pixel / c.width.max(1) as usize);
            return format!(
                "first difference at x={x} y={y} channel {}: C {a}, Rust {b} (C {:?}, Rust {:?})",
                i % 4,
                &c.pixels[pixel * 4..pixel * 4 + 4],
                &rust.pixels[pixel * 4..pixel * 4 + 4]
            );
        }
    }
    "identical".to_owned()
}

/// Keeps a failing input for diagnosis, then fails.
fn fail_with(label: &str, files: &[(&str, &[u8])], why: String) -> ! {
    let scratch = oracle::Scratch::new(&format!("png_oracle.{}", sanitize(label)));
    for (name, data) in files {
        let _ = fs::write(scratch.file(name), data);
    }
    panic!("{label}: {why}");
}

fn sanitize(label: &str) -> String {
    label.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(80).collect()
}

/// A status and optional image from each side must agree exactly.
fn compare_result(
    label: &str,
    c: (u8, Option<Image>),
    rust: (u8, Option<Image>),
    input: &[(&str, &[u8])],
) {
    if c.0 != rust.0 {
        fail_with(label, input, format!("status: C {}, Rust {}", c.0, rust.0));
    }
    if let (Some(c_image), Some(rust_image)) = (&c.1, &rust.1)
        && c_image != rust_image
    {
        fail_with(label, input, first_difference(c_image, rust_image));
    }
}

fn rust_result(result: Result<Image, PngError>) -> (u8, Option<Image>) {
    let status = code(&result);
    (status, result.ok())
}

/*=========================== Image corpora =================================*/

const PATTERNS: u32 = 10;

/// A test image of one of several shapes: what a compressor or resampler
/// meets in sprites and screens, and what it rarely meets.
fn pattern(width: i32, height: i32, kind: u32, rng: &mut Rng) -> Image {
    let mut image = Image::alloc(width, height).expect("alloc");
    let (w, h) = (width as usize, height as usize);
    let mut run_left = 0u64;
    let mut run_colour = [0u8; 4];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let p = &mut image.pixels[i * 4..i * 4 + 4];
            match kind {
                0 => p.copy_from_slice(&[40, 40, 40, 255]), // flat
                1 => {
                    // noise, alpha included
                    for v in p.iter_mut() {
                        *v = rng.byte();
                    }
                }
                2 => {
                    // stripes
                    let on = (i / 7) % 2 == 1;
                    p.copy_from_slice(&[
                        if on { 255 } else { 0 },
                        128,
                        if on { 0 } else { 255 },
                        255,
                    ]);
                }
                3 => p.copy_from_slice(&[(i % 256) as u8, (i / 256) as u8, 90, 255]), // gradient
                4 => {
                    // colour noise over an alpha ramp
                    p[0] = rng.byte();
                    p[1] = (x * 255 / w.max(2).saturating_sub(1).max(1)) as u8;
                    p[2] = (y * 31) as u8;
                    p[3] = ((x + y) * 255 / (w + h).max(1)) as u8;
                }
                5 => {
                    // a sprite: a soft opaque blob on transparency
                    let cx = w as f64 / 2.0 - 0.5;
                    let cy = h as f64 / 2.0 - 0.5;
                    let r = (w.min(h) as f64 / 2.5).max(0.8);
                    let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();
                    if d < r {
                        let a = (255.0 * (1.0 - d / r)).min(255.0) as u8;
                        p.copy_from_slice(&[200, (x * 8) as u8, 30, a.max(1)]);
                    } else {
                        p.copy_from_slice(&[rng.byte(), rng.byte(), 0, 0]);
                    }
                }
                6 => {
                    // runs of random length and colour, some repeated
                    if run_left == 0 {
                        run_left = 1 + rng.below(40);
                        if !rng.chance(3) {
                            run_colour = [rng.byte(), rng.byte(), rng.byte(), rng.byte() | 1];
                        }
                    }
                    run_left -= 1;
                    p.copy_from_slice(&run_colour);
                }
                7 => {
                    // checker, half transparent squares
                    let on = ((x / 3) + (y / 3)) % 2 == 0;
                    p.copy_from_slice(if on { &[250, 10, 10, 255] } else { &[10, 250, 10, 128] });
                }
                8 => {
                    // one three byte prefix everywhere: long hash chains
                    p.copy_from_slice(&[1, 2, 3, rng.byte() & 7]);
                }
                _ => {
                    // two prefixes that differ in one bit of the first byte,
                    // hashed apart only by that bit: long chains of near
                    // misses, where the chain limit decides what is found
                    let first = if rng.chance(2) { 0x05 } else { 0x15 };
                    p.copy_from_slice(&[first, 2, 3, rng.byte() & 3]);
                }
            }
        }
    }
    image
}

/// Runs that can only be matched at one distance: a compressible background
/// of bytes 0 and 1 (so the fixed Huffman stream, not the stored fallback, is
/// what comes out) with three runs of other bytes, each copied `distance`
/// bytes further on in the raw scanlines, whose rows are 4001 bytes (a filter
/// byte and 1000 pixels).
fn windowed(distance: usize, rng: &mut Rng) -> Image {
    const ROW: usize = 4000;
    let mut image = Image::alloc(1000, 10).expect("alloc");
    for v in image.pixels.iter_mut() {
        *v = rng.byte() & 1;
    }
    let raw_offset = |i: usize| i + i / ROW + 1;
    for from in [400usize, ROW + 12, ROW + 2000] {
        let target = raw_offset(from) + distance;
        let row = (target - 1) / (ROW + 1);
        let to = target - 1 - row;
        assert_eq!(raw_offset(to), target);
        assert!(to % ROW + 160 <= ROW && to < image.pixels.len());
        let run: Vec<u8> = (0..160).map(|_| 2 + rng.byte() % 254).collect();
        image.pixels[from..from + 160].copy_from_slice(&run);
        image.pixels[to..to + 160].copy_from_slice(&run);
    }
    image
}

fn sprite_png() -> Vec<u8> {
    fs::read(oracle::repository().join("assets/sprite.png")).expect("assets/sprite.png")
}

/*============================ Encoding (a, b) ==============================*/

/// Rust encode bytes == C encode bytes over a corpus, and each side decodes
/// the other's file back to the original pixels.
#[test]
fn encoded_bytes_match_and_cross_decode() {
    let Some(exe) = png_oracle() else { return };
    let mut rng = Rng::new(1);
    let mut corpus: Vec<(String, Image)> = Vec::new();

    let widths: &[i32] = if full() {
        &[1, 2, 3, 4, 5, 7, 8, 13, 16, 17, 31, 32, 33, 50, 64, 70]
    } else {
        &[1, 2, 5, 13, 32, 70]
    };
    let heights: &[i32] = if full() { &[1, 2, 3, 5, 8, 16, 25, 49, 50] } else { &[1, 3, 16, 50] };
    for &w in widths {
        for &h in heights {
            for kind in 0..PATTERNS {
                corpus.push((format!("{w}x{h} pattern {kind}"), pattern(w, h, kind, &mut rng)));
            }
        }
    }
    // Stored fallbacks over several blocks, long matches, the window edge and
    // the chain limit, and the embedded sprite.
    corpus.push(("128x128 noise, two stored blocks".into(), pattern(128, 128, 1, &mut rng)));
    corpus.push(("181x181 noise, three stored blocks".into(), pattern(181, 181, 1, &mut rng)));
    corpus.push(("300x200 gradient".into(), pattern(300, 200, 3, &mut rng)));
    corpus.push(("256x256 runs".into(), pattern(256, 256, 6, &mut rng)));
    corpus.push(("400x300 flat".into(), pattern(400, 300, 0, &mut rng)));
    for distance in [32767, 32768, 32769, 32772] {
        corpus.push((
            format!("runs only matched at distance {distance}"),
            windowed(distance, &mut rng),
        ));
    }
    corpus.push(("200x200 hash chains".into(), pattern(200, 200, 8, &mut rng)));
    corpus.push(("200x200 near-miss chains".into(), pattern(200, 200, 9, &mut rng)));
    corpus.push(("600x600 sprite".into(), png::decode(&sprite_png()).expect("sprite")));

    let mut records = Records::default();
    for (_, image) in &corpus {
        records.image(image);
    }
    // What the C refuses: pixels NULL, with and without a size.
    let refused = [Image::empty(), Image { width: 3, height: 2, pixels: Vec::new() }];
    for image in &refused {
        records.image(image);
    }
    let mut reader = run(&exe, "encode", &records);

    let mut rust_files = Records::default();
    for (label, image) in &corpus {
        let c_status = reader.u8();
        let rust = png::encode(image);
        if c_status != code(&rust) {
            fail_with(label, &[], format!("encode status: C {c_status}, Rust {}", code(&rust)));
        }
        let length = reader.u32() as usize;
        let c_bytes = reader.bytes(length);
        let rust_bytes = rust.expect("encoded");
        if c_bytes != rust_bytes {
            let at = c_bytes.iter().zip(&rust_bytes).position(|(a, b)| a != b);
            fail_with(
                label,
                &[("c.png", &c_bytes), ("rust.png", &rust_bytes)],
                format!(
                    "encoded bytes differ: C {} bytes, Rust {}, first difference at {at:?}",
                    c_bytes.len(),
                    rust_bytes.len()
                ),
            );
        }
        // C-encode -> Rust-decode.
        let back = png::decode(&c_bytes).expect("Rust decodes the C file");
        if &back != image {
            fail_with(label, &[("c.png", &c_bytes)], first_difference(image, &back));
        }
        rust_files.file(&rust_bytes);
    }
    for image in &refused {
        let c_status = reader.u8();
        assert_eq!(c_status, 2, "C refuses NULL pixels as an argument error");
        assert_eq!(code(&png::encode(image)), c_status);
    }
    reader.finish();

    // Rust-encode -> C-decode.
    let mut reader = run(&exe, "decode", &rust_files);
    for (label, image) in &corpus {
        let (status, decoded) = reader.image_result();
        assert_eq!(status, 0, "{label}: C cannot decode the Rust file");
        let decoded = decoded.expect("image");
        if &decoded != image {
            fail_with(
                label,
                &[],
                format!("C decode of Rust file: {}", first_difference(image, &decoded)),
            );
        }
    }
    reader.finish();
}

/// The embedded sprite, a file from a real encoder with ancillary chunks,
/// decodes to the same pixels on both sides.
#[test]
fn sprite_decodes_identically() {
    let Some(exe) = png_oracle() else { return };
    let mut files = vec![("assets/sprite.png".to_owned(), sprite_png())];
    let matrix = oracle::reference_dir().join("matrix.png");
    if let Ok(data) = fs::read(&matrix) {
        files.push(("reference matrix.png".to_owned(), data));
    }
    let mut records = Records::default();
    for (_, data) in &files {
        records.file(data);
    }
    let mut reader = run(&exe, "decode", &records);
    for (label, data) in &files {
        let c = reader.image_result();
        assert_eq!(c.0, 0, "{label} decodes in C");
        let rust = rust_result(png::decode(data));
        let c_image = c.1.as_ref().expect("image");
        assert_eq!((c_image.width, c_image.height), (600, 600));
        compare_result(label, c, rust, &[("input.png", data)]);
    }
    reader.finish();
}

/*===================== A PNG writer of the test's own ======================*/

/// DEFLATE and zlib, written from RFC 1950 and 1951 for these tests only.
mod deflate {
    use super::Rng;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// What the encoder has written on this thread: block kinds, length and
    /// distance codes, code length codes. The corpus is checked against it.
    #[derive(Default)]
    pub struct Seen {
        pub blocks: [bool; 3],
        pub lengths: [bool; 29],
        pub distances: [bool; 30],
        pub code_lengths: [bool; 19],
    }

    thread_local! {
        pub static SEEN: RefCell<Seen> = RefCell::new(Seen::default());
    }

    fn seen(record: impl FnOnce(&mut Seen)) {
        SEEN.with(|s| record(&mut s.borrow_mut()));
    }

    pub const LENGTH_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    pub const LENGTH_EXTRA: [u8; 29] =
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    pub const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    pub const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];
    pub const ORDER: [usize; 19] =
        [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

    /// A little endian bit stream.
    #[derive(Default)]
    pub struct Bits {
        pub out: Vec<u8>,
        acc: u64,
        count: u32,
    }

    impl Bits {
        pub fn put(&mut self, value: u32, count: u32) {
            self.acc |= (u64::from(value) & ((1u64 << count) - 1)) << self.count;
            self.count += count;
            while self.count >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.count -= 8;
            }
        }
        /// A Huffman code, most significant bit first.
        pub fn code(&mut self, code: u32, length: u8) {
            for i in (0..u32::from(length)).rev() {
                self.put((code >> i) & 1, 1);
            }
        }
        pub fn align(&mut self) {
            if self.count > 0 {
                self.put(0, 8 - self.count);
            }
        }
        pub fn finish(mut self) -> Vec<u8> {
            self.align();
            self.out
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub enum Token {
        Literal(u8),
        Match { length: usize, distance: usize },
    }

    impl Token {
        pub fn size(self) -> usize {
            match self {
                Token::Literal(_) => 1,
                Token::Match { length, .. } => length,
            }
        }
    }

    /// LZ77 over the whole input, with some matches cut short or passed up at
    /// random so every length and distance code turns up.
    pub fn tokenize(data: &[u8], rng: &mut Rng) -> Vec<Token> {
        let mut chains: HashMap<[u8; 3], Vec<usize>> = HashMap::new();
        let mut tokens = Vec::new();
        let mut at = 0;
        while at < data.len() {
            let mut best = (0usize, 0usize);
            if at + 3 <= data.len()
                && let Some(list) = chains.get(&[data[at], data[at + 1], data[at + 2]])
            {
                for &from in list.iter().rev().take(48) {
                    let distance = at - from;
                    if distance > 32768 {
                        break;
                    }
                    let mut length = 0;
                    while length < 258
                        && at + length < data.len()
                        && data[from + length] == data[at + length]
                    {
                        length += 1;
                    }
                    if length > best.0 || (length == best.0 && rng.chance(4)) {
                        best = (length, distance);
                    }
                }
            }
            let mut size = 1;
            if best.0 >= 3 && !rng.chance(10) {
                let mut length = best.0;
                if rng.chance(3) {
                    length = 3 + rng.below((length - 2) as u64) as usize;
                }
                tokens.push(Token::Match { length, distance: best.1 });
                size = length;
            } else {
                tokens.push(Token::Literal(data[at]));
            }
            for here in at..at + size {
                if here + 3 <= data.len() {
                    chains
                        .entry([data[here], data[here + 1], data[here + 2]])
                        .or_default()
                        .push(here);
                }
            }
            at += size;
        }
        tokens
    }

    fn length_symbol(length: usize) -> (usize, u32, u32) {
        let mut index = 28;
        while usize::from(LENGTH_BASE[index]) > length {
            index -= 1;
        }
        // 258 has a code of its own; 227 + 31 would be the other way to say it.
        if length == 258 {
            index = 28;
        }
        (
            257 + index,
            u32::from(LENGTH_EXTRA[index]),
            (length - usize::from(LENGTH_BASE[index])) as u32,
        )
    }

    fn distance_symbol(distance: usize) -> (usize, u32, u32) {
        let mut index = 29;
        while usize::from(DIST_BASE[index]) > distance {
            index -= 1;
        }
        (index, u32::from(DIST_EXTRA[index]), (distance - usize::from(DIST_BASE[index])) as u32)
    }

    /// Code lengths of a Huffman code over `freq`, none longer than `limit`
    /// (frequencies are halved until it fits).
    pub fn huffman_lengths(freq: &[u32], limit: u8) -> Vec<u8> {
        let mut weights: Vec<u64> = freq.iter().map(|&f| u64::from(f)).collect();
        loop {
            let lengths = unlimited_lengths(&weights);
            if lengths.iter().all(|&l| l <= limit) {
                return lengths;
            }
            for w in weights.iter_mut() {
                if *w > 0 {
                    *w = w.div_ceil(2);
                }
            }
        }
    }

    type Node = (u64, Option<(usize, usize)>, usize);

    fn unlimited_lengths(weights: &[u64]) -> Vec<u8> {
        let mut lengths = vec![0u8; weights.len()];
        let used: Vec<usize> = (0..weights.len()).filter(|&s| weights[s] > 0).collect();
        if used.len() == 1 {
            lengths[used[0]] = 1;
        }
        if used.len() < 2 {
            return lengths;
        }
        // (weight, children, symbol); leaves first.
        let mut nodes: Vec<Node> = used.iter().map(|&s| (weights[s], None, s)).collect();
        let mut live: Vec<usize> = (0..nodes.len()).collect();
        while live.len() > 1 {
            live.sort_by_key(|&n| (nodes[n].0, n));
            let (a, b) = (live[0], live[1]);
            nodes.push((nodes[a].0 + nodes[b].0, Some((a, b)), 0));
            live.drain(..2);
            live.push(nodes.len() - 1);
        }
        let mut stack = vec![(live[0], 0u8)];
        while let Some((n, depth)) = stack.pop() {
            match nodes[n].1 {
                Some((a, b)) => {
                    stack.push((a, depth + 1));
                    stack.push((b, depth + 1));
                }
                None => lengths[nodes[n].2] = depth,
            }
        }
        lengths
    }

    /// Canonical codes for the lengths (RFC 1951 3.2.2).
    pub fn canonical(lengths: &[u8]) -> Vec<u32> {
        let mut count = [0u32; 16];
        for &l in lengths {
            if l > 0 {
                count[usize::from(l)] += 1;
            }
        }
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for bits in 1..16 {
            code = (code + count[bits - 1]) << 1;
            next[bits] = code;
        }
        lengths
            .iter()
            .map(|&l| {
                if l == 0 {
                    0
                } else {
                    let c = next[usize::from(l)];
                    next[usize::from(l)] += 1;
                    c
                }
            })
            .collect()
    }

    fn fixed_lengths() -> (Vec<u8>, Vec<u8>) {
        let mut lit = vec![8u8; 288];
        lit[144..256].fill(9);
        lit[256..280].fill(7);
        (lit, vec![5u8; 30])
    }

    fn put_tokens(bits: &mut Bits, tokens: &[Token], lit: (&[u8], &[u32]), dist: (&[u8], &[u32])) {
        for &token in tokens {
            match token {
                Token::Literal(byte) => {
                    bits.code(lit.1[usize::from(byte)], lit.0[usize::from(byte)])
                }
                Token::Match { length, distance } => {
                    let (symbol, extra, value) = length_symbol(length);
                    seen(|s| s.lengths[symbol - 257] = true);
                    bits.code(lit.1[symbol], lit.0[symbol]);
                    bits.put(value, extra);
                    let (symbol, extra, value) = distance_symbol(distance);
                    seen(|s| s.distances[symbol] = true);
                    bits.code(dist.1[symbol], dist.0[symbol]);
                    bits.put(value, extra);
                }
            }
        }
        bits.code(lit.1[256], lit.0[256]);
    }

    pub fn stored_block(bits: &mut Bits, data: &[u8], last: bool) {
        seen(|s| s.blocks[0] = true);
        bits.put(u32::from(last), 1);
        bits.put(0, 2);
        bits.align();
        let n = data.len() as u32;
        bits.out.extend_from_slice(&[n as u8, (n >> 8) as u8, !n as u8, (!n >> 8) as u8]);
        bits.out.extend_from_slice(data);
    }

    pub fn fixed_block(bits: &mut Bits, tokens: &[Token], last: bool) {
        let (lit, dist) = fixed_lengths();
        let (lit_codes, dist_codes) = (canonical(&lit), canonical(&dist));
        seen(|s| s.blocks[1] = true);
        bits.put(u32::from(last), 1);
        bits.put(1, 2);
        put_tokens(bits, tokens, (&lit, &lit_codes), (&dist, &dist_codes));
    }

    /// Run length coding of the code lengths with 16, 17 and 18.
    fn run_lengths(sequence: &[u8]) -> Vec<(usize, u32, u32)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < sequence.len() {
            let value = sequence[i];
            let mut run = 1;
            while i + run < sequence.len() && sequence[i + run] == value {
                run += 1;
            }
            if value == 0 && run >= 3 {
                let n = run.min(138);
                if n >= 11 {
                    out.push((18, 7, (n - 11) as u32));
                } else {
                    out.push((17, 3, (n - 3) as u32));
                }
                i += n;
            } else if value != 0 && run >= 4 {
                out.push((usize::from(value), 0, 0));
                let mut left = run - 1;
                while left >= 3 {
                    let n = left.min(6);
                    out.push((16, 2, (n - 3) as u32));
                    left -= n;
                }
                i += run - left;
            } else {
                out.push((usize::from(value), 0, 0));
                i += 1;
            }
        }
        out
    }

    /// A dynamic Huffman block. `flat` uses the fixed code's lengths sent as
    /// a dynamic table; `spare` adds unused codes where the code has room.
    pub fn dynamic_block(bits: &mut Bits, tokens: &[Token], last: bool, flat: bool, spare: bool) {
        let mut lit_freq = vec![0u32; 286];
        let mut dist_freq = vec![0u32; 30];
        lit_freq[256] = 1;
        for &token in tokens {
            match token {
                Token::Literal(byte) => lit_freq[usize::from(byte)] += 1,
                Token::Match { length, distance } => {
                    lit_freq[length_symbol(length).0] += 1;
                    dist_freq[distance_symbol(distance).0] += 1;
                }
            }
        }
        let (mut lit, mut dist) = if flat {
            let (lit, dist) = fixed_lengths();
            (lit[..286].to_vec(), dist)
        } else {
            (huffman_lengths(&lit_freq, 15), huffman_lengths(&dist_freq, 15))
        };
        if spare {
            // A symbol no token uses, given a code only where the code is
            // incomplete (a single distance code of one bit).
            if dist.iter().filter(|&&l| l > 0).count() == 1 {
                let unused = dist.iter().position(|&l| l == 0).expect("unused distance");
                dist[unused] = 1;
            }
            if lit.iter().filter(|&&l| l > 0).count() == 1 {
                lit[0] = 1;
            }
        }
        let hlit = (lit.iter().rposition(|&l| l > 0).unwrap_or(0) + 1).max(257);
        let hdist = (dist.iter().rposition(|&l| l > 0).unwrap_or(0) + 1).max(1);
        let mut sequence = lit[..hlit].to_vec();
        sequence.extend_from_slice(&dist[..hdist]);
        let rle = run_lengths(&sequence);
        let mut cl_freq = vec![0u32; 19];
        for &(symbol, _, _) in &rle {
            cl_freq[symbol] += 1;
            seen(|s| s.code_lengths[symbol] = true);
        }
        seen(|s| s.blocks[2] = true);
        let cl = huffman_lengths(&cl_freq, 7);
        let cl_codes = canonical(&cl);
        let hclen = (ORDER.iter().rposition(|&s| cl[s] > 0).unwrap_or(0) + 1).max(4);

        bits.put(u32::from(last), 1);
        bits.put(2, 2);
        bits.put((hlit - 257) as u32, 5);
        bits.put((hdist - 1) as u32, 5);
        bits.put((hclen - 4) as u32, 4);
        for &symbol in &ORDER[..hclen] {
            bits.put(u32::from(cl[symbol]), 3);
        }
        for &(symbol, extra, value) in &rle {
            bits.code(cl_codes[symbol], cl[symbol]);
            bits.put(value, extra);
        }
        let (lit_codes, dist_codes) = (canonical(&lit), canonical(&dist));
        put_tokens(bits, tokens, (&lit, &lit_codes), (&dist, &dist_codes));
    }

    #[derive(Clone, Copy, Debug)]
    pub enum Compression {
        /// Stored blocks of at most this many bytes.
        Stored(usize),
        Fixed,
        Dynamic,
        /// Blocks of every kind in one stream, matches reaching back across
        /// them, empty blocks included.
        Mixed,
    }

    pub fn deflate(data: &[u8], compression: Compression, rng: &mut Rng) -> Vec<u8> {
        let mut bits = Bits::default();
        match compression {
            Compression::Stored(block) => {
                let mut at = 0;
                loop {
                    let n = (data.len() - at).min(block);
                    stored_block(&mut bits, &data[at..at + n], at + n == data.len());
                    at += n;
                    if at >= data.len() {
                        break;
                    }
                }
            }
            Compression::Fixed => fixed_block(&mut bits, &tokenize(data, rng), true),
            Compression::Dynamic => {
                let flat = rng.chance(5);
                let spare = rng.chance(2);
                dynamic_block(&mut bits, &tokenize(data, rng), true, flat, spare)
            }
            Compression::Mixed => {
                let tokens = tokenize(data, rng);
                let mut at_token = 0;
                let mut at_byte = 0;
                while at_token < tokens.len() {
                    let take = 1 + rng.below((tokens.len() / 3 + 1) as u64) as usize;
                    let end = (at_token + take).min(tokens.len());
                    let group = &tokens[at_token..end];
                    let size: usize = group.iter().map(|t| t.size()).sum();
                    let last = end == tokens.len();
                    if rng.chance(6) {
                        // An empty block first, of either kind.
                        if rng.chance(2) {
                            stored_block(&mut bits, &[], false);
                        } else {
                            fixed_block(&mut bits, &[], false);
                        }
                    }
                    match rng.below(3) {
                        0 => stored_block(&mut bits, &data[at_byte..at_byte + size], last),
                        1 => fixed_block(&mut bits, group, last),
                        _ => dynamic_block(&mut bits, group, last, false, rng.chance(2)),
                    }
                    at_token = end;
                    at_byte += size;
                }
                if tokens.is_empty() {
                    stored_block(&mut bits, &[], true);
                }
            }
        }
        bits.finish()
    }

    pub fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + u32::from(byte)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    /// Valid zlib headers: several window sizes and levels.
    pub const HEADERS: [[u8; 2]; 5] =
        [[0x78, 0x01], [0x78, 0x9c], [0x78, 0xda], [0x68, 0x05], [0x08, 0x1d]];

    pub fn zlib(data: &[u8], compression: Compression, rng: &mut Rng) -> Vec<u8> {
        let mut out = HEADERS[rng.below(HEADERS.len() as u64) as usize].to_vec();
        out.extend(deflate(data, compression, rng));
        out.extend_from_slice(&adler(data).to_be_bytes());
        out
    }
}

use deflate::Compression;

fn crc_of(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &byte in data {
        c ^= u32::from(byte);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(c & 1));
        }
    }
    !c
}

fn put_chunk(png: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    png.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(body);
    let crc = crc_of(&png[start..]);
    png.extend_from_slice(&crc.to_be_bytes());
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

fn ihdr(width: u32, height: u32, depth: u8, color_type: u8, interlace: u8) -> Vec<u8> {
    let mut body = width.to_be_bytes().to_vec();
    body.extend_from_slice(&height.to_be_bytes());
    body.extend_from_slice(&[depth, color_type, 0, 0, interlace]);
    body
}

fn channels_of(color_type: u8) -> usize {
    match color_type {
        2 => 3,
        4 => 2,
        6 => 4,
        _ => 1,
    }
}

#[derive(Clone, Copy, Debug)]
enum Filters {
    /// Row y of pass p takes (y + p) % 5.
    Cycle,
    /// Every row the same filter.
    All(u8),
    /// A random filter a row.
    Random,
}

#[derive(Clone, Copy, Debug)]
enum Split {
    One,
    Halves,
    /// Chunks of this many bytes, empty IDAT chunks between some.
    Small(usize),
}

/// A picture as the file holds it, and how to write it.
#[derive(Clone, Debug)]
struct Variant {
    label: String,
    width: u32,
    height: u32,
    depth: u8,
    color_type: u8,
    interlaced: bool,
    palette: Option<Vec<u8>>,
    trns: Option<Vec<u8>>,
    samples: Vec<u32>,
    filters: Filters,
    compression: Compression,
    split: Split,
    /// Chunk order: H IHDR, P PLTE, T tRNS, D the data, X tEXt, G gAMA,
    /// U an unknown ancillary chunk, C an unknown critical one. IEND closes.
    order: &'static str,
}

const ADAM7: [[usize; 4]; 7] = [
    [0, 0, 8, 8],
    [4, 0, 8, 8],
    [0, 4, 4, 8],
    [2, 0, 4, 4],
    [0, 2, 2, 4],
    [1, 0, 2, 2],
    [0, 1, 1, 2],
];

fn predict(filter: u8, a: i32, b: i32, c: i32) -> i32 {
    match filter {
        1 => a,
        2 => b,
        3 => (a + b) / 2,
        4 => {
            let p = a + b - c;
            let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
            if pa <= pb && pa <= pc {
                a
            } else if pb <= pc {
                b
            } else {
                c
            }
        }
        _ => 0,
    }
}

/// The filtered scanlines of every pass.
fn raster(v: &Variant, rng: &mut Rng) -> Vec<u8> {
    let channels = channels_of(v.color_type);
    let depth = usize::from(v.depth);
    let bits = channels * depth;
    let unit = (bits / 8).max(1);
    let passes: &[[usize; 4]] = if v.interlaced { &ADAM7 } else { &[[0, 0, 1, 1]] };
    let (width, height) = (v.width as usize, v.height as usize);
    let mut raw = Vec::new();

    for (p, &[x0, y0, dx, dy]) in passes.iter().enumerate() {
        let pw = if width > x0 { (width - x0).div_ceil(dx) } else { 0 };
        let ph = if height > y0 { (height - y0).div_ceil(dy) } else { 0 };
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = (pw * bits).div_ceil(8);
        let mut above = vec![0u8; stride];
        for y in 0..ph {
            let mut line = vec![0u8; stride];
            for x in 0..pw {
                let pixel = (y0 + y * dy) * width + x0 + x * dx;
                for c in 0..channels {
                    let value = v.samples[pixel * channels + c];
                    let index = x * channels + c;
                    match depth {
                        16 => {
                            line[index * 2] = (value >> 8) as u8;
                            line[index * 2 + 1] = value as u8;
                        }
                        8 => line[index] = value as u8,
                        _ => {
                            let bit = index * depth;
                            line[bit / 8] |= (value << (8 - depth - bit % 8)) as u8;
                        }
                    }
                }
            }
            let filter = match v.filters {
                Filters::Cycle => ((y + p) % 5) as u8,
                Filters::All(f) => f,
                Filters::Random => rng.below(5) as u8,
            };
            raw.push(filter);
            for i in 0..stride {
                let a = if i >= unit { i32::from(line[i - unit]) } else { 0 };
                let c = if i >= unit { i32::from(above[i - unit]) } else { 0 };
                raw.push((i32::from(line[i]) - predict(filter, a, i32::from(above[i]), c)) as u8);
            }
            above = line;
        }
    }
    raw
}

fn build(v: &Variant, seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let raw = raster(v, &mut rng);
    let zlib = deflate::zlib(&raw, v.compression, &mut rng);
    let mut png = SIGNATURE.to_vec();
    let header = ihdr(v.width, v.height, v.depth, v.color_type, u8::from(v.interlaced));
    let order = if v.order.contains('H') { v.order.to_owned() } else { format!("H{}", v.order) };
    for o in order.chars() {
        match o {
            'H' => put_chunk(&mut png, b"IHDR", &header),
            'P' => {
                if let Some(palette) = &v.palette {
                    put_chunk(&mut png, b"PLTE", palette);
                }
            }
            'T' => {
                if let Some(trns) = &v.trns {
                    put_chunk(&mut png, b"tRNS", trns);
                }
            }
            'X' => put_chunk(&mut png, b"tEXt", b"Comment\0rbirds"),
            'G' => put_chunk(&mut png, b"gAMA", &45455u32.to_be_bytes()),
            'U' => put_chunk(&mut png, b"zzZz", &[1, 2, 3]),
            'C' => put_chunk(&mut png, b"ZZZZ", &[9; 9]),
            'D' => match v.split {
                Split::One => put_chunk(&mut png, b"IDAT", &zlib),
                Split::Halves => {
                    put_chunk(&mut png, b"IDAT", &zlib[..zlib.len() / 2]);
                    put_chunk(&mut png, b"IDAT", &zlib[zlib.len() / 2..]);
                }
                Split::Small(n) => {
                    for (i, piece) in zlib.chunks(n).enumerate() {
                        if i % 3 == 1 {
                            put_chunk(&mut png, b"IDAT", &[]);
                        }
                        put_chunk(&mut png, b"IDAT", piece);
                    }
                }
            },
            _ => unreachable!("chunk letter {o}"),
        }
    }
    put_chunk(&mut png, b"IEND", &[]);
    png
}

/// What a decoder must give back, from the specification alone (the C
/// test's expected_rgba).
fn expected_rgba(v: &Variant) -> Vec<u8> {
    let channels = channels_of(v.color_type);
    let top = (1u32 << v.depth) - 1;
    let keyed = v.trns.is_some() && (v.color_type == 0 || v.color_type == 2);
    let mut key = [0u32; 3];
    if keyed {
        let trns = v.trns.as_ref().expect("key");
        for c in 0..channels {
            key[c] = ((u32::from(trns[2 * c]) << 8) | u32::from(trns[2 * c + 1])) & top;
        }
    }
    let count = v.width as usize * v.height as usize;
    let mut rgba = vec![0u8; count * 4];
    for i in 0..count {
        let s = &v.samples[i * channels..(i + 1) * channels];
        let q = &mut rgba[i * 4..i * 4 + 4];
        let mut e = [0u8; 4];
        for c in 0..channels {
            e[c] = if v.depth == 16 { (s[c] >> 8) as u8 } else { (s[c] * 255 / top) as u8 };
        }
        match v.color_type {
            0 => {
                q[..3].fill(e[0]);
                q[3] = if keyed && s[0] == key[0] { 0 } else { 255 };
            }
            2 => {
                q[..3].copy_from_slice(&e[..3]);
                q[3] = if keyed && s[..3] == key[..] { 0 } else { 255 };
            }
            3 => {
                let palette = v.palette.as_ref().expect("palette");
                let index = s[0] as usize;
                if index < palette.len() / 3 {
                    q[..3].copy_from_slice(&palette[index * 3..index * 3 + 3]);
                    q[3] = match &v.trns {
                        Some(trns) if index < trns.len() => trns[index],
                        _ => 255,
                    };
                }
            }
            4 => {
                q[..3].fill(e[0]);
                q[3] = e[1];
            }
            _ => q.copy_from_slice(&e),
        }
    }
    rgba
}

/// Samples below `limit`, both ends in; `repeat` copies whole rows from far
/// back so long and distant matches exist.
fn samples(
    width: u32,
    height: u32,
    channels: usize,
    limit: u32,
    repeat: bool,
    rng: &mut Rng,
) -> Vec<u32> {
    let row = width as usize * channels;
    let count = row * height as usize;
    let mut s: Vec<u32> = (0..count).map(|_| (rng.next() >> 20) as u32 % limit).collect();
    if repeat {
        for y in 1..height as usize {
            if rng.chance(2) {
                let from = rng.below(y as u64) as usize;
                let copied: Vec<u32> = s[from * row..(from + 1) * row].to_vec();
                s[y * row..(y + 1) * row].copy_from_slice(&copied);
            }
        }
    }
    s[0] = 0;
    s[count - 1] = limit - 1;
    s
}

/// Every supported color type and depth, plain and Adam7, under every filter
/// and block kind, split and ordered several ways.
fn variant_corpus() -> Vec<Variant> {
    const KINDS: [(u8, u8); 15] = [
        (0, 1),
        (0, 2),
        (0, 4),
        (0, 8),
        (0, 16),
        (3, 1),
        (3, 2),
        (3, 4),
        (3, 8),
        (2, 8),
        (2, 16),
        (4, 8),
        (4, 16),
        (6, 8),
        (6, 16),
    ];
    let sizes: &[(u32, u32)] =
        if full() { &[(1, 1), (5, 3), (13, 7), (33, 9)] } else { &[(1, 1), (13, 7)] };
    let compressions =
        [Compression::Stored(65535), Compression::Fixed, Compression::Dynamic, Compression::Mixed];
    let filters = [
        Filters::Cycle,
        Filters::All(0),
        Filters::All(1),
        Filters::All(2),
        Filters::All(3),
        Filters::All(4),
        Filters::Random,
    ];
    let splits = [Split::One, Split::Halves, Split::Small(7)];
    let orders = ["PTD", "XPTD", "HXGPUTD", "PTDX", "CPTD", "PTUDG"];
    let mut rng = Rng::new(3);
    let mut out = Vec::new();
    let mut n = 0usize;

    for &(color_type, depth) in &KINDS {
        for &(width, height) in sizes {
            for interlaced in [false, true] {
                for compression in compressions {
                    for with_trns in [false, true] {
                        let channels = channels_of(color_type);
                        let (palette, trns, limit) = match color_type {
                            3 => {
                                let entries = if n.is_multiple_of(3) {
                                    1usize << depth
                                } else {
                                    (1usize << depth).div_ceil(2).max(1)
                                };
                                let palette: Vec<u8> =
                                    (0..entries * 3).map(|_| rng.byte()).collect();
                                let trns = with_trns.then(|| {
                                    (0..1 + rng.below(entries as u64) as usize)
                                        .map(|_| rng.byte())
                                        .collect()
                                });
                                // Indices past PLTE sometimes: they read as holes.
                                let limit = if n % 4 == 1 { 1u32 << depth } else { entries as u32 };
                                (Some(palette), trns, limit)
                            }
                            0 | 2 => {
                                let limit = 1u32 << depth;
                                let trns = with_trns.then(|| {
                                    let mut key = Vec::new();
                                    for _ in 0..channels {
                                        // Bits above the depth are masked off.
                                        let sample = ((rng.next() >> 20) as u32 % limit)
                                            | if depth < 16 && rng.chance(2) {
                                                0x100 << (depth % 8)
                                            } else {
                                                0
                                            };
                                        key.extend_from_slice(&(sample as u16).to_be_bytes());
                                    }
                                    key
                                });
                                (
                                    if color_type == 2 && n.is_multiple_of(5) {
                                        Some(vec![1, 2, 3, 4, 5, 6])
                                    } else {
                                        None
                                    },
                                    trns,
                                    limit,
                                )
                            }
                            // tRNS next to an alpha channel is ignored.
                            _ => (None, with_trns.then(|| vec![0, 1]), 1u32 << depth),
                        };
                        let mut s = samples(width, height, channels, limit, false, &mut rng);
                        if let (Some(key), 0 | 2) = (&trns, color_type) {
                            // Put some pixels on the key.
                            let top = (1u32 << depth) - 1;
                            for pixel in (0..(width * height) as usize).step_by(3) {
                                for c in 0..channels {
                                    s[pixel * channels + c] = ((u32::from(key[2 * c]) << 8)
                                        | u32::from(key[2 * c + 1]))
                                        & top;
                                }
                            }
                        }
                        out.push(Variant {
                            label: format!("type {color_type} depth {depth} {width}x{height} interlaced {interlaced} {compression:?} tRNS {with_trns} #{n}"),
                            width,
                            height,
                            depth,
                            color_type,
                            interlaced,
                            palette,
                            trns,
                            samples: s,
                            filters: filters[n % filters.len()],
                            compression,
                            split: splits[n % splits.len()],
                            order: orders[n % orders.len()],
                        });
                        n += 1;
                    }
                }
            }
        }
    }

    // Large enough for distant and long matches, every length and distance
    // code, and stored blocks split small.
    let big: &[(u8, u8, u32, u32)] = if full() {
        &[(6, 8, 64, 128), (2, 16, 40, 48), (0, 2, 200, 90), (3, 8, 120, 100), (4, 16, 50, 70)]
    } else {
        &[(6, 8, 64, 128), (0, 2, 200, 90)]
    };
    for &(color_type, depth, width, height) in big {
        for compression in [
            Compression::Stored(1000),
            Compression::Fixed,
            Compression::Dynamic,
            Compression::Mixed,
        ] {
            for interlaced in [false, true] {
                let channels = channels_of(color_type);
                let limit = if color_type == 3 { 256 } else { 1u32 << depth };
                let palette = (color_type == 3).then(|| (0..768).map(|_| rng.byte()).collect());
                out.push(Variant {
                    label: format!("big type {color_type} depth {depth} {width}x{height} interlaced {interlaced} {compression:?} #{n}"),
                    width,
                    height,
                    depth,
                    color_type,
                    interlaced,
                    palette,
                    trns: None,
                    samples: samples(width, height, channels, limit, true, &mut rng),
                    filters: if n.is_multiple_of(2) { Filters::All(0) } else { Filters::Random },
                    compression,
                    split: splits[n % splits.len()],
                    order: "PD",
                });
                n += 1;
            }
        }
    }
    out
}

/// Every supported PNG variant: C and Rust agree, and both give the pixels
/// the specification does.
#[test]
fn every_png_variant_decodes_identically() {
    let Some(exe) = png_oracle() else { return };
    let corpus = variant_corpus();
    deflate::SEEN.with(|s| *s.borrow_mut() = deflate::Seen::default());
    let files: Vec<Vec<u8>> =
        corpus.iter().enumerate().map(|(i, v)| build(v, 1000 + i as u64)).collect();
    // The full corpus reaches every block kind and every length, distance and
    // code length code the format has; the debug subset says what it misses.
    deflate::SEEN.with(|s| {
        let s = s.borrow();
        let missing = |seen: &[bool]| (0..seen.len()).filter(|&i| !seen[i]).collect::<Vec<_>>();
        let gaps = (
            missing(&s.blocks),
            missing(&s.lengths),
            missing(&s.distances),
            missing(&s.code_lengths),
        );
        if full() {
            assert!(gaps.0.is_empty(), "block kinds never written: {:?}", gaps.0);
            assert!(gaps.1.is_empty(), "length codes (257 +) never written: {:?}", gaps.1);
            assert!(gaps.2.is_empty(), "distance codes never written: {:?}", gaps.2);
            assert!(gaps.3.is_empty(), "code length codes never written: {:?}", gaps.3);
        } else {
            eprintln!(
                "debug subset leaves unwritten (blocks, lengths, distances, code lengths): {gaps:?}"
            );
        }
    });

    let mut records = Records::default();
    for file in &files {
        records.file(file);
    }
    let mut reader = run(&exe, "decode", &records);
    for (v, file) in corpus.iter().zip(&files) {
        let c = reader.image_result();
        let rust = rust_result(png::decode(file));
        if c.0 != 0 {
            fail_with(
                &v.label,
                &[("input.png", file)],
                format!("C refuses a valid file: status {}", c.0),
            );
        }
        let want = expected_rgba(v);
        let c_image = c.1.as_ref().expect("image");
        if c_image.pixels != want {
            let expected = Image { width: v.width as i32, height: v.height as i32, pixels: want };
            fail_with(
                &v.label,
                &[("input.png", file)],
                format!("C against the specification: {}", first_difference(&expected, c_image)),
            );
        }
        compare_result(&v.label, c, rust, &[("input.png", file)]);
    }
    reader.finish();
}

/*========================= Malformed input (d) =============================*/

/// A 4x3 RGBA file around the given zlib stream.
fn rgba_4x3(zlib: &[u8]) -> Vec<u8> {
    let mut png = SIGNATURE.to_vec();
    put_chunk(&mut png, b"IHDR", &ihdr(4, 3, 8, 6, 0));
    put_chunk(&mut png, b"IDAT", zlib);
    put_chunk(&mut png, b"IEND", &[]);
    png
}

/// The raster of a 4x3 RGBA image: 3 rows of a filter byte and 16 bytes.
fn raster_4x3(rng: &mut Rng) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..3 {
        raw.push(rng.below(5) as u8);
        for _ in 0..16 {
            raw.push(rng.byte());
        }
    }
    raw
}

fn wrap_zlib(deflated: &[u8], adler: Option<u32>) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    z.extend_from_slice(deflated);
    if let Some(a) = adler {
        z.extend_from_slice(&a.to_be_bytes());
    }
    z
}

/// Hand made faults at every level: DEFLATE blocks, the zlib wrapper, chunk
/// structure and order, the header's limits and the signature.
fn malformed_corpus() -> Vec<(String, Vec<u8>)> {
    use deflate::{Bits, Token};
    let mut rng = Rng::new(9);
    let raw = raster_4x3(&mut rng);
    let adler = deflate::adler(&raw);
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |label: &str, png: Vec<u8>| out.push((label.to_owned(), png));

    // DEFLATE.
    let mut b = Bits::default();
    b.put(1, 1);
    b.put(3, 2);
    add("block type 3", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
    for (label, len, nlen) in [
        ("stored LEN/NLEN mismatch", 51u16, 0u16),
        ("stored LEN too long", 60, !60u16),
        ("stored LEN too short", 50, !50u16),
    ] {
        let mut d = vec![1u8, len as u8, (len >> 8) as u8, nlen as u8, (nlen >> 8) as u8];
        d.extend_from_slice(&raw);
        add(label, rgba_4x3(&wrap_zlib(&d, Some(adler))));
    }
    add("stored header cut", rgba_4x3(&wrap_zlib(&[1, 51, 0], None)));
    add("stored header cut after nlen byte", rgba_4x3(&[0x78, 0x01, 1, 51, 0, 0xcc]));
    for symbol in [286u32, 287] {
        let mut b = Bits::default();
        b.put(1, 1);
        b.put(1, 2);
        b.code(0xc0 + symbol - 280, 8);
        b.put(0, 7);
        add(
            &format!("fixed literal/length symbol {symbol}"),
            rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))),
        );
    }
    for symbol in [30u32, 31] {
        let mut b = Bits::default();
        b.put(1, 1);
        b.put(1, 2);
        b.code(0x30, 8); // literal 0
        b.code(1, 7); // length 3
        b.code(symbol, 5);
        b.put(0, 16);
        add(
            &format!("fixed distance symbol {symbol}"),
            rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))),
        );
    }
    {
        let mut b = Bits::default();
        deflate::fixed_block(
            &mut b,
            &[Token::Literal(0), Token::Match { length: 3, distance: 2 }],
            true,
        );
        add("distance past the start", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        let mut b = Bits::default();
        let tokens: Vec<Token> = raw.iter().map(|&x| Token::Literal(x)).collect();
        deflate::fixed_block(&mut b, &tokens[..40], true);
        add("final block too early", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        let mut b = Bits::default();
        deflate::fixed_block(&mut b, &tokens, false);
        add("no final block", rgba_4x3(&wrap_zlib(&b.finish(), None)));
        let mut b = Bits::default();
        deflate::fixed_block(&mut b, &tokens, true);
        let mut d = b.finish();
        d.truncate(d.len() - 3);
        add("stream cut inside a block", rgba_4x3(&wrap_zlib(&d, None)));
        let mut b = Bits::default();
        deflate::fixed_block(&mut b, &tokens[..50], false);
        deflate::fixed_block(&mut b, &[Token::Literal(1), Token::Literal(2)], true);
        add("one byte over, across blocks", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        let mut b = Bits::default();
        let mut long = tokens.clone();
        long.push(Token::Match { length: 10, distance: 5 });
        deflate::fixed_block(&mut b, &long, true);
        add("match past the limit", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        // A good stream with the Adler at every length and value.
        let mut b = Bits::default();
        deflate::fixed_block(&mut b, &tokens, true);
        let d = b.finish();
        for keep in 0..=4 {
            let mut z = wrap_zlib(&d, Some(adler));
            z.truncate(z.len() - 4 + keep);
            add(&format!("Adler cut to {keep} bytes"), rgba_4x3(&z));
        }
        add("wrong Adler", rgba_4x3(&wrap_zlib(&d, Some(adler ^ 1))));
        let mut z = wrap_zlib(&d, Some(adler));
        z.extend_from_slice(&[0xde, 0xad]);
        add("garbage after the Adler", rgba_4x3(&z));
        for (label, header) in [
            ("CM 7", [0x77u8, 0x01]),
            ("FDICT", [0x78, 0x20]),
            ("FCHECK wrong", [0x78, 0x02]),
            ("CINFO 15", [0xf8, 0x00]),
            ("CINFO 0", [0x08, 0x1d]),
        ] {
            let mut z = header.to_vec();
            z.extend_from_slice(&d);
            z.extend_from_slice(&adler.to_be_bytes());
            add(&format!("zlib header {label}"), rgba_4x3(&z));
        }
        for n in 0..6 {
            add(&format!("zlib stream of {n} bytes"), rgba_4x3(&wrap_zlib(&d, Some(adler))[..n]));
        }
    }
    // Dynamic tables.
    let dynamic_header = |hlit: u32, hdist: u32, hclen: u32, lengths: &[u32]| {
        let mut b = Bits::default();
        b.put(1, 1);
        b.put(2, 2);
        b.put(hlit, 5);
        b.put(hdist, 5);
        b.put(hclen, 4);
        for &l in lengths {
            b.put(l, 3);
        }
        b
    };
    for (label, hlit, hdist) in
        [("HLIT 287", 30u32, 0u32), ("HLIT 288", 31, 0), ("HDIST 31", 0, 30), ("HDIST 32", 0, 31)]
    {
        let mut b = dynamic_header(hlit, hdist, 15, &[3; 19]);
        b.put(0, 32);
        add(&format!("dynamic {label}"), rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
    }
    // Code length code: symbols 16, 17, 18 and 0..15 all of length 5 (19
    // codes of 5 bits: an incomplete code).
    {
        let mut b = dynamic_header(0, 0, 15, &[5; 19]);
        let cl = deflate::canonical(&[5; 19]);
        b.code(cl[16], 5); // repeat with nothing before it
        b.put(0, 2);
        b.put(0, 32);
        add("dynamic repeat first", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        let mut b = dynamic_header(0, 0, 15, &[5; 19]);
        for _ in 0..3 {
            b.code(cl[18], 5);
            b.put(127, 7); // 138 zeros each: past 258
        }
        b.put(0, 32);
        add("dynamic repeat past the end", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        let mut b = dynamic_header(0, 0, 15, &[5; 19]);
        b.code(cl[18], 5);
        b.put(127, 7); // 138 zeros
        b.code(cl[18], 5);
        b.put(108, 7); // 119 zeros: 257 lengths, 256 included, all zero
        b.code(cl[1], 5); // the distance code
        b.put(0, 32);
        add("dynamic without an end of block code", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        // Every literal/length code of length 1: over-subscribed.
        let mut b = dynamic_header(0, 0, 15, &[5; 19]);
        for _ in 0..258 {
            b.code(cl[1], 5);
        }
        for bit in 0..64 {
            b.put(bit & 1, 1);
        }
        add("dynamic over-subscribed code", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        // End of block alone, as 00: a stream of ones matches no code at any
        // length.
        let mut b = dynamic_header(0, 0, 15, &[5; 19]);
        b.code(cl[18], 5);
        b.put(127, 7); // 0..137 zero
        b.code(cl[18], 5);
        b.put(107, 7); // 138..255 zero (118)
        b.code(cl[2], 5); // 256: length 2
        b.code(cl[0], 5); // the one distance: no code
        for _ in 0..24 {
            b.put(1, 1);
        }
        add("dynamic code with no symbol", rgba_4x3(&wrap_zlib(&b.finish(), Some(adler))));
        let mut b = dynamic_header(0, 0, 15, &[5; 19]);
        b.code(cl[16], 5);
        let mut d = b.finish();
        d.truncate(d.len() - 1);
        add("dynamic header cut", rgba_4x3(&wrap_zlib(&d, None)));
    }

    // Chunks.
    let mut rng = Rng::new(10);
    let mut b = Bits::default();
    let tokens: Vec<Token> = raw.iter().map(|&x| Token::Literal(x)).collect();
    deflate::fixed_block(&mut b, &tokens, true);
    let good_zlib = wrap_zlib(&b.finish(), Some(adler));
    let good = rgba_4x3(&good_zlib);
    add("good", good.clone());
    let chunked = |parts: &[(&[u8; 4], Vec<u8>)]| {
        let mut png = SIGNATURE.to_vec();
        for (kind, body) in parts {
            put_chunk(&mut png, kind, body);
        }
        png
    };
    let h = ihdr(4, 3, 8, 6, 0);
    add(
        "IHDR of 12 bytes",
        chunked(&[(b"IHDR", h[..12].to_vec()), (b"IDAT", good_zlib.clone()), (b"IEND", vec![])]),
    );
    add(
        "IHDR of 14 bytes",
        chunked(&[
            (b"IHDR", [&h[..], &[0]].concat()),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "IHDR twice",
        chunked(&[
            (b"IHDR", h.clone()),
            (b"IHDR", h.clone()),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add("no IHDR", chunked(&[(b"IDAT", good_zlib.clone()), (b"IEND", vec![])]));
    add(
        "IDAT before IHDR",
        chunked(&[(b"IDAT", good_zlib.clone()), (b"IHDR", h.clone()), (b"IEND", vec![])]),
    );
    add(
        "PLTE before IHDR",
        chunked(&[
            (b"PLTE", vec![1, 2, 3]),
            (b"IHDR", h.clone()),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "tRNS before IHDR",
        chunked(&[
            (b"tRNS", vec![1, 2]),
            (b"IHDR", h.clone()),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "text before IHDR",
        chunked(&[
            (b"tEXt", b"a\0b".to_vec()),
            (b"IHDR", h.clone()),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "unknown critical chunk",
        chunked(&[
            (b"IHDR", h.clone()),
            (b"ABCD", vec![7; 5]),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "IEND before IDAT",
        chunked(&[(b"IHDR", h.clone()), (b"IEND", vec![]), (b"IDAT", good_zlib.clone())]),
    );
    add("no IEND", chunked(&[(b"IHDR", h.clone()), (b"IDAT", good_zlib.clone())]));
    add(
        "IEND with a body",
        chunked(&[(b"IHDR", h.clone()), (b"IDAT", good_zlib.clone()), (b"IEND", vec![1, 2, 3])]),
    );
    add("no IDAT", chunked(&[(b"IHDR", h.clone()), (b"IEND", vec![])]));
    add("empty IDAT only", chunked(&[(b"IHDR", h.clone()), (b"IDAT", vec![]), (b"IEND", vec![])]));
    add(
        "IDAT around another chunk",
        chunked(&[
            (b"IHDR", h.clone()),
            (b"IDAT", good_zlib[..10].to_vec()),
            (b"tEXt", b"x\0y".to_vec()),
            (b"IDAT", good_zlib[10..].to_vec()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "tRNS twice on RGBA",
        chunked(&[
            (b"IHDR", h.clone()),
            (b"tRNS", vec![0, 1]),
            (b"tRNS", vec![0, 1]),
            (b"IDAT", good_zlib.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "tRNS after IDAT on RGBA",
        chunked(&[
            (b"IHDR", h.clone()),
            (b"IDAT", good_zlib.clone()),
            (b"tRNS", vec![0, 1]),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "PLTE twice on RGBA",
        chunked(&[
            (b"IHDR", h.clone()),
            (b"PLTE", vec![1, 2]),
            (b"PLTE", vec![]),
            (b"IDAT", good_zlib.clone()),
            (b"PLTE", vec![3; 800]),
            (b"IEND", vec![]),
        ]),
    );
    let mut after = good.clone();
    after.extend_from_slice(b"anything at all after IEND");
    add("bytes after IEND", after);
    let mut trailing = good[..good.len() - 12].to_vec();
    trailing.extend_from_slice(&[0, 0, 0, 0, b'I', b'E', b'N']);
    add("seven bytes where IEND goes", trailing);
    let mut bad_crc = good.clone();
    let last = bad_crc.len() - 1;
    bad_crc[last] ^= 1;
    add("IEND CRC wrong", bad_crc);
    for (label, length) in [
        ("0x80000000", 0x8000_0000u32),
        ("0x7fffffff", 0x7fff_ffff),
        ("past the end", 1000),
        ("one past", 0),
    ] {
        let mut png = good.clone();
        let at = 8 + 25; // the IDAT length field
        let value = if length == 0 { good_zlib.len() as u32 + 1 } else { length };
        png[at..at + 4].copy_from_slice(&value.to_be_bytes());
        add(&format!("IDAT length {label}"), png);
    }
    // Header limits: the raster is never there, so nothing is allocated.
    for (w, hh, depth, ct) in [
        (0u32, 1u32, 8u8, 6u8),
        (1, 0, 8, 6),
        (16384, 1, 8, 6),
        (16385, 1, 8, 6),
        (1, 16385, 8, 6),
        (16384, 4096, 16, 6),
        (16384, 4097, 16, 6),
        (8192, 8192, 1, 0),
        (8193, 8192, 1, 0),
        (0x8000_0000, 1, 8, 6),
        (0xffff_ffff, 0xffff_ffff, 8, 6),
    ] {
        add(
            &format!("IHDR {w}x{hh} depth {depth} type {ct}"),
            chunked(&[
                (b"IHDR", ihdr(w, hh, depth, ct, 0)),
                (b"IDAT", good_zlib.clone()),
                (b"IEND", vec![]),
            ]),
        );
    }
    for (label, index, value) in [
        ("compression 1", 10usize, 1u8),
        ("filter method 1", 11, 1),
        ("interlace 2", 12, 2),
        ("interlace 255", 12, 255),
    ] {
        let mut body = h.clone();
        body[index] = value;
        add(
            &format!("IHDR {label}"),
            chunked(&[(b"IHDR", body), (b"IDAT", good_zlib.clone()), (b"IEND", vec![])]),
        );
    }
    // Palette faults.
    let ph = ihdr(4, 3, 2, 3, 0);
    let mut praw = Vec::new();
    for _ in 0..3 {
        praw.push(0);
        praw.push(rng.byte());
    }
    let pz = {
        let mut z = vec![0x78, 0x01];
        let mut b = Bits::default();
        deflate::stored_block(&mut b, &praw, true);
        z.extend(b.finish());
        z.extend_from_slice(&deflate::adler(&praw).to_be_bytes());
        z
    };
    add(
        "palette image, no PLTE",
        chunked(&[(b"IHDR", ph.clone()), (b"IDAT", pz.clone()), (b"IEND", vec![])]),
    );
    add(
        "palette image, tRNS without PLTE",
        chunked(&[
            (b"IHDR", ph.clone()),
            (b"tRNS", vec![1]),
            (b"PLTE", vec![1; 12]),
            (b"IDAT", pz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "palette image, PLTE of 2 entries",
        chunked(&[
            (b"IHDR", ph.clone()),
            (b"PLTE", vec![5; 6]),
            (b"IDAT", pz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "palette image, PLTE of 256 at 2 bits",
        chunked(&[
            (b"IHDR", ph.clone()),
            (b"PLTE", vec![5; 768]),
            (b"tRNS", vec![9; 256]),
            (b"IDAT", pz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "palette image, tRNS longer than PLTE",
        chunked(&[
            (b"IHDR", ph.clone()),
            (b"PLTE", vec![5; 6]),
            (b"tRNS", vec![9; 3]),
            (b"IDAT", pz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "palette image, empty tRNS",
        chunked(&[
            (b"IHDR", ph.clone()),
            (b"PLTE", vec![5; 6]),
            (b"tRNS", vec![]),
            (b"IDAT", pz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    let gray_raw: Vec<u8> = (0..15).map(|i| if i % 5 == 0 { 0 } else { rng.byte() }).collect();
    let gz = {
        let mut z = vec![0x78, 0x01];
        let mut b = Bits::default();
        deflate::stored_block(&mut b, &gray_raw, true);
        z.extend(b.finish());
        z.extend_from_slice(&deflate::adler(&gray_raw).to_be_bytes());
        z
    };
    add(
        "gray image, PLTE ignored",
        chunked(&[
            (b"IHDR", ihdr(4, 3, 8, 0, 0)),
            (b"PLTE", vec![1; 5]),
            (b"IDAT", gz.clone()),
            (b"PLTE", vec![]),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "gray image, tRNS key",
        chunked(&[
            (b"IHDR", ihdr(4, 3, 8, 0, 0)),
            (b"tRNS", vec![0x7f, gray_raw[1]]),
            (b"IDAT", gz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    add(
        "gray key of 4 bytes",
        chunked(&[
            (b"IHDR", ihdr(2, 3, 8, 0, 0)),
            (b"tRNS", vec![0, 1, 0, 2]),
            (b"IDAT", pz.clone()),
            (b"IEND", vec![]),
        ]),
    );
    // Signature.
    add("empty file", vec![]);
    add("seven bytes of signature", SIGNATURE[..7].to_vec());
    add("signature only", SIGNATURE.to_vec());
    let mut sig = good.clone();
    sig[1] = b'p';
    add("signature wrong", sig);
    out
}

/// Faults written on purpose: statuses and any pixels agree.
#[test]
fn malformed_files_are_refused_identically() {
    let Some(exe) = png_oracle() else { return };
    let corpus = malformed_corpus();
    compare_decodes(&exe, &corpus);
}

fn compare_decodes(exe: &Path, corpus: &[(String, Vec<u8>)]) {
    let mut records = Records::default();
    for (_, file) in corpus {
        records.file(file);
    }
    let mut reader = run(exe, "decode", &records);
    for (label, file) in corpus {
        let c = reader.image_result();
        let rust = rust_result(png::decode(file));
        compare_result(label, c, rust, &[("input.png", file)]);
    }
    reader.finish();
}

/// Small files each cut at every byte.
fn small_files() -> Vec<(String, Vec<u8>)> {
    let mut rng = Rng::new(21);
    let mut files = Vec::new();
    files.push((
        "encoded 5x3 runs".to_owned(),
        png::encode(&pattern(5, 3, 6, &mut rng)).expect("encode"),
    ));
    files.push((
        "encoded 9x4 noise".to_owned(),
        png::encode(&pattern(9, 4, 1, &mut rng)).expect("encode"),
    ));
    let corpus = variant_corpus();
    let picks: [fn(&Variant) -> bool; 4] = [
        |v: &Variant| {
            v.color_type == 3
                && v.depth == 1
                && v.interlaced
                && v.trns.is_some()
                && matches!(v.compression, Compression::Stored(_))
        },
        |v: &Variant| {
            v.color_type == 2 && v.depth == 16 && matches!(v.compression, Compression::Dynamic)
        },
        |v: &Variant| {
            v.color_type == 0
                && v.depth == 2
                && v.interlaced
                && matches!(v.compression, Compression::Mixed)
        },
        |v: &Variant| v.color_type == 4 && v.depth == 8 && v.order.contains('X'),
    ];
    for pick in picks {
        let (i, v) =
            corpus.iter().enumerate().rfind(|(_, v)| v.width > 1 && pick(v)).expect("variant");
        files.push((v.label.clone(), build(v, 1000 + i as u64)));
    }
    files
}

/// Truncation at every byte of a few small files.
#[test]
fn truncations_are_refused_identically() {
    let Some(exe) = png_oracle() else { return };
    let mut corpus = Vec::new();
    for (label, file) in small_files() {
        for cut in 0..=file.len() {
            corpus.push((format!("{label} cut to {cut} of {}", file.len()), file[..cut].to_vec()));
        }
    }
    compare_decodes(&exe, &corpus);
}

/// A file's chunks, for rearranging.
fn chunks_of(file: &[u8]) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut at = 8;
    while at + 12 <= file.len() {
        let length =
            u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        chunks.push(file[at..at + 12 + length].to_vec());
        at += 12 + length;
    }
    chunks
}

fn fix_crc(chunk: &mut [u8]) {
    let end = chunk.len() - 4;
    let crc = crc_of(&chunk[4..end]);
    chunk[end..].copy_from_slice(&crc.to_be_bytes());
}

fn assemble(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut file = SIGNATURE.to_vec();
    for chunk in chunks {
        file.extend_from_slice(chunk);
    }
    file
}

/// One reproducible mutation of a well formed file.
fn mutate(file: &[u8], rng: &mut Rng) -> (String, Vec<u8>) {
    let mut chunks = chunks_of(file);
    let pick_chunk = |rng: &mut Rng, chunks: &[Vec<u8>], kind: Option<&[u8; 4]>| -> Option<usize> {
        let candidates: Vec<usize> =
            (0..chunks.len()).filter(|&i| kind.is_none_or(|k| &chunks[i][4..8] == k)).collect();
        if candidates.is_empty() {
            None
        } else {
            Some(candidates[rng.below(candidates.len() as u64) as usize])
        }
    };
    let kind = rng.below(14);
    let mut out = file.to_vec();
    let what;
    match kind {
        0 => {
            let at = 8 + rng.below((file.len() - 8) as u64) as usize;
            let bit = rng.below(8);
            out[at] ^= 1 << bit;
            what = format!("bit {bit} of byte {at} flipped");
        }
        1 => {
            let at = rng.below(file.len() as u64) as usize;
            out[at] = rng.byte();
            what = format!("byte {at} set");
        }
        2 | 3 | 4 | 11 | 12 | 13 => {
            let target = if kind == 2 { None } else { Some(b"IDAT") };
            let Some(i) = pick_chunk(rng, &chunks, target) else {
                return ("unchanged".into(), out);
            };
            let chunk = &mut chunks[i];
            let body = chunk.len() - 12;
            match kind {
                2 | 3 if body > 0 => {
                    let at = 8 + rng.below(body as u64) as usize;
                    if kind == 2 {
                        chunk[at] = rng.byte();
                    } else {
                        chunk[at] ^= 1 << rng.below(8);
                    }
                    what = format!("chunk {i} byte {at} changed, CRC fixed");
                }
                4 if body > 0 => {
                    let n = 1 + rng.below(8);
                    for _ in 0..n {
                        let at = 8 + rng.below(body as u64) as usize;
                        chunk[at] = rng.byte();
                    }
                    what = format!("chunk {i}: {n} bytes changed, CRC fixed");
                }
                11 => {
                    let at = 8 + rng.below(body as u64 + 1) as usize;
                    let n = 1 + rng.below(6) as usize;
                    let inserted: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
                    chunk.splice(at..at, inserted);
                    let length = (chunk.len() - 12) as u32;
                    chunk[..4].copy_from_slice(&length.to_be_bytes());
                    what = format!("chunk {i}: {n} bytes inserted at {at}, length and CRC fixed");
                }
                12 if body > 0 => {
                    let at = 8 + rng.below(body as u64) as usize;
                    let n = (1 + rng.below(6) as usize).min(8 + body - at);
                    chunk.drain(at..at + n);
                    let length = (chunk.len() - 12) as u32;
                    chunk[..4].copy_from_slice(&length.to_be_bytes());
                    what = format!("chunk {i}: {n} bytes removed at {at}, length and CRC fixed");
                }
                13 if body >= 4 => {
                    let at = chunk.len() - 4 - 1 - rng.below(4) as usize;
                    chunk[at] = rng.byte();
                    what = format!("chunk {i}: checksum area byte {at} set, CRC fixed");
                }
                _ => what = format!("chunk {i} left alone"),
            }
            fix_crc(chunk);
            out = assemble(&chunks);
        }
        5 => {
            let Some(i) = pick_chunk(rng, &chunks, None) else { return ("unchanged".into(), out) };
            let body = (chunks[i].len() - 12) as u32;
            let length = match rng.below(6) {
                0 => body.wrapping_add(1),
                1 => body.wrapping_sub(1),
                2 => 0,
                3 => 0x7fff_ffff,
                4 => 0x8000_0000,
                _ => rng.next() as u32,
            };
            chunks[i][..4].copy_from_slice(&length.to_be_bytes());
            what = format!("chunk {i} length set to {length:#x}");
            out = assemble(&chunks);
        }
        6 => {
            let (a, b) =
                (rng.below(chunks.len() as u64) as usize, rng.below(chunks.len() as u64) as usize);
            chunks.swap(a, b);
            what = format!("chunks {a} and {b} swapped");
            out = assemble(&chunks);
        }
        7 => {
            let i = rng.below(chunks.len() as u64) as usize;
            let at = rng.below(chunks.len() as u64 + 1) as usize;
            let copy = chunks[i].clone();
            chunks.insert(at, copy);
            what = format!("chunk {i} repeated at {at}");
            out = assemble(&chunks);
        }
        8 => {
            let i = rng.below(chunks.len() as u64) as usize;
            chunks.remove(i);
            what = format!("chunk {i} removed");
            out = assemble(&chunks);
        }
        9 => {
            let cut = rng.below(file.len() as u64) as usize;
            out.truncate(cut);
            let n = rng.below(20) as usize;
            for _ in 0..n {
                out.push(rng.byte());
            }
            what = format!("cut to {cut}, {n} random bytes after");
        }
        _ => {
            // IHDR fields, CRC fixed.
            let Some(i) = pick_chunk(rng, &chunks, Some(b"IHDR")) else {
                return ("unchanged".into(), out);
            };
            let chunk = &mut chunks[i];
            let field = rng.below(6);
            match field {
                0 => chunk[8..12].copy_from_slice(&(rng.below(40) as u32).to_be_bytes()),
                1 => chunk[12..16].copy_from_slice(&(rng.below(40) as u32).to_be_bytes()),
                2 => chunk[16] = [1, 2, 4, 8, 16, 3, 0][rng.below(7) as usize],
                3 => chunk[17] = [0, 2, 3, 4, 6, 1][rng.below(6) as usize],
                4 => chunk[20] ^= 1,
                _ => chunk[8 + rng.below(13) as usize] = rng.byte(),
            }
            fix_crc(chunk);
            what = format!("IHDR field {field} changed, CRC fixed");
            out = assemble(&chunks);
        }
    }
    (what, out)
}

/// A fixed-seed mutation corpus: bit flips, bytes, chunk lengths, checksums,
/// IDAT contents with the CRC fixed so the damage reaches the inflater,
/// reordered, repeated and dropped chunks.
#[test]
fn mutations_are_handled_identically() {
    let Some(exe) = png_oracle() else { return };
    let per_file = if full() { 2500 } else { 300 };
    let mut rng = Rng::new(77);
    let mut corpus = Vec::new();
    let mut bases = small_files();
    bases.push((
        "encoded 24x16 sprite".to_owned(),
        png::encode(&pattern(24, 16, 5, &mut rng)).expect("encode"),
    ));
    for (label, file) in &bases {
        for n in 0..per_file {
            let (what, mutated) = mutate(file, &mut rng);
            corpus.push((format!("{label} mutation {n}: {what}"), mutated));
        }
    }
    compare_decodes(&exe, &corpus);
}

/*============================ Transforms (e) ===============================*/

fn transform_sources() -> Vec<(String, Image)> {
    let mut rng = Rng::new(5);
    let mut sources = Vec::new();
    let sizes: &[(i32, i32)] = if full() {
        &[(1, 1), (1, 5), (5, 1), (2, 2), (3, 7), (16, 16), (17, 9), (32, 32), (40, 25), (64, 64)]
    } else {
        &[(1, 1), (3, 7), (17, 9), (32, 32)]
    };
    for (n, &(w, h)) in sizes.iter().enumerate() {
        for kind in [1u32, 4, 5, (n as u32) % PATTERNS] {
            sources.push((format!("{w}x{h} pattern {kind}"), pattern(w, h, kind, &mut rng)));
        }
    }
    sources
}

const ANGLES: [f64; 16] = [
    0.0,
    1e-9,
    0.1,
    -0.1,
    PI / 6.0,
    PI / 4.0,
    PI / 2.0,
    PI,
    3.0 * PI / 2.0,
    2.0 * PI,
    -2.5,
    7.0,
    100.0,
    1e6,
    0.47123889803846897, // 27 degrees
    -1.0e-300,
];

/// Two colours differing by odd amounts, turned by multiples of 45, 22.5 or 5
/// degrees: half blends then sit exactly on the byte rounding boundary, so the
/// output depends on whether each `a * b + c` in png.c was fused. Found by a
/// search that evaluated each of the five contraction sites unfused in turn;
/// this generator produced inputs that tell every site apart within a few
/// hundred cases.
fn contraction_witnesses(count: usize, rng: &mut Rng) -> Vec<(String, Image, f64)> {
    let mut out = Vec::new();
    for n in 0..count {
        let w = 1 + rng.below(24) as i32;
        let h = 1 + rng.below(24) as i32;
        let a = [rng.byte(), rng.byte(), rng.byte(), 255];
        let b = [a[0] ^ 1, a[1] ^ 3, a[2] ^ 5, 255];
        let kind = rng.below(3);
        let mut image = Image::alloc(w, h).expect("alloc");
        for i in 0..(w * h) as usize {
            let (x, y) = (i % w as usize, i / w as usize);
            let on = match kind {
                0 => rng.chance(2),
                1 => (x + y) % 2 == 0,
                _ => x % 2 == 0,
            };
            image.pixels[i * 4..i * 4 + 4].copy_from_slice(if on { &a } else { &b });
        }
        let angle = match rng.below(4) {
            0 => rng.below(8) as f64 * PI / 4.0,
            1 => rng.below(360) as f64 * PI / 180.0,
            2 => (rng.below(72) * 5) as f64 * PI / 180.0,
            _ => rng.below(16) as f64 * PI / 8.0,
        };
        out.push((format!("witness {n}: {w}x{h} kind {kind}"), image, angle));
    }
    out
}

/// resize (shrink, enlarge, non-square, mixed), rotate, rotate_resize and
/// tint in both modes, pixel for pixel.
#[test]
fn transforms_match_c() {
    let Some(exe) = png_oracle() else { return };
    let sources = transform_sources();
    let mut rng = Rng::new(6);

    // resize
    let mut records = Records::default();
    let mut cases = Vec::new();
    for (label, image) in &sources {
        let (w, h) = (image.width, image.height);
        let targets = [
            (w / 2 + 1, h / 2 + 1),
            (1, 1),
            ((w - 1).max(1), h),
            (w, h),
            (w * 2, h * 2),
            (w * 3 + 1, h),
            (w * 2, (h / 2).max(1)),
            (w * 8, h * 8),
            (1 + rng.below(3 * w as u64) as i32, 1 + rng.below(3 * h as u64) as i32),
        ];
        for (tw, th) in targets {
            records.image(image);
            records.i32(tw);
            records.i32(th);
            cases.push((format!("resize {label} to {tw}x{th}"), image, tw, th));
        }
    }
    let mut reader = run(&exe, "resize", &records);
    for (label, image, tw, th) in &cases {
        compare_result(
            label,
            reader.image_result(),
            rust_result(png::resize(image, *tw, *th)),
            &[],
        );
    }
    reader.finish();

    // rotate
    let mut records = Records::default();
    let mut cases = Vec::new();
    for (label, image) in &sources {
        let mut angles = ANGLES.to_vec();
        angles.push((rng.next() >> 11) as f64 / (1u64 << 53) as f64 * 20.0 - 10.0);
        for angle in angles {
            records.image(image);
            records.f64(angle);
            cases.push((format!("rotate {label} by {angle:e}"), image, angle));
        }
    }
    let witnesses = contraction_witnesses(800, &mut rng);
    for (label, image, angle) in &witnesses {
        records.image(image);
        records.f64(*angle);
        cases.push((format!("rotate {label} by {angle:e}"), image, *angle));
    }
    let mut reader = run(&exe, "rotate", &records);
    for (label, image, angle) in &cases {
        compare_result(label, reader.image_result(), rust_result(png::rotate(image, *angle)), &[]);
    }
    reader.finish();

    // rotate_resize
    let mut records = Records::default();
    let mut cases = Vec::new();
    for (label, image) in &sources {
        for (k, &angle) in ANGLES.iter().enumerate() {
            let (tw, th) = match k % 3 {
                0 => ((image.width / 3).max(1), (image.height / 3).max(1)),
                1 => (image.width * 2, image.height + 3),
                _ => (image.width, (image.height / 2).max(1)),
            };
            records.image(image);
            records.f64(angle);
            records.i32(tw);
            records.i32(th);
            cases.push((
                format!("rotate_resize {label} by {angle:e} to {tw}x{th}"),
                image,
                angle,
                tw,
                th,
            ));
        }
    }
    for (k, (label, image, angle)) in witnesses.iter().enumerate().step_by(4) {
        let (tw, th) = if k % 8 == 0 {
            (image.width * 3, image.height * 2)
        } else {
            ((image.width / 2).max(1), image.height)
        };
        records.image(image);
        records.f64(*angle);
        records.i32(tw);
        records.i32(th);
        cases.push((
            format!("rotate_resize {label} by {angle:e} to {tw}x{th}"),
            image,
            *angle,
            tw,
            th,
        ));
    }
    let mut reader = run(&exe, "rotate_resize", &records);
    for (label, image, angle, tw, th) in &cases {
        compare_result(
            label,
            reader.image_result(),
            rust_result(png::rotate_resize(image, *angle, *tw, *th)),
            &[],
        );
    }
    reader.finish();

    // tint
    let mut records = Records::default();
    let mut cases = Vec::new();
    for (label, image) in &sources {
        for (r, g, b) in [
            (0u8, 0u8, 0u8),
            (255, 255, 255),
            (10, 20, 30),
            (150, 150, 150),
            (rng.byte(), rng.byte(), rng.byte()),
        ] {
            for mode in [TintMode::Multiply, TintMode::Replace] {
                records.image(image);
                records.0.extend_from_slice(&[r, g, b, u8::from(mode == TintMode::Replace)]);
                cases.push((format!("tint {label} {mode:?} {r},{g},{b}"), image, r, g, b, mode));
            }
        }
    }
    // The empty image is left alone.
    records.image(&Image::empty());
    records.0.extend_from_slice(&[1, 2, 3, 1]);
    let mut reader = run(&exe, "tint", &records);
    for (label, image, r, g, b, mode) in &cases {
        let c = reader.image();
        let mut rust = (*image).clone();
        png::tint(&mut rust, *r, *g, *b, *mode);
        if c != rust {
            fail_with(label, &[], first_difference(&c, &rust));
        }
    }
    let c = reader.image();
    let mut rust = Image::empty();
    png::tint(&mut rust, 1, 2, 3, TintMode::Replace);
    assert_eq!(c, rust);
    reader.finish();
}

/// Argument and limit refusals of the transforms, in the C's order.
#[test]
fn transform_refusals_match_c() {
    let Some(exe) = png_oracle() else { return };
    let mut rng = Rng::new(8);
    let small = pattern(4, 3, 1, &mut rng);
    let empty = Image::empty();
    let sized_empty = Image { width: 4, height: 3, pixels: Vec::new() };
    let resizes: Vec<(&Image, i32, i32)> = vec![
        (&empty, 4, 4),
        (&sized_empty, 4, 4),
        (&small, 0, 4),
        (&small, 4, 0),
        (&small, -1, 4),
        (&empty, -1, 4),
        (&small, 16385, 1),
        (&small, 1, 16385),
        (&small, 16384, 4097),
        (&small, i32::MIN, 1),
        (&small, 16384, 1),
    ];
    let mut records = Records::default();
    for (image, w, h) in &resizes {
        records.image(image);
        records.i32(*w);
        records.i32(*h);
    }
    let mut reader = run(&exe, "resize", &records);
    for (image, w, h) in &resizes {
        let label = format!(
            "resize {}x{} (empty {}) to {w}x{h}",
            image.width,
            image.height,
            image.is_empty()
        );
        compare_result(&label, reader.image_result(), rust_result(png::resize(image, *w, *h)), &[]);
    }
    reader.finish();

    let mut records = Records::default();
    for image in [&empty, &sized_empty] {
        records.image(image);
        records.f64(1.0);
    }
    let mut reader = run(&exe, "rotate", &records);
    for image in [&empty, &sized_empty] {
        compare_result(
            "rotate empty",
            reader.image_result(),
            rust_result(png::rotate(image, 1.0)),
            &[],
        );
    }
    reader.finish();

    let rotate_resizes: Vec<(&Image, i32, i32)> =
        vec![(&empty, 2, 2), (&small, 0, 2), (&small, 2, 16385)];
    let mut records = Records::default();
    for (image, w, h) in &rotate_resizes {
        records.image(image);
        records.f64(0.5);
        records.i32(*w);
        records.i32(*h);
    }
    let mut reader = run(&exe, "rotate_resize", &records);
    for (image, w, h) in &rotate_resizes {
        let label = format!("rotate_resize to {w}x{h}");
        compare_result(
            &label,
            reader.image_result(),
            rust_result(png::rotate_resize(image, 0.5, *w, *h)),
            &[],
        );
    }
    reader.finish();
}

/// The sprites exactly as the application builds them: the embedded sprite
/// resized to the working square, squashed for each wing phase, and rotated
/// through the sixty headings at every bird size the renderers use.
#[test]
fn sprite_pipeline_matches_c() {
    let Some(exe) = png_oracle() else { return };
    let source = png::decode(&sprite_png()).expect("sprite");
    assert_eq!((source.width, source.height), (600, 600));
    let sizes: &[i32] = if full() {
        &[4, 5, 18, 19, 25, 26, 30, 37, 60, 64, 120, 128]
    } else {
        &[4, 5, 18, 19, 30]
    };

    let mut records = Records::default();
    for &size in sizes {
        records.image(&source);
        records.i32(size);
    }
    let mut reader = run(&exe, "sprites", &records);
    for &size in sizes {
        let label = format!("sprite size {size}");
        let mut work = size * 6;
        if work > 256 {
            work = 256;
        }
        if work > source.width {
            work = source.width;
        }
        let square = png::resize(&source, work, work);
        let (c_status, c_square) = reader.image_result();
        compare_result(
            &format!("{label} square"),
            (c_status, c_square),
            rust_result(square.clone()),
            &[],
        );
        let square = square.expect("square");

        let mut geometry = vec![square.clone()];
        for span in [0.72f64, 0.45] {
            let c_height = reader.i32();
            // boids.c: (int)(square->height * span + 0.5)
            let mut height = (f64::from(square.height) * span + 0.5) as i32;
            if height < 1 {
                height = 1;
            }
            assert_eq!(height, c_height, "{label}: squashed height for span {span}");
            let narrow = png::resize(&square, square.width, height).expect("narrow");
            let mut squashed = Image::alloc(square.width, square.height).expect("alloc");
            let top = ((square.height - height) / 2) as usize;
            let row = narrow.width as usize * 4;
            for y in 0..height as usize {
                squashed.pixels[(top + y) * row..(top + y + 1) * row]
                    .copy_from_slice(&narrow.pixels[y * row..(y + 1) * row]);
            }
            compare_result(
                &format!("{label} squashed {span}"),
                reader.image_result(),
                (0, Some(squashed.clone())),
                &[],
            );
            geometry.push(squashed);
        }
        for (k, shape) in geometry.iter().enumerate() {
            for i in 0..60 {
                // i * FRAME_ANGLE * M_PI / 180.0, left to right.
                let radians = f64::from(i * 6) * PI / 180.0;
                let rust = rust_result(png::rotate_resize(shape, radians, size, size));
                compare_result(
                    &format!("{label} span {k} frame {i}"),
                    reader.image_result(),
                    rust,
                    &[],
                );
            }
        }
    }
    reader.finish();
}
