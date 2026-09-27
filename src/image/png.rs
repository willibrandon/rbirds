//! The PNG codec, its DEFLATE implementation and checksums, and the RGBA
//! transforms: a translation of cbirds `png.c` (commit
//! cc446fc3cb80733371c62676533adcac2fc10002).
//!
//! It handles what the program needs and nothing more: decoding of any still
//! PNG file (grayscale at 1, 2, 4, 8 or 16 bits, palette at 1, 2, 4 or 8 with
//! tRNS transparency, RGB, gray + alpha and RGBA at 8 or 16, plain or Adam7
//! interlaced) into 8 bit RGBA, a few geometric and color transformations, and
//! encoding back to a PNG kept in memory. Ancillary chunks other than tRNS
//! (gamma, color profiles, text) are ignored, and 16 bit samples keep their
//! high byte. The DEFLATE codec and the CRC/Adler checksums are here too.
//!
//! Where the C takes a pointer that may be NULL, this takes a reference, so a
//! NULL input or output cannot be expressed. The empty [`Image`] stands for an
//! image whose `pixels` is NULL and is refused with the same status. An
//! [`Image`] whose pixel buffer does not hold `width * height * 4` bytes is a
//! state the C API cannot produce and would read out of bounds; it is refused
//! as [`PngError::Argument`] here instead.
//!
//! Every C allocation that can report failure (`malloc`, `calloc`, `realloc`
//! returning NULL) is a fallible reservation here, mapped to the same status.

#![forbid(unsafe_code)]

use super::{Image, MAX_DIMENSION, MAX_PIXELS, PngError};
use crate::fp::{mul_add, sin_cos};

/// Largest stored DEFLATE block (`DEFLATE_MAX_BLOCK`).
const DEFLATE_MAX_BLOCK: usize = 65535;

/// How [`tint`] recolors (`png_tint_mode_t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TintMode {
    /// Color is scaled by the given one (`PNG_TINT_MULTIPLY`).
    Multiply,
    /// Color is replaced, alpha is kept (`PNG_TINT_REPLACE`).
    Replace,
}

/*============================== Checksums ==================================*/

/// The CRC table the C builds on first use (`crc_table_init`).
const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// The PNG/zlib CRC-32 of `data` (`crc32_of`).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &byte in data {
        c = CRC_TABLE[((c ^ u32::from(byte)) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// The zlib Adler-32 of `data` (`adler32_of`).
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/*=============================== Inflate ===================================
 *
 * DEFLATE decompressor (RFC 1951). Supports the three block types: stored,
 * fixed Huffman and dynamic Huffman. Speed is not a concern here, the base
 * image is decompressed exactly once at startup, so the straightforward bit by
 * bit canonical Huffman decoding is used.
 */

/// `inflate_t`. `out.len()` is the C's `out_len`; `out_cap` is the capacity
/// the C has asked `realloc` for, tracked separately because the growth
/// policy, not the allocator's rounding, decides when the next request is made.
struct Inflate<'a> {
    src: &'a [u8],
    src_pos: usize,
    bit_buf: u32,
    bit_count: i32,
    out: Vec<u8>,
    out_cap: usize,
    /// The stream is refused as soon as it inflates past this.
    out_limit: usize,
    failed: bool,
    no_memory: bool,
}

/// `huffman_t`.
#[derive(Clone, Copy)]
struct Huffman {
    /// Number of codes per length.
    counts: [i16; 16],
    /// Symbols ordered by code.
    symbols: [i16; 288],
}

impl Huffman {
    const EMPTY: Huffman = Huffman { counts: [0; 16], symbols: [0; 288] };

    /// `huffman_build`, `count` being `lengths.len()`.
    fn build(&mut self, lengths: &[u8]) {
        let mut offsets = [0i16; 16];

        self.counts = [0; 16];
        for &length in lengths {
            self.counts[usize::from(length)] += 1;
        }
        self.counts[0] = 0;

        offsets[0] = 0;
        offsets[1] = 0;
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + self.counts[len];
        }
        for (i, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let slot = &mut offsets[usize::from(length)];
                self.symbols[*slot as usize] = i as i16;
                *slot += 1;
            }
        }
    }
}

