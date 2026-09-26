//! cbirds `tests/png_test.c`, translated one test for one test.
//!
//! Every `test_*` function its `main` calls is a `#[test]` of the same name
//! here, with the same fixtures, boundaries and assertions. The C test's
//! independent PNG writer (scanline packing and filtering, stored DEFLATE
//! blocks, zlib header, Adler and CRC) is translated with it, so the files
//! the decoder is checked against still owe nothing to the codec.
//!
//! C calls that take an output pointer become `png_decode(data, &mut image)`
//! below, which writes the image only on success exactly as `png_decode`
//! does, so the C's `image.pixels == NULL` checks after refusals keep their
//! meaning. Assertions on NULL arguments that Rust cannot express are mapped
//! to the nearest guarantee Rust can check, with a comment at each.

use rbirds::image::png::{self, TintMode};
use rbirds::image::{Image, PngError, png_status_string};

/// `png_decode` with the C's out parameter: `out` is only written on success.
fn png_decode(data: &[u8], out: &mut Image) -> Result<(), PngError> {
    *out = png::decode(data)?;
    Ok(())
}

/// Fills an image with one of several shapes of data, because a compressor
/// that only ever sees one shape is not tested.
fn paint(image: &mut Image, kind: i32, seed: &mut u32) {
    let count = i64::from(image.width) * i64::from(image.height);
    for i in 0..count {
        let p = &mut image.pixels[i as usize * 4..i as usize * 4 + 4];
        match kind {
            0 => {
                // Flat, the easy case.
                p[0] = 40;
                p[1] = 40;
                p[2] = 40;
                p[3] = 255;
            }
            1 => {
                // Noise, the incompressible one.
                *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                p[0] = (*seed >> 16) as u8;
                p[1] = (*seed >> 8) as u8;
                p[2] = *seed as u8;
                p[3] = (*seed >> 24) as u8;
            }
            2 => {
                // Stripes, which is what a sprite mostly is.
                p[0] = if (i / 7) % 2 != 0 { 255 } else { 0 };
                p[1] = 128;
                p[2] = if (i / 7) % 2 != 0 { 0 } else { 255 };
                p[3] = 255;
            }
            _ => {
                // A gradient, where matches are long but never exact.
                p[0] = (i % 256) as u8;
                p[1] = (i / 256) as u8;
                p[2] = 90;
                p[3] = 255;
            }
        }
    }
}

#[test]
fn test_round_trip() {
    let mut seed = 1u32;
    const SIZES: [[i32; 2]; 6] = [[1, 1], [2, 3], [17, 5], [64, 64], [200, 120], [300, 200]];

    for size in SIZES {
        for kind in 0..4 {
            let mut back = Image::empty();

            let mut image = Image::alloc(size[0], size[1]).expect("alloc");
            paint(&mut image, kind, &mut seed);
            let encoded = png::encode(&image).expect("encode");

            // Every byte back, through our own inflater: the compressor and
            // the decompressor are each other's only check.
            assert_eq!(png_decode(&encoded, &mut back), Ok(()));
            assert!(back.width == image.width && back.height == image.height);
            assert_eq!(back.pixels, image.pixels);
        }
    }
}

#[test]
fn test_compression_earns_its_place() {
    let mut seed = 7u32;

    // Flat data must compress hard, or the compressor is not doing anything.
    let mut image = Image::alloc(128, 128).expect("alloc");
    paint(&mut image, 0, &mut seed);
    let raw = image.width as usize * image.height as usize * 4;
    let encoded = png::encode(&image).expect("encode");
    assert!(encoded.len() < raw / 20);
    image.free();

    // Stripes are what a sprite is mostly made of.
    let mut image = Image::alloc(200, 120).expect("alloc");
    paint(&mut image, 2, &mut seed);
    let raw = image.width as usize * image.height as usize * 4;
    let encoded = png::encode(&image).expect("encode");
    assert!(encoded.len() < raw / 20);
    image.free();

    // And noise must not come out meaningfully larger than it went in: the
    // stored fallback exists for exactly this.
    let mut image = Image::alloc(128, 128).expect("alloc");
    paint(&mut image, 1, &mut seed);
    let raw = image.width as usize * image.height as usize * 4;
    let encoded = png::encode(&image).expect("encode");
    assert!(encoded.len() < raw + raw / 50);
}