const LENGTH_BASE: [i16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [i16; 29] =
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [i16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [i16; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

impl Inflate<'_> {
    /// `inflate_bits`: the next `need` bits, least significant first. Running
    /// out of input sets `failed` and returns 0, leaving the buffer as it was.
    fn bits(&mut self, need: i32) -> i32 {
        let mut val = self.bit_buf;

        while self.bit_count < need {
            if self.src_pos >= self.src.len() {
                self.failed = true;
                return 0;
            }
            val |= u32::from(self.src[self.src_pos]) << self.bit_count;
            self.src_pos += 1;
            self.bit_count += 8;
        }
        self.bit_buf = val >> need;
        self.bit_count -= need;
        (val & ((1u32 << need) - 1)) as i32
    }

    /// `inflate_reserve`: makes room for `extra` more bytes of output, never
    /// past the limit. The caller knows how much the stream is meant to hold,
    /// and a stream that holds more is refused before a byte of the excess is
    /// allocated. Without it a file of a few hundred kilobytes inflates to
    /// gigabytes (a decompression bomb), since one DEFLATE match repeats 258
    /// bytes for as little as two bits.
    fn reserve(&mut self, extra: usize) -> bool {
        if extra > self.out_limit - self.out.len() {
            return false;
        }
        let needed = self.out.len() + extra;
        if needed <= self.out_cap {
            return true;
        }

        let mut cap = if self.out_cap != 0 { self.out_cap } else { 4096 };
        while cap < needed {
            if cap > usize::MAX / 2 {
                return false;
            }
            cap *= 2;
        }
        if cap > self.out_limit {
            cap = self.out_limit;
        }
        // realloc(s->out, cap)
        if self.out.try_reserve_exact(cap - self.out.len()).is_err() {
            self.no_memory = true;
            return false;
        }
        self.out_cap = cap;
        true
    }

    /// `huffman_decode`: -1 on running out of input or on a code no symbol has.
    fn decode_symbol(&mut self, h: &Huffman) -> i32 {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);

        for len in 1..=15 {
            code |= self.bits(1);
            if self.failed {
                return -1;
            }
            let count = i32::from(h.counts[len]);
            // code >= first on every pass (a prefix that matched no shorter
            // code is at least the first code of this length), so the index is
            // one of the `index + count` symbols built.
            if code - first < count {
                return i32::from(h.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        -1
    }

    /// `inflate_stored`.
    fn stored(&mut self) -> bool {
        self.bit_buf = 0;
        self.bit_count = 0;
        let src = self.src;
        let pos = self.src_pos;
        if pos + 4 > src.len() {
            return false;
        }

        let len = u32::from(src[pos]) | (u32::from(src[pos + 1]) << 8);
        let nlen = u32::from(src[pos + 2]) | (u32::from(src[pos + 3]) << 8);
        self.src_pos += 4;
        if len != (!nlen & 0xffff) {
            return false;
        }
        let len = len as usize;
        if self.src_pos + len > src.len() {
            return false;
        }
        if !self.reserve(len) {
            return false;
        }

        self.out.extend_from_slice(&src[self.src_pos..self.src_pos + len]);
        self.src_pos += len;
        true
    }

    /// `inflate_block`: one block's codes up to its end-of-block symbol.
    fn block(&mut self, lencode: &Huffman, distcode: &Huffman) -> bool {
        loop {
            let mut symbol = self.decode_symbol(lencode);
            if symbol < 0 {
                return false;
            }

            if symbol < 256 {
                if !self.reserve(1) {
                    return false;
                }
                self.out.push(symbol as u8);
                continue;
            }
            if symbol == 256 {
                return true; // end of block
            }

            symbol -= 257;
            if symbol >= 29 {
                return false;
            }
            let len = i32::from(LENGTH_BASE[symbol as usize])
                + self.bits(i32::from(LENGTH_EXTRA[symbol as usize]));

            symbol = self.decode_symbol(distcode);
            if !(0..30).contains(&symbol) {
                return false;
            }
            let dist = DIST_BASE[symbol as usize] as usize
                + self.bits(i32::from(DIST_EXTRA[symbol as usize])) as usize;
            if self.failed || dist > self.out.len() {
                return false;
            }
            if !self.reserve(len as usize) {
                return false;
            }

            for _ in 0..len {
                let byte = self.out[self.out.len() - dist];
                self.out.push(byte);
            }
        }
    }

    /// `inflate_dynamic_tables`.
    fn dynamic_tables(&mut self, lencode: &mut Huffman, distcode: &mut Huffman) -> bool {
        const ORDER: [u8; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
        // The C zeroes the first 19 and reads no other entry before writing it.
        let mut lengths = [0u8; 288 + 30];
        let mut codelen = Huffman::EMPTY;

        let nlen = self.bits(5) + 257;
        let ndist = self.bits(5) + 1;
        let ncode = self.bits(4) + 4;
        if self.failed || nlen > 286 || ndist > 30 {
            return false;
        }

        for &at in &ORDER[..ncode as usize] {
            lengths[usize::from(at)] = self.bits(3) as u8;
        }
        if self.failed {
            return false;
        }
        codelen.build(&lengths[..19]);

        let (nlen, ndist) = (nlen as usize, ndist as usize);
        let mut index = 0usize;
        while index < nlen + ndist {
            let symbol = self.decode_symbol(&codelen);
            if symbol < 0 {
                return false;
            }

            if symbol < 16 {
                lengths[index] = symbol as u8;
                index += 1;
                continue;
            }

            let mut repeat;
            let mut value = 0u8;
            if symbol == 16 {
                if index == 0 {
                    return false;
                }
                value = lengths[index - 1];
                repeat = 3 + self.bits(2);
            } else if symbol == 17 {
                repeat = 3 + self.bits(3);
            } else {
                repeat = 11 + self.bits(7);
            }
            if self.failed || index + repeat as usize > nlen + ndist {
                return false;
            }
            while repeat > 0 {
                repeat -= 1;
                lengths[index] = value;
                index += 1;
            }
        }
        if lengths[256] == 0 {
            return false; // no end of block code
        }

        lencode.build(&lengths[..nlen]);
        distcode.build(&lengths[nlen..nlen + ndist]);
        true
    }
}

/// `inflate_fixed_tables`.
fn inflate_fixed_tables(lencode: &mut Huffman, distcode: &mut Huffman) {
    let mut lengths = [0u8; 288];

    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..288].fill(8);
    lencode.build(&lengths);

    lengths[..30].fill(5);
    distcode.build(&lengths[..30]);
}

/// `inflate_raw`: a raw DEFLATE stream of at most `limit` bytes of output,
/// with the number of input bytes it consumed.
fn inflate_raw(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), PngError> {
    let mut s = Inflate {
        src: data,
        src_pos: 0,
        bit_buf: 0,
        bit_count: 0,
        out: Vec::new(),
        out_cap: 0,
        out_limit: limit,
        failed: false,
        no_memory: false,
    };
    let mut lencode = Huffman::EMPTY;
    let mut distcode = Huffman::EMPTY;

    loop {
        let last = s.bits(1);
        let kind = s.bits(2);

        if s.failed {
            return Err(PngError::Truncated);
        }
        let ok = match kind {
            0 => s.stored(),
            1 => {
                inflate_fixed_tables(&mut lencode, &mut distcode);
                s.block(&lencode, &distcode)
            }
            2 => s.dynamic_tables(&mut lencode, &mut distcode) && s.block(&lencode, &distcode),
            _ => false,
        };
        if !ok || s.failed {
            return Err(if s.no_memory { PngError::Memory } else { PngError::Deflate });
        }
        if last != 0 {
            break;
        }
    }

    Ok((s.out, s.src_pos))
}

/// `inflate_zlib`: a zlib stream (RFC 1950), header and Adler checksum
/// verified; output past `limit` bytes is refused as corrupted. A stream that
/// ends with fewer than four bytes after the DEFLATE data is taken without a
/// checksum, as the C takes it.
fn inflate_zlib(data: &[u8], limit: usize) -> Result<Vec<u8>, PngError> {
    if data.len() < 6 {
        return Err(PngError::Truncated);
    }
    if data[0] & 0x0f != 8 {
        return Err(PngError::Unsupported); // not deflate
    }
    if data[1] & 0x20 != 0 {
        return Err(PngError::Unsupported); // preset dictionary
    }
    if ((u32::from(data[0]) << 8) | u32::from(data[1])) % 31 != 0 {
        return Err(PngError::Deflate);
    }

    let (out, consumed) = inflate_raw(&data[2..], limit)?;

    if data.len() - 2 - consumed >= 4 {
        let stored = read_be32(&data[2 + consumed..]);
        if stored != adler32(&out) {
            return Err(PngError::Deflate);
        }
    }
    Ok(out)
}

/*============================ Image helpers ================================*/

/// The pixel count of an image handed to a transform or the encoder, refusing
/// what the C refuses as NULL (`src->pixels == NULL`) and the inconsistent
/// buffers the C API cannot be given (see the module documentation).
fn checked_count(image: &Image) -> Result<usize, PngError> {
    if image.pixels.is_empty() {
        return Err(PngError::Argument);
    }
    let (Ok(width), Ok(height)) = (usize::try_from(image.width), usize::try_from(image.height))
    else {
        return Err(PngError::Argument);
    };
    let count = width.checked_mul(height).ok_or(PngError::Argument)?;
    if count.checked_mul(4) != Some(image.pixels.len()) {
        return Err(PngError::Argument);
    }
    Ok(count)
}

/// `malloc` + `memcpy` of a pixel buffer.
fn copy_of(pixels: &[u8]) -> Result<Vec<u8>, PngError> {
    let mut work = Vec::new();
    work.try_reserve_exact(pixels.len()).map_err(|_| PngError::Memory)?;
    work.extend_from_slice(pixels);
    Ok(work)
}

/// Color channels are scaled by alpha so that filtering never drags the color
/// of fully transparent pixels into the visible ones.
fn premultiply(pixels: &mut [u8], count: usize) {
    for p in pixels.chunks_exact_mut(4).take(count) {
        let a = u32::from(p[3]);
        p[0] = ((u32::from(p[0]) * a + 127) / 255) as u8;
        p[1] = ((u32::from(p[1]) * a + 127) / 255) as u8;
        p[2] = ((u32::from(p[2]) * a + 127) / 255) as u8;
    }
}

fn unpremultiply(pixels: &mut [u8], count: usize) {
    for p in pixels.chunks_exact_mut(4).take(count) {
        let a = u32::from(p[3]);
        if a == 0 {
            p[0] = 0;
            p[1] = 0;
            p[2] = 0;
            continue;
        }
        for channel in &mut p[..3] {
            let v = (u32::from(*channel) * 255 + a / 2) / a;
            *channel = if v > 255 { 255 } else { v as u8 };
        }
    }
}

/// Samples the premultiplied buffer, everything outside the image is
/// transparent.
fn sample_bilinear(pixels: &[u8], width: i32, height: i32, x: f64, y: f64, out: &mut [u8]) {
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let wx = fx - f64::from(x0);
    let wy = fy - f64::from(y0);

    out[..4].fill(0);

    let mut acc = [0f64; 4];
    for dy in 0..2 {
        for dx in 0..2 {
            let sx = x0 + dx;
            let sy = y0 + dy;
            if sx < 0 || sy < 0 || sx >= width || sy >= height {
                continue;
            }
            let weight =
                (if dx != 0 { wx } else { 1.0 - wx }) * (if dy != 0 { wy } else { 1.0 - wy });
            let at = (sy as usize * width as usize + sx as usize) * 4;
            for c in 0..4 {
                // fma: png.c:417:48
                acc[c] = mul_add(weight, f64::from(pixels[at + c]), acc[c]);
            }
        }
    }
    for c in 0..4 {
        let v = acc[c] + 0.5;
        // `v < 0 ? 0 : (v > 255 ? 255 : v)`, which is what `clamp` does, NaN
        // passing through both; (uint8_t) of the result truncates.
        out[c] = v.clamp(0.0, 255.0) as u8;
    }
}

/*=============================== Transforms ================================*/

/// Rotates around the center keeping the canvas size (`png_rotate`): whatever
/// falls outside is cropped, whatever enters is transparent. Positive angles
/// turn clockwise on screen (y grows downwards).
pub fn rotate(src: &Image, radians: f64) -> Result<Image, PngError> {
    let count = checked_count(src)?;
    let mut work = copy_of(&src.pixels)?;
    premultiply(&mut work, count);

    let mut out = Image::alloc(src.width, src.height)?;

    let cx = f64::from(src.width) / 2.0;
    let cy = f64::from(src.height) / 2.0;
    // cos(radians), sin(radians): the canonical build computes the pair with
    // one sincos call (png.c:444), which crate::fp::sin_cos reproduces.
    let (sn, cs) = sin_cos(radians);

    for y in 0..out.height {
        for x in 0..out.width {
            let dx = f64::from(x) + 0.5 - cx;
            let dy = f64::from(y) + 0.5 - cy;
            // Inverse rotation: where does this destination pixel come from.
            // sx = cx + dx * cs + dy * sn
            // fma: png.c:450:28
            let sx = mul_add(dx, cs, cx);
            // fma: png.c:450:38
            let sx = mul_add(dy, sn, sx);
            // sy = cy - dx * sn + dy * cs
            // fma: png.c:451:28
            let sy = mul_add(-dx, sn, cy);
            // fma: png.c:451:38
            let sy = mul_add(dy, cs, sy);
            let at = (y as usize * out.width as usize + x as usize) * 4;
            sample_bilinear(&work, src.width, src.height, sx, sy, &mut out.pixels[at..at + 4]);
        }
    }
    unpremultiply(&mut out.pixels, count);
    Ok(out)
}

/// Box filtered when shrinking, bilinear when enlarging (`png_resize`).
pub fn resize(src: &Image, width: i32, height: i32) -> Result<Image, PngError> {
    if src.pixels.is_empty() {
        return Err(PngError::Argument);
    }
    if width <= 0 || height <= 0 {
        return Err(PngError::Argument);
    }

    let src_count = checked_count(src)?;
    let mut work = copy_of(&src.pixels)?;
    premultiply(&mut work, src_count);

    let mut out = Image::alloc(width, height)?;

    let scale_x = f64::from(src.width) / f64::from(width);
    let scale_y = f64::from(src.height) / f64::from(height);
    let shrinking = width <= src.width && height <= src.height;
    let src_width = src.width as usize;

    for y in 0..height {
        for x in 0..width {
            let at = (y as usize * width as usize + x as usize) * 4;
            let dst = &mut out.pixels[at..at + 4];

            if shrinking {
                // Box filter: average of every source pixel falling in the cell.
                let x0 = (f64::from(x) * scale_x).floor() as i32;
                let mut x1 = (f64::from(x + 1) * scale_x).ceil() as i32;
                let y0 = (f64::from(y) * scale_y).floor() as i32;
                let mut y1 = (f64::from(y + 1) * scale_y).ceil() as i32;
                if x1 <= x0 {
                    x1 = x0 + 1;
                }
                if y1 <= y0 {
                    y1 = y0 + 1;
                }
                if x1 > src.width {
                    x1 = src.width;
                }
                if y1 > src.height {
                    y1 = src.height;
                }

                // uint32_t sums, which wrap as the C's do on a huge cell.
                let mut acc = [0u32; 4];
                let mut samples = 0u32;
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let p = (sy as usize * src_width + sx as usize) * 4;
                        for c in 0..4 {
                            acc[c] = acc[c].wrapping_add(u32::from(work[p + c]));
                        }
                        samples = samples.wrapping_add(1);
                    }
                }
                if samples == 0 {
                    samples = 1;
                }
                for c in 0..4 {
                    dst[c] = (acc[c].wrapping_add(samples / 2) / samples) as u8;
                }
            } else {
                sample_bilinear(
                    &work,
                    src.width,
                    src.height,
                    (f64::from(x) + 0.5) * scale_x,
                    (f64::from(y) + 0.5) * scale_y,
                    dst,
                );
            }
        }
    }
    unpremultiply(&mut out.pixels, width as usize * height as usize);
    Ok(out)
}

/// Rotation at the source resolution followed by the resize, which is what
/// keeps the small sprites clean (`png_rotate_resize`).
pub fn rotate_resize(
    src: &Image,
    radians: f64,
    width: i32,
    height: i32,
) -> Result<Image, PngError> {
    let rotated = rotate(src, radians)?;
    resize(&rotated, width, height)
}

/// Recolors the image in place, alpha is never touched (`png_tint`). Like the
/// C, the empty image is left alone.
pub fn tint(image: &mut Image, r: u8, g: u8, b: u8, mode: TintMode) {
    if image.pixels.is_empty() {
        return;
    }

    // (size_t)width * (size_t)height, bounded by the buffer the C would overrun.
    let count = (image.width as usize).wrapping_mul(image.height as usize);
    let color = [r, g, b];

    for p in image.pixels.chunks_exact_mut(4).take(count) {
        for c in 0..3 {
            p[c] = match mode {
                TintMode::Replace => color[c],
                TintMode::Multiply => ((i32::from(p[c]) * i32::from(color[c]) + 127) / 255) as u8,
            };
        }
    }
}

/*============================== PNG decoding ===============================*/

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

fn read_be32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

fn paeth_predictor(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();

    if pa <= pb && pa <= pc {
        return a;
    }
    if pb <= pc {
        return b;
    }
    c
}

/// Everything IHDR, PLTE and tRNS say about how to read the raster
/// (`png_format_t`). The decoder reads every still PNG: grayscale at 1, 2, 4,
/// 8 and 16 bits, palette at 1, 2, 4 and 8, RGB, gray + alpha and RGBA at 8
/// and 16, each either as it is or Adam7 interlaced, and always brings it to
/// 8 bit RGBA.
struct Format {
    width: i32,
    height: i32,
    /// Bits per sample.
    bit_depth: i32,
    /// 0 gray, 2 RGB, 3 palette, 4 gray + alpha, 6 RGBA.
    color_type: i32,
    /// Samples per pixel.
    channels: i32,
    /// Adam7.
    interlaced: bool,
    /// PLTE entries, 0 until it is seen.
    palette_size: i32,
    /// tRNS on gray or RGB: pixels of that one color are transparent.
    has_key: bool,
    /// That color, at the full sample depth.
    key: [u32; 3],
    /// What each index expands to. Indices past PLTE are an error in the
    /// file; they are left transparent black rather than refused, so a stray
    /// index shows as a hole instead of costing the whole image, and every
    /// index a sample can hold (8 bits at most) lands inside the table
    /// without a check.
    palette: [u8; 256 * 4],
}

impl Format {
    /// The C's `memset(&f, 0, sizeof(f))`.
    const ZERO: Format = Format {
        width: 0,
        height: 0,
        bit_depth: 0,
        color_type: 0,
        channels: 0,
        interlaced: false,
        palette_size: 0,
        has_key: false,
        key: [0; 3],
        palette: [0; 256 * 4],
    };
}

/// Adam7 sends the image as seven reduced images, each with scanlines and
/// filters of its own: pass p holds the pixels at (x0 + i * dx, y0 + j * dy).
/// An image that is not interlaced is the single pass holding every pixel.
struct Pass {
    x0: i32,
    y0: i32,
    dx: i32,
    dy: i32,
}

const ADAM7: [Pass; 7] = [
    Pass { x0: 0, y0: 0, dx: 8, dy: 8 },
    Pass { x0: 4, y0: 0, dx: 8, dy: 8 },
    Pass { x0: 0, y0: 4, dx: 4, dy: 8 },
    Pass { x0: 2, y0: 0, dx: 4, dy: 4 },
    Pass { x0: 0, y0: 2, dx: 2, dy: 4 },
    Pass { x0: 1, y0: 0, dx: 2, dy: 2 },
    Pass { x0: 0, y0: 1, dx: 1, dy: 2 },
];
const WHOLE: [Pass; 1] = [Pass { x0: 0, y0: 0, dx: 1, dy: 1 }];

fn passes(f: &Format) -> &'static [Pass] {
    if f.interlaced { &ADAM7 } else { &WHOLE }
}

/// Size of the reduced image a pass carries, 0 wide or 0 high when the image
/// is too small to reach it.
fn pass_size(f: &Format, pass: &Pass) -> (i32, i32) {
    let width = if f.width > pass.x0 { (f.width - pass.x0 + pass.dx - 1) / pass.dx } else { 0 };
    let height = if f.height > pass.y0 { (f.height - pass.y0 + pass.dy - 1) / pass.dy } else { 0 };
    (width, height)
}

/// Bytes in a scanline of that many pixels, filter byte excluded: at most
/// 16384 pixels of 64 bits, so it never comes near overflowing.
fn row_bytes(f: &Format, width: i32) -> usize {
    (width as usize * (f.channels * f.bit_depth) as usize).div_ceil(8)
}

/// The byte distance the filters look back: one pixel, or one byte when
/// pixels are smaller than that.
fn filter_unit(f: &Format) -> usize {
    let bytes = (f.channels * f.bit_depth) as usize / 8;
    if bytes != 0 { bytes } else { 1 }
}

/// Exact length of the inflated raster: for every pass, its scanlines each
/// with their filter byte. A pass with no pixel sends nothing, not even filter
/// bytes. 0 if it could not be represented, which the size limits already
/// rule out.
fn raster_length(f: &Format) -> usize {
    let mut total = 0usize;

    for pass in passes(f) {
        let (width, height) = pass_size(f, pass);
        if width == 0 || height == 0 {
            continue;
        }

        let row = row_bytes(f, width) + 1;
        if row > (usize::MAX - total) / height as usize {
            return 0;
        }
        total += row * height as usize;
    }
    total
}

fn depth_allowed(color_type: i32, bit_depth: i32) -> bool {
    match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(bit_depth, 8 | 16),
        _ => false,
    }
}

/// Validates the 13 bytes of IHDR (`png_parse_header`): the size limits, a
/// bit depth the color type allows, the one compression and filter method
/// there is, no interlacing or Adam7.
fn parse_header(body: &[u8], f: &mut Format) -> Result<(), PngError> {
    let width = read_be32(body);
    let height = read_be32(&body[4..]);
    let bit_depth = i32::from(body[8]);
    let color_type = i32::from(body[9]);

    if width == 0
        || height == 0
        || width > MAX_DIMENSION as u32
        || height > MAX_DIMENSION as u32
        || u64::from(width) * u64::from(height) > MAX_PIXELS as u64
    {
        return Err(PngError::Unsupported);
    }
    if !depth_allowed(color_type, bit_depth) || body[10] != 0 || body[11] != 0 || body[12] > 1 {
        return Err(PngError::Unsupported);
    }

    f.width = width as i32;
    f.height = height as i32;
    f.bit_depth = bit_depth;
    f.color_type = color_type;
    f.channels = match color_type {
        2 => 3,
        4 => 2,
        6 => 4,
        _ => 1,
    };
    f.interlaced = body[12] != 0;
    Ok(())
}

/// Undoes the per scanline filters, in place, on one pass of the raw (still
/// packed) raster (`png_unfilter`): `height` rows of a filter byte followed by
/// `stride` bytes. The filters work on bytes, the left neighbor being `unit`
/// bytes back. False on an unknown filter type.
fn unfilter(raster: &mut [u8], stride: usize, height: i32, unit: usize) -> bool {
    let mut previous: Option<usize> = None;
    let mut row = 0usize;

    for _ in 0..height {
        let filter = raster[row];
        let current = row + 1;

        for i in 0..stride {
            let a = if i >= unit { i32::from(raster[current + i - unit]) } else { 0 };
            let b = match previous {
                Some(above) => i32::from(raster[above + i]),
                None => 0,
            };
            let c = match previous {
                Some(above) if i >= unit => i32::from(raster[above + i - unit]),
                _ => 0,
            };
            let mut value = i32::from(raster[current + i]);

            match filter {
                0 => {}
                1 => value += a,
                2 => value += b,
                3 => value += (a + b) / 2,
                4 => value += paeth_predictor(a, b, c),
                _ => return false,
            }
            raster[current + i] = value as u8;
        }
        previous = Some(current);
        row += stride + 1;
    }
    true
}