#[test]
fn test_transforms() {
    let mut seed = 3u32;

    let mut source = Image::alloc(32, 32).expect("alloc");
    paint(&mut source, 3, &mut seed);

    // A full turn is the image again, to within the resampling.
    #[allow(clippy::approx_constant, clippy::excessive_precision)] // The C test's own literal.
    let turned = png::rotate(&source, 2.0 * 3.14159265358979323846).expect("rotate");
    assert!(turned.width == 32 && turned.height == 32);

    // Shrinking halves it, enlarging doubles it, and neither loses the canvas.
    let smaller = png::resize(&source, 16, 16).expect("resize");
    assert!(smaller.width == 16 && smaller.height == 16);
    let smaller = png::resize(&source, 64, 48).expect("resize");
    assert!(smaller.width == 64 && smaller.height == 48);

    // A tint leaves alpha alone, which is what keeps wings from bleeding.
    let before = source.pixels[3];
    png::tint(&mut source, 10, 20, 30, TintMode::Replace);
    assert!(source.pixels[0] == 10 && source.pixels[1] == 20 && source.pixels[2] == 30);
    assert_eq!(source.pixels[3], before);
}

#[test]
fn test_refusals() {
    let mut image = Image::empty();
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

    // png_decode(NULL, 10, &image) == PNG_ERR_ARGUMENT: a slice cannot be
    // NULL, so the refusal of absent data cannot be asked for. What can be
    // asked for, no bytes at all, is refused rather than read.
    assert_eq!(png_decode(&[], &mut image), Err(PngError::Truncated));
    assert_eq!(png_decode(&SIGNATURE[..4], &mut image), Err(PngError::Truncated));
    assert_eq!(png_decode(b"not a png at all", &mut image), Err(PngError::Signature));
    assert_eq!(png_decode(&SIGNATURE, &mut image), Err(PngError::Truncated));

    assert_eq!(Image::alloc(0, 10), Err(PngError::Argument));
    assert_eq!(Image::alloc(10, -1), Err(PngError::Argument));
    assert_eq!(Image::alloc(1 << 20, 1 << 20), Err(PngError::Unsupported));
    // The refused allocations left the image unallocated, which is what the
    // C's png_encode(&image, ...) is then handed.
    assert!(image.is_empty());
    assert_eq!(png::encode(&image), Err(PngError::Argument));
    // png_rotate(NULL, ...): the empty image is the Rust spelling of a NULL
    // source, refused the same way.
    assert_eq!(png::rotate(&Image::empty(), 1.0), Err(PngError::Argument));
    assert_eq!(png::resize(&image, -1, 4), Err(PngError::Argument));

    // Every status has something to say for itself.
    let statuses = [
        Ok(()),
        Err(PngError::Memory),
        Err(PngError::Argument),
        Err(PngError::Truncated),
        Err(PngError::Signature),
        Err(PngError::Chunk),
        Err(PngError::Crc),
        Err(PngError::Unsupported),
        Err(PngError::Deflate),
    ];
    for status in statuses {
        let text = png_status_string(status);
        assert!(!text.is_empty());
        assert_ne!(text, "unknown error");
    }
}

/// A truncated or corrupted stream must be refused, never followed.
#[test]
fn test_damage_is_refused() {
    let mut back = Image::empty();
    let mut seed = 11u32;

    let mut image = Image::alloc(48, 48).expect("alloc");
    paint(&mut image, 3, &mut seed);
    let mut encoded = png::encode(&image).expect("encode");
    let length = encoded.len();

    // Every truncation of a good file is a bad file.
    for cut in (1..length).step_by(7) {
        assert!(png_decode(&encoded[..cut], &mut back).is_err());
    }

    // And so is a flipped bit anywhere in it, because every chunk is
    // checksummed.
    for at in (8..length).step_by(101) {
        encoded[at] ^= 0x40;
        assert!(png_decode(&encoded, &mut back).is_err());
        encoded[at] ^= 0x40;
    }
    // Unharmed, it still reads.
    assert_eq!(png_decode(&encoded, &mut back), Ok(()));
}

/* Everything below reads files built here byte by byte: scanlines packed and
 * filtered, stored deflate blocks, the zlib header and Adler checksum, the
 * chunk CRCs. None of it goes through png.rs, so the two cannot share a
 * mistake. */

fn put_byte(b: &mut Vec<u8>, value: u32) {
    b.push(value as u8);
}

fn put_bytes(b: &mut Vec<u8>, data: &[u8]) {
    for &byte in data {
        put_byte(b, u32::from(byte));
    }
}

fn put_be32(b: &mut Vec<u8>, value: u32) {
    for shift in [24, 16, 8, 0] {
        put_byte(b, (value >> shift) & 0xff);
    }
}