/// Sample number `index` of an unfiltered scanline, at its full depth
/// (`png_sample`). Samples under 8 bits are packed from the most significant
/// bit down and never straddle two bytes.
fn sample(row: &[u8], index: usize, bit_depth: i32) -> u32 {
    if bit_depth == 8 {
        return u32::from(row[index]);
    }
    if bit_depth == 16 {
        return (u32::from(row[2 * index]) << 8) | u32::from(row[2 * index + 1]);
    }

    let bit = index * bit_depth as usize;
    let shift = 8 - bit_depth as u32 - (bit % 8) as u32;
    (u32::from(row[bit / 8]) >> shift) & ((1u32 << bit_depth) - 1)
}

/// A sample brought to 8 bits (`png_to_8`). Small ones are stretched so that
/// the largest value is 255. 16 bit ones keep their high byte, as libpng's
/// strip does: exact for 8 bit data widened by 257, never more than one level
/// from the rounded value otherwise, and no division per sample.
fn to_8(value: u32, bit_depth: i32) -> u8 {
    if bit_depth == 16 {
        return (value >> 8) as u8;
    }
    if bit_depth == 8 {
        return value as u8;
    }
    (value * 255 / ((1u32 << bit_depth) - 1)) as u8
}

/// Pixel x of an unfiltered scanline, as straight RGBA (`png_pixel`).
fn pixel(f: &Format, row: &[u8], x: usize, rgba: &mut [u8]) {
    let mut s = [0u32; 4];

    if f.color_type == 3 {
        let index = sample(row, x, f.bit_depth) as usize;
        rgba[..4].copy_from_slice(&f.palette[4 * index..4 * index + 4]);
        return;
    }
    let channels = f.channels as usize;
    for (c, value) in s.iter_mut().enumerate().take(channels) {
        *value = sample(row, x * channels + c, f.bit_depth);
    }

    match f.color_type {
        0 => {
            let gray = to_8(s[0], f.bit_depth);
            rgba[..3].fill(gray);
            rgba[3] = if f.has_key && s[0] == f.key[0] { 0 } else { 255 };
        }
        4 => {
            let gray = to_8(s[0], f.bit_depth);
            rgba[..3].fill(gray);
            rgba[3] = to_8(s[1], f.bit_depth);
        }
        2 => {
            for c in 0..3 {
                rgba[c] = to_8(s[c], f.bit_depth);
            }
            rgba[3] = if f.has_key && s[0] == f.key[0] && s[1] == f.key[1] && s[2] == f.key[2] {
                0
            } else {
                255
            };
        }
        _ => {
            for c in 0..4 {
                rgba[c] = to_8(s[c], f.bit_depth);
            }
        }
    }
}

/// Unfilters every pass of the raster in place, false on an unknown filter
/// type (`png_unfilter_passes`).
fn unfilter_passes(f: &Format, raster: &mut [u8]) -> bool {
    let mut offset = 0usize;

    for pass in passes(f) {
        let (width, height) = pass_size(f, pass);
        if width == 0 || height == 0 {
            continue;
        }

        let stride = row_bytes(f, width);
        if !unfilter(&mut raster[offset..], stride, height, filter_unit(f)) {
            return false;
        }
        offset += (stride + 1) * height as usize;
    }
    true
}

/// Expands the unfiltered raster into straight RGBA, scattering the pixels of
/// each pass to where they belong in the full image (`png_expand`).
fn expand(f: &Format, raster: &[u8], rgba: &mut [u8]) {
    let mut offset = 0usize;

    for pass in passes(f) {
        let (width, height) = pass_size(f, pass);
        if width == 0 || height == 0 {
            continue;
        }

        let stride = row_bytes(f, width);
        for y in 0..height {
            let start = offset + (stride + 1) * y as usize + 1;
            let row = &raster[start..start + stride];
            let dst_y = pass.y0 as usize + y as usize * pass.dy as usize;

            for x in 0..width {
                let dst_x = pass.x0 as usize + x as usize * pass.dx as usize;
                let at = (dst_y * f.width as usize + dst_x) * 4;
                pixel(f, row, x as usize, &mut rgba[at..at + 4]);
            }
        }
        offset += (stride + 1) * height as usize;
    }
}