/// Bit by bit, without a table: slow, and plainly the one in the
/// specification.
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

fn adler_of(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn put_chunk(png: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    put_be32(png, body.len() as u32);
    let start = png.len();
    put_bytes(png, kind);
    put_bytes(png, body);
    let crc = crc_of(&png[start..]);
    put_be32(png, crc);
}

/// Signature and IHDR, whatever they say: what follows is up to the caller.
fn put_header(
    png: &mut Vec<u8>,
    width: i32,
    height: i32,
    depth: i32,
    color_type: i32,
    interlace: i32,
) {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut ihdr = [0u8; 13];

    for i in 0..4 {
        ihdr[i] = ((width as u32) >> (24 - 8 * i)) as u8;
        ihdr[4 + i] = ((height as u32) >> (24 - 8 * i)) as u8;
    }
    ihdr[8] = depth as u8;
    ihdr[9] = color_type as u8;
    ihdr[12] = interlace as u8;
    put_bytes(png, &SIGNATURE);
    put_chunk(png, b"IHDR", &ihdr);
}

/// A zlib stream of stored blocks: no compression, every byte where it can
/// be seen.
fn put_zlib_stored(zlib: &mut Vec<u8>, raw: &[u8]) {
    let length = raw.len();
    let mut at = 0usize;

    put_byte(zlib, 0x78);
    put_byte(zlib, 0x01);
    loop {
        let n = if length - at > 65535 { 65535 } else { length - at };
        put_byte(zlib, u32::from(at + n == length)); // Final block or not.
        put_byte(zlib, (n & 0xff) as u32);
        put_byte(zlib, (n >> 8) as u32);
        put_byte(zlib, (!n & 0xff) as u32);
        put_byte(zlib, ((!n >> 8) & 0xff) as u32);
        put_bytes(zlib, &raw[at..at + n]);
        at += n;
        if at >= length {
            break;
        }
    }
    put_be32(zlib, adler_of(raw));
}

/// An image as the file holds it: samples at their own depth, before anything
/// is brought to 8 bits.
#[derive(Clone)]
struct Picture<'a> {
    width: i32,
    height: i32,
    depth: i32,
    color_type: i32,
    interlaced: i32,
    /// The PLTE body, or None for none; `palette_length` bytes of it are
    /// written.
    palette: Option<&'a [u8]>,
    palette_length: usize,
    /// The tRNS body, or None for none.
    trns: Option<&'a [u8]>,
    trns_length: usize,
    /// width * height * channels, row major.
    samples: Vec<u32>,
}

impl<'a> Picture<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: i32,
        height: i32,
        depth: i32,
        color_type: i32,
        interlaced: i32,
        palette: Option<&'a [u8]>,
        palette_length: usize,
        trns: Option<&'a [u8]>,
        trns_length: usize,
    ) -> Picture<'a> {
        Picture {
            width,
            height,
            depth,
            color_type,
            interlaced,
            palette,
            palette_length,
            trns,
            trns_length,
            samples: Vec::new(),
        }
    }
}

fn channels_of(color_type: i32) -> i32 {
    match color_type {
        2 => 3,
        4 => 2,
        6 => 4,
        _ => 1,
    }
}

/// Samples below `limit`, with both ends of the range in.
fn make_samples(pic: &Picture, limit: u32, seed: &mut u32) -> Vec<u32> {
    let count = pic.width as usize * pic.height as usize * channels_of(pic.color_type) as usize;
    let mut samples = vec![0u32; count];

    for sample in samples.iter_mut() {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        *sample = (*seed >> 8) % limit;
    }
    samples[0] = 0;
    samples[count - 1] = limit - 1;
    samples
}

/// Adam7, from the specification: first column, first row, and the two
/// steps.
const ADAM7: [[i32; 4]; 7] = [
    [0, 0, 8, 8],
    [4, 0, 8, 8],
    [0, 4, 4, 8],
    [2, 0, 4, 4],
    [0, 2, 2, 4],
    [1, 0, 2, 2],
    [0, 1, 1, 2],
];
const NOT_INTERLACED: [[i32; 4]; 1] = [[0, 0, 1, 1]];