/// Decodes an in memory PNG file into a RGBA image (`png_decode`). Images over
/// 16384 pixels a side or 64 M pixels are [`PngError::Unsupported`], and
/// compressed data that inflates past the size IHDR gives is refused
/// ([`PngError::Deflate`]) as soon as it does.
///
/// The C refuses NULL data as [`PngError::Argument`]; a slice always exists,
/// so no input reaches that status here.
pub fn decode(data: &[u8]) -> Result<Image, PngError> {
    let length = data.len();
    if length < SIGNATURE.len() {
        return Err(PngError::Truncated);
    }
    if data[..SIGNATURE.len()] != SIGNATURE {
        return Err(PngError::Signature);
    }

    let mut f = Format::ZERO;
    let mut idat: Vec<u8> = Vec::new();
    let mut idat_cap = 0usize; // what the C has asked realloc for
    let mut pos = SIGNATURE.len();
    let (mut seen_header, mut seen_idat, mut seen_trns, mut seen_end) =
        (false, false, false, false);

    while length - pos >= 8 {
        let chunk_len = read_be32(&data[pos..]);
        let kind = &data[pos + 4..pos + 8];

        if chunk_len > 0x7fff_ffff || length - pos < 12 || chunk_len as usize > length - pos - 12 {
            return Err(PngError::Truncated);
        }
        let chunk_len = chunk_len as usize;
        let body = &data[pos + 8..pos + 8 + chunk_len];
        if crc32(&data[pos + 4..pos + 8 + chunk_len]) != read_be32(&data[pos + 8 + chunk_len..]) {
            return Err(PngError::Crc);
        }

        match kind {
            b"IHDR" => {
                if chunk_len != 13 || seen_header {
                    return Err(PngError::Chunk);
                }
                parse_header(body, &mut f)?;
                seen_header = true;
            }
            b"PLTE" => {
                if !seen_header {
                    return Err(PngError::Chunk);
                }
                // Read for palette images only: for RGB it is a hint for
                // displays that cannot show true color, and gray has no use
                // for it.
                if f.color_type == 3 {
                    if f.palette_size > 0
                        || seen_idat
                        || chunk_len == 0
                        || !chunk_len.is_multiple_of(3)
                        || chunk_len > 256 * 3
                    {
                        return Err(PngError::Chunk);
                    }
                    f.palette_size = (chunk_len / 3) as i32;
                    for i in 0..f.palette_size as usize {
                        f.palette[4 * i..4 * i + 3].copy_from_slice(&body[3 * i..3 * i + 3]);
                        f.palette[4 * i + 3] = 255;
                    }
                }
            }
            b"tRNS" => {
                if !seen_header || seen_trns || seen_idat {
                    return Err(PngError::Chunk);
                }
                seen_trns = true;
                if f.color_type == 3 {
                    // One alpha per palette entry, those it does not reach
                    // stay opaque.
                    if f.palette_size == 0 || chunk_len > f.palette_size as usize {
                        return Err(PngError::Chunk);
                    }
                    for (i, &alpha) in body.iter().enumerate() {
                        f.palette[4 * i + 3] = alpha;
                    }
                } else if f.color_type == 0 || f.color_type == 2 {
                    // One color, two bytes per sample whatever the depth:
                    // compared with the samples before they are brought to 8
                    // bits, the bits above the depth masked off as the
                    // specification asks.
                    let samples = if f.color_type == 0 { 1 } else { 3 };
                    let mask = (1u32 << f.bit_depth) - 1;
                    if chunk_len != 2 * samples {
                        return Err(PngError::Chunk);
                    }
                    for c in 0..samples {
                        f.key[c] =
                            ((u32::from(body[2 * c]) << 8) | u32::from(body[2 * c + 1])) & mask;
                    }
                    f.has_key = true;
                }
                // Next to an alpha channel it has nothing to add, and is
                // ignored.
            }
            b"IDAT" => {
                if !seen_header || (f.color_type == 3 && f.palette_size == 0) {
                    return Err(PngError::Chunk);
                }
                seen_idat = true;
                if chunk_len > idat_cap - idat.len() {
                    let mut cap = if idat_cap != 0 { idat_cap } else { 8192 };
                    while chunk_len > cap - idat.len() {
                        if cap > usize::MAX / 2 {
                            return Err(PngError::Memory);
                        }
                        cap *= 2;
                    }
                    // realloc(idat, cap)
                    idat.try_reserve_exact(cap - idat.len()).map_err(|_| PngError::Memory)?;
                    idat_cap = cap;
                }
                idat.extend_from_slice(body);
            }
            b"IEND" => {
                seen_end = true;
                break;
            }
            _ => {}
        }
        pos += 12 + chunk_len;
    }

    if !seen_header || idat.is_empty() || !seen_end {
        return Err(if seen_end { PngError::Chunk } else { PngError::Truncated });
    }

    // The header says exactly how long the raster is, and inflating stops at
    // that: a small file that would expand into gigabytes is refused at the
    // first byte too many, and a raster that comes out short is refused too.
    let expected = raster_length(&f);
    if expected == 0 {
        return Err(PngError::Unsupported);
    }
    let inflated = inflate_zlib(&idat, expected);
    drop(idat);
    let mut raster = inflated?;

    if raster.len() != expected {
        return Err(PngError::Truncated);
    }
    // Every pass is unfiltered before the image is allocated, so that a bad
    // filter leaves nothing allocated, like every other failure.
    if !unfilter_passes(&f, &mut raster) {
        return Err(PngError::Chunk);
    }

    let mut out = Image::alloc(f.width, f.height)?;
    expand(&f, &raster, &mut out.pixels);
    Ok(out)
}