fn predict(filter: i32, a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());

    match filter {
        1 => a,
        2 => b,
        3 => (a + b) / 2,
        4 => {
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

/// The raw scanlines, pass by pass. Row y of pass p takes filter (y + p) % 5,
/// so every filter meets every depth, sub byte and 16 bit ones included, and
/// the filters look back one pixel, or one byte when pixels are smaller.
fn put_scanlines(raw: &mut Vec<u8>, pic: &Picture) {
    let channels = channels_of(pic.color_type);
    let bits = channels * pic.depth;
    let unit = if bits >= 8 { bits as usize / 8 } else { 1 };
    let passes: &[[i32; 4]] = if pic.interlaced != 0 { &ADAM7 } else { &NOT_INTERLACED };

    for (p, pass) in passes.iter().enumerate() {
        let [x0, y0, dx, dy] = *pass;
        let width = if pic.width > x0 { (pic.width - x0 + dx - 1) / dx } else { 0 };
        let height = if pic.height > y0 { (pic.height - y0 + dy - 1) / dy } else { 0 };
        if width == 0 || height == 0 {
            continue; // Not even a filter byte.
        }

        let stride = (width as usize * bits as usize).div_ceil(8);
        let mut line = vec![0u8; stride];
        let mut above = vec![0u8; stride];
        for y in 0..height {
            line.fill(0);
            for x in 0..width {
                let pixel = (y0 + y * dy) as usize * pic.width as usize + (x0 + x * dx) as usize;
                for c in 0..channels as usize {
                    let value = pic.samples[pixel * channels as usize + c];
                    let index = x as usize * channels as usize + c;
                    if pic.depth == 16 {
                        line[index * 2] = (value >> 8) as u8;
                        line[index * 2 + 1] = value as u8;
                    } else if pic.depth == 8 {
                        line[index] = value as u8;
                    } else {
                        // Packed from the most significant bit.
                        let bit = index * pic.depth as usize;
                        line[bit / 8] |= (value << (8 - pic.depth as usize - bit % 8)) as u8;
                    }
                }
            }
            let filter = (y + p as i32) % 5;
            put_byte(raw, filter as u32);
            for i in 0..stride {
                let a = if i >= unit { i32::from(line[i - unit]) } else { 0 };
                // The row above starts as zeros.
                let c = if i >= unit { i32::from(above[i - unit]) } else { 0 };
                put_byte(
                    raw,
                    (i32::from(line[i]) - predict(filter, a, i32::from(above[i]), c)) as u32 & 0xff,
                );
            }
            above.copy_from_slice(&line);
        }
    }
}

/// The zlib stream of the picture, its raster longer or shorter by `extra`
/// bytes.
fn put_picture_zlib(zlib: &mut Vec<u8>, pic: &Picture, extra: i32) {
    let mut raw = Vec::new();

    put_scanlines(&mut raw, pic);
    if extra < 0 {
        raw.truncate(raw.len() - (-extra) as usize);
    }
    for _ in 0..extra {
        put_byte(&mut raw, 0);
    }
    put_zlib_stored(zlib, &raw);
}

/// The file, its chunks in the given order: X a text chunk the decoder must
/// skip, P PLTE, T tRNS, D the image data split over two IDAT, as encoders
/// may do. IEND closes it whatever the order.
fn put_picture(png: &mut Vec<u8>, pic: &Picture, extra: i32, order: &str) {
    let mut zlib = Vec::new();

    put_picture_zlib(&mut zlib, pic, extra);
    put_header(png, pic.width, pic.height, pic.depth, pic.color_type, pic.interlaced);
    for o in order.bytes() {
        if o == b'X' {
            put_chunk(png, b"tEXt", b"Comment\0by hand");
        }
        if o == b'P'
            && let Some(palette) = pic.palette
        {
            put_chunk(png, b"PLTE", &palette[..pic.palette_length]);
        }
        if o == b'T'
            && let Some(trns) = pic.trns
        {
            put_chunk(png, b"tRNS", &trns[..pic.trns_length]);
        }
        if o == b'D' {
            put_chunk(png, b"IDAT", &zlib[..zlib.len() / 2]);
            put_chunk(png, b"IDAT", &zlib[zlib.len() / 2..]);
        }
    }
    put_chunk(png, b"IEND", &[]);
}

fn decode_picture(
    pic: &Picture,
    extra: i32,
    order: &str,
    image: &mut Image,
) -> Result<(), PngError> {
    let mut png = Vec::new();

    put_picture(&mut png, pic, extra, order);
    png_decode(&png, image)
}

/// What the decoder must give back, from the specification alone: small
/// samples stretched to 0..255, 16 bit ones cut to their high byte, a tRNS key
/// matched at the full depth, and palette indices past PLTE transparent
/// black.
fn expected_rgba(pic: &Picture, rgba: &mut [u8]) {
    let channels = channels_of(pic.color_type) as usize;
    let top = (1u32 << pic.depth) - 1;
    let mut key = [0u32; 3];
    let keyed = pic.trns.is_some() && (pic.color_type == 0 || pic.color_type == 2);

    if keyed {
        let trns = pic.trns.expect("keyed");
        for c in 0..channels {
            key[c] = (u32::from(trns[2 * c]) << 8) | u32::from(trns[2 * c + 1]);
        }
    }

    for i in 0..pic.width as usize * pic.height as usize {
        let s = &pic.samples[i * channels..i * channels + channels];
        let q = &mut rgba[i * 4..i * 4 + 4];
        let mut v = [0u8; 4];

        for c in 0..channels {
            v[c] = if pic.depth == 16 { (s[c] >> 8) as u8 } else { (s[c] * 255 / top) as u8 };
        }
        match pic.color_type {
            0 => {
                q[..3].fill(v[0]);
                q[3] = if keyed && s[0] == key[0] { 0 } else { 255 };
            }
            2 => {
                q[..3].copy_from_slice(&v[..3]);
                q[3] = if keyed && s[0] == key[0] && s[1] == key[1] && s[2] == key[2] {
                    0
                } else {
                    255
                };
            }
            3 => {
                q.fill(0);
                if (s[0] as usize) < pic.palette_length / 3 {
                    let palette = pic.palette.expect("palette");
                    let at = 3 * s[0] as usize;
                    q[..3].copy_from_slice(&palette[at..at + 3]);
                    q[3] = match pic.trns {
                        Some(trns) if (s[0] as usize) < pic.trns_length => trns[s[0] as usize],
                        _ => 255,
                    };
                }
            }
            4 => {
                q[..3].fill(v[0]);
                q[3] = v[1];
            }
            _ => q.copy_from_slice(&v),
        }
    }
}

/// Decodes, and must match the expected pixels exactly.
fn assert_decodes(pic: &Picture) {
    let size = pic.width as usize * pic.height as usize * 4;
    let mut want = vec![0u8; size];
    let mut image = Image::empty();

    expected_rgba(pic, &mut want);
    assert_eq!(decode_picture(pic, 0, "XPTD", &mut image), Ok(()));
    assert!(image.width == pic.width && image.height == pic.height);
    assert_eq!(image.pixels, want);
}

/// The header says how long the raster is. A stream that inflates to more is
/// a decompression bomb, and must be stopped at the first byte too many
/// rather than inflated and judged afterwards.
#[test]
fn test_bombs_are_refused() {
    let mut image = Image::empty();
    let mut zlib = Vec::new();
    let mut png = Vec::new();
    let size = 4usize << 20;
    let zeros = vec![0u8; size];

    // A 1x1 RGBA image is five bytes of raster; these are four megabytes.
    put_zlib_stored(&mut zlib, &zeros);
    put_header(&mut png, 1, 1, 8, 6, 0);
    put_chunk(&mut png, b"IDAT", &zlib);
    put_chunk(&mut png, b"IEND", &[]);
    assert_eq!(png_decode(&png, &mut image), Err(PngError::Deflate));
    assert!(image.pixels.is_empty());
    drop(zeros);

    // The real thing, fixed Huffman codes: a literal zero, then matches of
    // 258 bytes at distance 1, thirteen bits each. About 420 KB that inflate
    // to 64 MB, and a correct Adler checksum, so the size is the only fault.
    let mut produced = 1usize;
    let target = 64usize << 20;
    zlib.clear();
    png.clear();
    put_byte(&mut zlib, 0x78);
    put_byte(&mut zlib, 0x01);
    let mut bits: u32 = 1 | (1 << 1); // Final block, fixed codes.
    let mut count: i32 = 3;
    bits |= 0x0c << count; // Literal 0 is 00110000, sent from its first bit.
    count += 8;
    while produced + 258 <= target {
        // Length 258 is 11000101, reversed; distance 1 is five zeros.
        bits |= 0xa3 << count;
        count += 13;
        while count >= 8 {
            put_byte(&mut zlib, bits & 0xff);
            bits >>= 8;
            count -= 8;
        }
        produced += 258;
    }
    count += 7; // End of block is seven zeros.
    while count > 0 {
        put_byte(&mut zlib, bits & 0xff);
        bits >>= 8;
        count -= 8;
    }
    put_be32(&mut zlib, (((produced % 65521) as u32) << 16) | 1); // Adler of zeros.
    assert!(zlib.len() < 450000);
    put_header(&mut png, 1, 1, 8, 6, 0);
    put_chunk(&mut png, b"IDAT", &zlib);
    put_chunk(&mut png, b"IEND", &[]);
    assert_eq!(png_decode(&png, &mut image), Err(PngError::Deflate));
    assert!(image.pixels.is_empty());
}

/// One byte short is refused, and so is one byte over: the raster is exact.
#[test]
fn test_raster_length_is_exact() {
    let mut seed = 5u32;

    for interlaced in 0..2 {
        let mut pic = Picture::new(3, 2, 8, 2, interlaced, None, 0, None, 0);
        let mut image = Image::empty();

        pic.samples = make_samples(&pic, 256, &mut seed);
        assert_eq!(decode_picture(&pic, -1, "D", &mut image), Err(PngError::Truncated));
        assert_eq!(decode_picture(&pic, 1, "D", &mut image), Err(PngError::Deflate));
        assert!(image.pixels.is_empty());
        assert_decodes(&pic);
    }
}

/// What every optimizer writes: an index per pixel, often under 8 bits, often
/// with fewer colors than the depth allows, often with some of them see
/// through.
#[test]
fn test_palettes() {
    let mut palette = [0u8; 257 * 3];
    let mut alpha = [0u8; 256];
    let mut seed = 21u32;
    // Width, height, depth, PLTE entries, tRNS entries. Odd widths end rows
    // in the middle of a byte.
    const CASES: [[i32; 5]; 8] = [
        [13, 6, 1, 2, 1],
        [7, 5, 2, 3, 2],
        [5, 7, 4, 11, 6],
        [9, 6, 8, 200, 17],
        [9, 6, 8, 256, 256],
        [10, 5, 8, 7, 0],
        [17, 5, 4, 16, 16],
        [3, 5, 1, 2, 0],
    ];

    for (i, entry) in palette.iter_mut().enumerate() {
        *entry = (i as u32 * 37 + 11) as u8;
    }
    for (i, entry) in alpha.iter_mut().enumerate() {
        *entry = (i as u32 * 53 + 7) as u8;
    }

    for c in CASES {
        for interlaced in 0..2 {
            let mut pic = Picture::new(
                c[0],
                c[1],
                c[2],
                3,
                interlaced,
                Some(&palette),
                c[3] as usize * 3,
                if c[4] != 0 { Some(&alpha) } else { None },
                c[4] as usize,
            );
            pic.samples = make_samples(&pic, c[3] as u32, &mut seed);
            assert_decodes(&pic);
        }
    }

    // An index past PLTE is an error in the file: it reads as transparent
    // black.
    let mut pic = Picture::new(5, 7, 4, 3, 0, Some(&palette), 5 * 3, Some(&alpha), 2);
    let mut image = Image::empty();
    pic.samples = make_samples(&pic, 16, &mut seed);
    pic.samples[1] = 15;
    pic.samples[2] = 4;
    assert_decodes(&pic);
    assert_eq!(decode_picture(&pic, 0, "PTD", &mut image), Ok(()));
    const HOLE: [u8; 4] = [0, 0, 0, 0];
    assert_eq!(image.pixels[4..8], HOLE);
    assert!(image.pixels[8..11] == palette[12..15] && image.pixels[11] == 255);
    image.free();

    // No palette, an empty one, one not made of triples, one too long.
    pic.palette = None;
    assert_eq!(decode_picture(&pic, 0, "PD", &mut image), Err(PngError::Chunk));
    pic.palette = Some(&palette);
    pic.palette_length = 0;
    assert_eq!(decode_picture(&pic, 0, "PD", &mut image), Err(PngError::Chunk));
    pic.palette_length = 16;
    assert_eq!(decode_picture(&pic, 0, "PD", &mut image), Err(PngError::Chunk));
    pic.palette_length = 257 * 3;
    assert_eq!(decode_picture(&pic, 0, "PD", &mut image), Err(PngError::Chunk));

    // More alphas than colors, and chunks out of order: PLTE after the data,
    // tRNS before PLTE or after the data, either one twice.
    pic.palette_length = 5 * 3;
    pic.trns_length = 6;
    assert_eq!(decode_picture(&pic, 0, "PTD", &mut image), Err(PngError::Chunk));
    pic.trns_length = 5;
    assert_eq!(decode_picture(&pic, 0, "PTD", &mut image), Ok(()));
    image.free();
    assert_eq!(decode_picture(&pic, 0, "DP", &mut image), Err(PngError::Chunk));
    assert_eq!(decode_picture(&pic, 0, "PDP", &mut image), Err(PngError::Chunk));
    assert_eq!(decode_picture(&pic, 0, "TPD", &mut image), Err(PngError::Chunk));
    assert_eq!(decode_picture(&pic, 0, "PDT", &mut image), Err(PngError::Chunk));
    assert_eq!(decode_picture(&pic, 0, "PPD", &mut image), Err(PngError::Chunk));
    assert_eq!(decode_picture(&pic, 0, "PTTD", &mut image), Err(PngError::Chunk));
    assert!(image.pixels.is_empty());
}

#[test]
fn test_gray() {
    let mut seed = 33u32;
    const DEPTHS: [i32; 5] = [1, 2, 4, 8, 16];
    let mut image = Image::empty();

    for depth in DEPTHS {
        for interlaced in 0..2 {
            let mut pic = Picture::new(11, 6, depth, 0, interlaced, None, 0, None, 0);
            pic.samples = make_samples(&pic, 1u32 << depth, &mut seed);
            assert_decodes(&pic);
        }
    }

    // A key at 2 bits: every pixel of value 2 is transparent.
    const KEY2: [u8; 2] = [0, 2];
    let mut pic = Picture::new(9, 5, 2, 0, 0, None, 0, Some(&KEY2), 2);
    pic.samples = make_samples(&pic, 4, &mut seed);
    assert_decodes(&pic);

    // A key at 16 bits is matched at 16 bits: 0x12ff reads as the same gray
    // as the key 0x1234, and stays opaque.
    const KEY16: [u8; 2] = [0x12, 0x34];
    let mut pic = Picture::new(8, 6, 16, 0, 0, None, 0, Some(&KEY16), 2);
    pic.samples = make_samples(&pic, 65536, &mut seed);
    for i in (0..8 * 6).step_by(3) {
        pic.samples[i] = 0x1234;
        pic.samples[i + 1] = 0x12ff;
    }
    assert_decodes(&pic);
    assert_eq!(decode_picture(&pic, 0, "TD", &mut image), Ok(()));
    assert!(image.pixels[0] == 0x12 && image.pixels[3] == 0);
    assert!(image.pixels[4] == 0x12 && image.pixels[7] == 255);
    image.free();

    // A key is two bytes for gray, never more or fewer.
    pic.trns_length = 1;
    assert_eq!(decode_picture(&pic, 0, "TD", &mut image), Err(PngError::Chunk));
    pic.trns = Some(b"\x12\x34\x56\x78\x9a\xbc");
    pic.trns_length = 6;
    assert_eq!(decode_picture(&pic, 0, "TD", &mut image), Err(PngError::Chunk));

    // Gray with alpha, at both depths it comes in.
    for depth in [8, 16] {
        let mut pic = Picture::new(7, 6, depth, 4, 0, None, 0, None, 0);
        pic.samples = make_samples(&pic, 1u32 << depth, &mut seed);
        assert_decodes(&pic);
    }
    assert!(image.pixels.is_empty());
}

#[test]
fn test_sixteen_bits() {
    let mut seed = 44u32;
    let mut image = Image::empty();

    // An RGB key at 16 bits: pixels differing from it in one low byte read as
    // the same color and must stay opaque.
    const KEY: [u8; 6] = [0x12, 0x34, 0xab, 0xcd, 0x00, 0x01];
    let mut pic = Picture::new(7, 5, 16, 2, 0, None, 0, Some(&KEY), 6);
    pic.samples = make_samples(&pic, 65536, &mut seed);
    let mut i = 0;
    while i + 1 < 7 * 5 {
        let key = i * 3;
        let almost = key + 3;
        pic.samples[key] = 0x1234;
        pic.samples[almost] = 0x1234;
        pic.samples[key + 1] = 0xabcd;
        pic.samples[almost + 1] = 0xabcd;
        pic.samples[key + 2] = 0x0001;
        pic.samples[almost + 2] = 0x0002;
        i += 4;
    }
    assert_decodes(&pic);
    assert_eq!(decode_picture(&pic, 0, "TD", &mut image), Ok(()));
    const KEYED: [u8; 4] = [0x12, 0xab, 0x00, 0];
    const ALMOST: [u8; 4] = [0x12, 0xab, 0x00, 255];
    assert!(image.pixels[..4] == KEYED && image.pixels[4..8] == ALMOST);
    image.free();

    // The key is six bytes for RGB.
    pic.trns_length = 2;
    assert_eq!(decode_picture(&pic, 0, "TD", &mut image), Err(PngError::Chunk));

    // The same at 8 bits, where the key is still written with two bytes a
    // sample.
    const KEY8: [u8; 6] = [0, 10, 0, 20, 0, 30];
    let mut pic = Picture::new(6, 6, 8, 2, 0, None, 0, Some(&KEY8), 6);
    pic.samples = make_samples(&pic, 256, &mut seed);
    pic.samples[3] = 10;
    pic.samples[4] = 20;
    pic.samples[5] = 30;
    assert_decodes(&pic);

    // RGBA at 16 bits, eight bytes a pixel, with a palette the decoder
    // ignores.
    let mut pic = Picture::new(6, 5, 16, 6, 0, Some(b"\x01\x02\x03"), 3, None, 0);
    pic.samples = make_samples(&pic, 65536, &mut seed);
    assert_decodes(&pic);
    assert!(image.pixels.is_empty());
}

/// Adam7: seven passes, each its own small image with its own filtered rows,
/// scattered back into the whole.
#[test]
fn test_interlaced() {
    let mut seed = 55u32;
    let palette: [u8; 4 * 3] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9];
    const ALPHA: [u8; 3] = [0, 128, 255];
    const KEY: [u8; 6] = [0x80, 0x00, 0x00, 0x00, 0xff, 0xff];
    // 13x7: passes of one or two columns, and rows that end in the middle of
    // a byte at every depth under 8.
    let mut cases = [
        Picture::new(13, 7, 8, 6, 1, None, 0, None, 0),
        Picture::new(13, 7, 16, 2, 1, None, 0, Some(&KEY), 6),
        Picture::new(13, 7, 2, 3, 1, Some(&palette), 12, Some(&ALPHA), 3),
        Picture::new(13, 7, 1, 0, 1, None, 0, None, 0),
        Picture::new(13, 7, 16, 4, 1, None, 0, None, 0),
        Picture::new(13, 7, 4, 0, 1, None, 0, None, 0),
        // 1x1: passes 2 to 7 are empty, and send nothing at all.
        Picture::new(1, 1, 8, 6, 1, None, 0, None, 0),
        Picture::new(1, 1, 1, 0, 1, None, 0, None, 0),
    ];

    for pic in cases.iter_mut() {
        let limit = if pic.color_type == 3 { 4 } else { 1u32 << pic.depth };
        pic.samples = make_samples(pic, limit, &mut seed);
        if pic.color_type == 2 {
            // One pixel on the key.
            pic.samples[3] = 0x8000;
            pic.samples[4] = 0;
            pic.samples[5] = 0xffff;
        }
        assert_decodes(pic);
    }

    // Every small size, where passes come and go.
    for width in 1..=10 {
        for height in 1..=10 {
            let mut gray = Picture::new(width, height, 2, 0, 1, None, 0, None, 0);
            let mut rgba = Picture::new(width, height, 8, 6, 1, None, 0, None, 0);
            gray.samples = make_samples(&gray, 4, &mut seed);
            rgba.samples = make_samples(&rgba, 256, &mut seed);
            assert_decodes(&gray);
            assert_decodes(&rgba);
        }
    }
}

#[test]
fn test_unsupported_variants() {
    let mut image = Image::empty();
    let mut zlib = Vec::new();
    const RAW: [u8; 5] = [0, 1, 2, 3, 4];
    // Depth and color type pairs outside the specification, then an interlace
    // method that does not exist.
    const CASES: [[i32; 3]; 11] = [
        [3, 0, 0],
        [16, 3, 0],
        [4, 2, 0],
        [1, 4, 0],
        [2, 6, 0],
        [8, 1, 0],
        [8, 5, 0],
        [8, 7, 0],
        [0, 0, 0],
        [32, 6, 0],
        [8, 6, 2],
    ];

    put_zlib_stored(&mut zlib, &RAW);
    for case in CASES {
        let mut png = Vec::new();
        put_header(&mut png, 1, 1, case[0], case[1], case[2]);
        put_chunk(&mut png, b"IDAT", &zlib);
        put_chunk(&mut png, b"IEND", &[]);
        assert_eq!(png_decode(&png, &mut image), Err(PngError::Unsupported));
    }

    // A filter type past Paeth.
    const BAD_FILTER: [u8; 5] = [5, 1, 2, 3, 4];
    let mut png = Vec::new();
    zlib.clear();
    put_zlib_stored(&mut zlib, &BAD_FILTER);
    put_header(&mut png, 1, 1, 8, 6, 0);
    put_chunk(&mut png, b"IDAT", &zlib);
    put_chunk(&mut png, b"IEND", &[]);
    assert_eq!(png_decode(&png, &mut image), Err(PngError::Chunk));
    assert!(image.pixels.is_empty());
}