/*=============================== Deflate ===================================
 *
 * LZ77 with fixed Huffman codes (RFC 1951, 3.2.6). The inflater above has
 * always been able to read this; the encoder only ever wrote stored blocks,
 * because the sprites are a few hundred bytes each and the ratio did not
 * matter. Then a snapshot of a 1600x800 screen turned out to be four
 * megabytes, and a run at --size 64 spent seconds pushing ten megabytes of
 * base64 at the terminal before the first frame. It matters now.
 *
 * Fixed codes rather than dynamic: no tree to build, no second pass, and on
 * this data, which is long runs of one colour and long runs of transparency,
 * it gets within a few percent of what a dynamic tree would. Stored blocks
 * remain the fallback for anything that comes out larger than it went in.
 */

const DEFLATE_WINDOW: usize = 32768;
const DEFLATE_MIN_MATCH: usize = 3;
const DEFLATE_MAX_MATCH: usize = 258;
const DEFLATE_HASH_BITS: u32 = 15;
const DEFLATE_HASH_SIZE: usize = 1 << DEFLATE_HASH_BITS;
/// Matches tried per position: quality against time.
const DEFLATE_CHAIN_LIMIT: i32 = 160;

/// `bitwriter_t`, `out.len()` being its `length` and `capacity` what the C
/// has asked `realloc` for.
struct BitWriter {
    out: Vec<u8>,
    capacity: usize,
    bits: u32,
    bit_count: i32,
    failed: bool,
}

impl BitWriter {
    /// `bits_reserve`.
    fn reserve(&mut self, extra: usize) {
        if self.failed {
            return;
        }
        if self.out.len() + extra <= self.capacity {
            return;
        }
        let mut capacity = if self.capacity != 0 { self.capacity } else { 4096 };
        while capacity < self.out.len() + extra {
            match capacity.checked_mul(2) {
                Some(doubled) => capacity = doubled,
                None => {
                    self.failed = true;
                    return;
                }
            }
        }
        if self.out.try_reserve_exact(capacity - self.out.len()).is_err() {
            self.failed = true;
            return;
        }
        self.capacity = capacity;
    }

    /// The bit stream is least significant bit first within each byte
    /// (`put_bits`). Once an allocation has failed the stream is discarded
    /// whole, so nothing more is accumulated (the C keeps shifting bits it
    /// will throw away, past the width of the register).
    fn put_bits(&mut self, value: u32, count: i32) {
        if self.failed {
            return;
        }
        self.bits |= (value & ((1u32 << count) - 1)) << self.bit_count;
        self.bit_count += count;
        while self.bit_count >= 8 {
            self.reserve(1);
            if self.failed {
                return;
            }
            self.out.push((self.bits & 0xff) as u8);
            self.bits >>= 8;
            self.bit_count -= 8;
        }
    }

    /// Huffman codes are defined most significant bit first, so they go in
    /// reversed (`put_code`).
    fn put_code(&mut self, code: u32, length: i32) {
        let mut reversed = 0u32;
        for i in 0..length {
            reversed |= ((code >> i) & 1) << (length - 1 - i);
        }
        self.put_bits(reversed, length);
    }

    fn put_literal(&mut self, symbol: i32) {
        if symbol < 144 {
            self.put_code((0x30 + symbol) as u32, 8);
        } else {
            self.put_code((0x190 + symbol - 144) as u32, 9);
        }
    }

    fn put_end_of_block(&mut self) {
        self.put_code(0, 7); // Symbol 256.
    }

    fn put_length(&mut self, length: i32) {
        let mut index = 28;
        while index > 0 && length < i32::from(LENGTH_BASE[index]) {
            index -= 1;
        }
        let symbol = 257 + index as i32;
        // 256 to 279 are seven bits, 280 to 287 are eight: the fixed table.
        if symbol <= 279 {
            self.put_code((symbol - 256) as u32, 7);
        } else {
            self.put_code((0xc0 + symbol - 280) as u32, 8);
        }
        if LENGTH_EXTRA[index] != 0 {
            self.put_bits(
                (length - i32::from(LENGTH_BASE[index])) as u32,
                i32::from(LENGTH_EXTRA[index]),
            );
        }
    }

    fn put_distance(&mut self, distance: i32) {
        let mut symbol = 29;
        while symbol > 0 && distance < i32::from(DIST_BASE[symbol]) {
            symbol -= 1;
        }
        self.put_code(symbol as u32, 5);
        if DIST_EXTRA[symbol] != 0 {
            self.put_bits(
                (distance - i32::from(DIST_BASE[symbol])) as u32,
                i32::from(DIST_EXTRA[symbol]),
            );
        }
    }
}

fn deflate_hash(data: &[u8], at: usize) -> usize {
    (((i32::from(data[at]) << 10) ^ (i32::from(data[at + 1]) << 5) ^ i32::from(data[at + 2]))
        & (DEFLATE_HASH_SIZE as i32 - 1)) as usize
}

/// Longest match for the bytes at `at`, searched back along the hash chain
/// (`longest_match`).
fn longest_match(
    data: &[u8],
    at: usize,
    head: &[i32],
    prev: &[i32],
    best_distance: &mut i32,
) -> i32 {
    let mut best = 0i32;
    let mut limit = data.len() - at;
    if limit > DEFLATE_MAX_MATCH {
        limit = DEFLATE_MAX_MATCH;
    }
    if limit < DEFLATE_MIN_MATCH {
        return 0;
    }

    let mut candidate = head[deflate_hash(data, at)];
    let mut tries = 0;
    while candidate >= 0 && tries < DEFLATE_CHAIN_LIMIT {
        // size_t distance = at - (size_t)candidate
        let distance = at.wrapping_sub(candidate as usize);
        if distance == 0 || distance > DEFLATE_WINDOW {
            break;
        }
        let from = candidate as usize;
        if data[from + best as usize] == data[at + best as usize] {
            let mut run = 0usize;
            while run < limit && data[from + run] == data[at + run] {
                run += 1;
            }
            if run as i32 > best {
                best = run as i32;
                *best_distance = distance as i32;
                if best >= limit as i32 {
                    break; // Cannot do better than the limit.
                }
            }
        }
        candidate = prev[candidate as usize & (DEFLATE_WINDOW - 1)];
        tries += 1;
    }
    if best >= DEFLATE_MIN_MATCH as i32 { best } else { 0 }
}

/// A `malloc`ed table of `-1`s, `None` when it cannot be allocated.
fn table_of_empty(length: usize) -> Option<Vec<i32>> {
    let mut table = Vec::new();
    table.try_reserve_exact(length).ok()?;
    table.resize(length, -1);
    Some(table)
}

/// One fixed Huffman block, final (`deflate_fixed`). `None` on failure, which
/// the encoder answers with stored blocks rather than an error.
fn deflate_fixed(data: &[u8]) -> Option<Vec<u8>> {
    let mut w = BitWriter { out: Vec::new(), capacity: 0, bits: 0, bit_count: 0, failed: false };
    let mut head = table_of_empty(DEFLATE_HASH_SIZE)?;
    let mut prev = table_of_empty(DEFLATE_WINDOW)?;
    let length = data.len();

    w.put_bits(1, 1); // Final block.
    w.put_bits(1, 2); // Fixed Huffman.

    let mut at = 0usize;
    while at < length {
        let mut distance = 0i32;
        let mut matched = 0i32;
        if at + DEFLATE_MIN_MATCH <= length {
            matched = longest_match(data, at, &head, &prev, &mut distance);
        }

        if matched >= DEFLATE_MIN_MATCH as i32 {
            w.put_length(matched);
            w.put_distance(distance);
        } else {
            w.put_literal(i32::from(data[at]));
            matched = 1;
        }
        // Every position the match covered still goes in the chains, or the
        // next search starts blind.
        let mut i = 0usize;
        while (i as i32) < matched && at + i + DEFLATE_MIN_MATCH <= length {
            let here = at + i;
            let slot = deflate_hash(data, here);
            prev[here & (DEFLATE_WINDOW - 1)] = head[slot];
            head[slot] = here as i32; // (int)here
            i += 1;
        }
        at += matched as usize;
        if w.failed {
            break;
        }
    }
    w.put_end_of_block();
    if w.bit_count > 0 {
        w.put_bits(0, 8 - w.bit_count);
    }

    if w.failed {
        return None;
    }
    Some(w.out)
}

/*============================== PNG encoding ===============================*/

/// Appends a chunk: length, type, body, and the CRC of type and body
/// (`write_chunk`).
fn write_chunk(png: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    png.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(body);
    let crc = crc32(&png[start..]);
    png.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes a RGBA image into an in memory PNG file (`png_encode`): 8 bit
/// RGBA, filter none on every scanline, one IDAT holding a fixed Huffman
/// stream, or stored blocks when that does not come out smaller than the raw
/// scanlines.
pub fn encode(image: &Image) -> Result<Vec<u8>, PngError> {
    if image.pixels.is_empty() {
        return Err(PngError::Argument);
    }
    if image.width <= 0 || image.height <= 0 {
        return Err(PngError::Argument);
    }
    checked_count(image)?;

    let width = image.width as usize;
    let height = image.height as usize;
    // Bounded by the pixel buffer that exists, so neither can overflow.
    let stride = width * 4 + 1; // one filter byte per scanline
    let raw_len = stride * height;

    let mut raw = Vec::new();
    raw.try_reserve_exact(raw_len).map_err(|_| PngError::Memory)?;
    for row in image.pixels.chunks_exact(width * 4) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }

    // Compressed if it helps, stored if it does not: incompressible data must
    // not come out larger than it went in.
    let squeezed = deflate_fixed(&raw).filter(|squeezed| squeezed.len() < raw_len);

    let mut blocks = raw_len.div_ceil(DEFLATE_MAX_BLOCK);
    if blocks == 0 {
        blocks = 1;
    }
    let zlib_len = match &squeezed {
        Some(squeezed) => 2 + squeezed.len() + 4,
        None => 2 + blocks * 5 + raw_len + 4,
    };
    let total = SIGNATURE.len() + (12 + 13) + (12 + zlib_len) + 12;

    let mut png = Vec::new();
    png.try_reserve_exact(total).map_err(|_| PngError::Memory)?;
    let mut zlib = Vec::new();
    zlib.try_reserve_exact(zlib_len).map_err(|_| PngError::Memory)?;

    zlib.push(0x78); // deflate, 32k window
    zlib.push(0x01); // no dictionary, checksum of the two bytes is a multiple of 31
    let mut offset = 0usize;
    if let Some(squeezed) = &squeezed {
        zlib.extend_from_slice(squeezed);
        offset = raw_len;
        blocks = 0;
    }
    for i in 0..blocks {
        let chunk = (raw_len - offset).min(DEFLATE_MAX_BLOCK);

        zlib.push(if i + 1 == blocks { 1 } else { 0 }); // stored block, final flag
        zlib.push((chunk & 0xff) as u8);
        zlib.push((chunk >> 8) as u8);
        zlib.push((!chunk & 0xff) as u8);
        zlib.push(((!chunk >> 8) & 0xff) as u8);
        zlib.extend_from_slice(&raw[offset..offset + chunk]);
        offset += chunk;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&(image.width as u32).to_be_bytes());
    header[4..8].copy_from_slice(&(image.height as u32).to_be_bytes());
    header[8] = 8; // bit depth
    header[9] = 6; // RGBA
    header[10] = 0; // deflate
    header[11] = 0; // adaptive filtering
    header[12] = 0; // no interlace

    png.extend_from_slice(&SIGNATURE);
    write_chunk(&mut png, b"IHDR", &header);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", &[]);
    Ok(png)
}

#[cfg(test)]
pub(crate) fn inflate_for_test(data: &[u8], limit: usize) -> Result<Vec<u8>, PngError> {
    inflate_zlib(data, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_their_specifications() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn a_zlib_stream_without_a_full_checksum_is_taken() {
        // Stored block of "ab", then only three bytes where the Adler goes.
        let stream = [0x78, 0x01, 0x01, 0x02, 0x00, 0xfd, 0xff, b'a', b'b', 0, 0, 0];
        assert_eq!(inflate_zlib(&stream, 2), Ok(b"ab".to_vec()));
        // A wrong one in full is refused.
        let stream = [0x78, 0x01, 0x01, 0x02, 0x00, 0xfd, 0xff, b'a', b'b', 0, 0, 0, 0];
        assert_eq!(inflate_zlib(&stream, 2), Err(PngError::Deflate));
    }

    #[test]
    fn inflating_stops_at_the_limit() {
        let stream = [0x78, 0x01, 0x01, 0x02, 0x00, 0xfd, 0xff, b'a', b'b'];
        assert_eq!(inflate_zlib(&stream, 1), Err(PngError::Deflate));
    }
}
