//! Portable Sixel output. Every frame paints its background as well as birds:
//! transparent Sixel pixels would otherwise leave the previous frame behind.
//! The 256-entry palette is stable across frames (cube + greys + background).
//!
//! An image is written a band of six rows at a time. In big-flock mode the
//! bands are written on several threads: first every band's colours, then,
//! knowing from those which colours the bands above have defined, every
//! band's text, and the bands are joined in order, so the image is the one
//! a single thread writes.

use crate::image::Image;
use crate::parallel;
use crate::render::kitty::{KittyError, KittyGraphics};
use crate::sprites::PICTURE_GROUND;

#[derive(Debug, Default)]
pub struct Sixel {
    bytes: Vec<u8>,
    /// For each thread, one row of six-pixel columns per palette colour.
    /// Writing a band clears the rows it used, so they are all zero between
    /// bands.
    planes: Vec<Vec<u8>>,
    /// Big-flock mode's bands, written apart and then joined.
    bands: Vec<Band>,
}

/// One band of six rows, or fewer at the bottom, written on its own.
#[derive(Debug, Default)]
struct Band {
    /// Each pixel's palette index, row after row.
    indices: Vec<u8>,
    /// The colours the band uses, a bit each.
    used: Palette,
    /// The colours the bands above it define.
    defined: Palette,
    bytes: Vec<u8>,
    failed: Option<KittyError>,
}

/// A set of palette indices.
type Palette = [u64; 4];

fn has(set: &Palette, c: usize) -> bool {
    set[c / 64] & 1 << (c % 64) != 0
}

fn add(set: &mut Palette, c: usize) {
    set[c / 64] |= 1 << (c % 64);
}

/// Bands a thread takes at a time.
const BANDS_PER_BLOCK: usize = 2;

fn palette(index: usize) -> [u8; 3] {
    if index == 0 {
        return PICTURE_GROUND;
    }
    if index <= 216 {
        let i = index - 1;
        return [((i / 36) * 51) as u8, (((i / 6) % 6) * 51) as u8, ((i % 6) * 51) as u8];
    }
    let grey = ((index - 217) * 255 / 38) as u8;
    [grey; 3]
}
fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.into_iter().zip(b).map(|(a, b)| (i32::from(a) - i32::from(b)).pow(2) as u32).sum()
}
fn colour(pixel: &[u8]) -> usize {
    let alpha = u32::from(pixel[3]);
    let rgb = std::array::from_fn(|c| {
        ((u32::from(pixel[c]) * alpha + u32::from(PICTURE_GROUND[c]) * (255 - alpha) + 127) / 255)
            as u8
    });
    if rgb == PICTURE_GROUND {
        return 0;
    }
    let cube = 1
        + ((usize::from(rgb[0]) + 25) / 51) * 36
        + ((usize::from(rgb[1]) + 25) / 51) * 6
        + (usize::from(rgb[2]) + 25) / 51;
    let grey = 217 + (rgb.into_iter().map(usize::from).sum::<usize>() * 38 + 382) / 765;
    [0, cube, grey].into_iter().min_by_key(|&i| distance(rgb, palette(i))).unwrap()
}

/// `colour` remembered: a frame is mostly background, and the birds are a
/// few flat tints, so nearly every pixel is one seen a moment before.
struct Colours {
    /// Pixels as little-endian words, each in the slot its hash picks. Zero
    /// (transparent black) is never kept, so it marks an empty slot.
    pixels: [u32; COLOUR_SLOTS],
    indices: [u8; COLOUR_SLOTS],
    /// The pixel before, which is usually this one.
    last: u32,
    last_index: u8,
}

const COLOUR_SLOTS: usize = 1024;
const GROUND: u32 =
    u32::from_le_bytes([PICTURE_GROUND[0], PICTURE_GROUND[1], PICTURE_GROUND[2], 255]);

impl Default for Colours {
    fn default() -> Colours {
        // The background is palette entry 0.
        Colours {
            pixels: [0; COLOUR_SLOTS],
            indices: [0; COLOUR_SLOTS],
            last: GROUND,
            last_index: 0,
        }
    }
}

impl Colours {
    #[inline]
    fn of(&mut self, pixel: &[u8]) -> usize {
        let word = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
        if word == self.last {
            return usize::from(self.last_index);
        }
        let slot = (word.wrapping_mul(0x9E37_79B9) >> 22) as usize;
        let c = if word != 0 && self.pixels[slot] == word {
            self.indices[slot]
        } else {
            let c = colour(pixel) as u8;
            self.pixels[slot] = word;
            self.indices[slot] = c;
            c
        };
        self.last = word;
        self.last_index = c;
        usize::from(c)
    }
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), KittyError> {
    out.try_reserve(bytes.len()).map_err(|_| KittyError::Memory)?;
    out.extend_from_slice(bytes);
    Ok(())
}
fn number(out: &mut Vec<u8>, mut value: usize) -> Result<(), KittyError> {
    let mut digits = [0; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    put(out, &digits[at..])
}
fn run(out: &mut Vec<u8>, value: u8, count: usize) -> Result<(), KittyError> {
    if count >= 4 {
        put(out, b"!")?;
        number(out, count)?;
        put(out, &[value])
    } else {
        for _ in 0..count {
            put(out, &[value])?;
        }
        Ok(())
    }
}
/// The length of the run of `row[0]` at the start of `row`. Most runs are
/// long, empty columns or background, so they are measured eight at a time.
fn run_length(row: &[u8]) -> usize {
    let value = row[0];
    let splat = u64::from_ne_bytes([value; 8]);
    let mut length = 1;
    for word in row[1..].chunks_exact(8) {
        if u64::from_ne_bytes(word.try_into().unwrap()) != splat {
            break;
        }
        length += 8;
    }
    length + row[length..].iter().position(|&b| b != value).unwrap_or(row.len() - length)
}

/// A band's colour definitions and runs, from `planes` holding its six rows
/// and `ends` each colour's last column. The rows used are cleared again
/// whether or not the text fits.
fn write_band(
    planes: &mut [u8],
    width: usize,
    ends: &[usize; 256],
    defined: &mut Palette,
    out: &mut Vec<u8>,
) -> Result<(), KittyError> {
    let written = write_runs(planes, width, ends, defined, out);
    // A colour's bits all lie before its end, and only the colours in this
    // band have any: clearing those rows leaves the planes zero.
    for (c, &end) in ends.iter().enumerate() {
        if end != 0 {
            planes[c * width..c * width + end].fill(0);
        }
    }
    written
}

fn write_runs(
    planes: &[u8],
    width: usize,
    ends: &[usize; 256],
    defined: &mut Palette,
    out: &mut Vec<u8>,
) -> Result<(), KittyError> {
    let mut first = true;
    for (c, &end) in ends.iter().enumerate() {
        if end == 0 {
            continue;
        }
        if !first {
            put(out, b"$")?;
        }
        first = false;
        put(out, b"#")?;
        number(out, c)?;
        if !has(defined, c) {
            put(out, b";2")?;
            for component in palette(c) {
                put(out, b";")?;
                number(out, (usize::from(component) * 100 + 127) / 255)?;
            }
            add(defined, c);
        }
        let row = &planes[c * width..c * width + end];
        let mut x = 0;
        while x < end {
            let length = run_length(&row[x..]);
            run(out, row[x] + b'?', length)?;
            x += length;
        }
    }
    Ok(())
}

/// Sets a pixel of palette colour `c` in row `dy` of a band's planes.
#[inline]
fn plot(planes: &mut [u8], width: usize, ends: &mut [usize; 256], c: usize, x: usize, dy: usize) {
    planes[c * width + x] |= 1 << dy;
    ends[c] = ends[c].max(x + 1);
}

/// Planes for every thread, each a row per palette colour `width` wide.
fn prepare_planes(
    planes: &mut Vec<Vec<u8>>,
    threads: usize,
    width: usize,
) -> Result<(), KittyError> {
    let size = width.checked_mul(256).ok_or(KittyError::Memory)?;
    if planes.len() < threads {
        planes.try_reserve(threads - planes.len()).map_err(|_| KittyError::Memory)?;
        planes.resize_with(threads, Vec::new);
    }
    for plane in &mut planes[..threads] {
        if plane.len() != size {
            plane.clear();
            plane.try_reserve_exact(size).map_err(|_| KittyError::Memory)?;
            plane.resize(size, 0);
        }
    }
    Ok(())
}

impl Sixel {
    /// Encodes an RGBA image, flattening alpha onto the picture background.
    /// Colour definitions are sent in each image so it is self-contained.
    pub fn encode(&mut self, image: &Image) -> Result<&[u8], KittyError> {
        self.encode_in_parallel(1, image)
    }

    /// [`Sixel::encode`] on up to `threads` threads (big-flock mode), with
    /// the same result.
    pub fn encode_in_parallel(
        &mut self,
        threads: usize,
        image: &Image,
    ) -> Result<&[u8], KittyError> {
        self.bytes.clear();
        let width = usize::try_from(image.width).map_err(|_| KittyError::Argument)?;
        let height = usize::try_from(image.height).map_err(|_| KittyError::Argument)?;
        if width == 0
            || height == 0
            || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(image.pixels.len())
        {
            return Err(KittyError::Argument);
        }
        let bands = height.div_ceil(6);
        let threads = threads.clamp(1, bands.div_ceil(BANDS_PER_BLOCK));
        prepare_planes(&mut self.planes, threads, width)?;
        let out = &mut self.bytes;
        // P2=1 leaves pixels outside the raster alone. Inside the raster,
        // every pixel (including background) is explicitly painted.
        put(out, b"\x1bP0;1q\"1;1;")?;
        number(out, width)?;
        put(out, b";")?;
        number(out, height)?;
        if threads == 1 {
            let planes = &mut self.planes[0];
            let mut defined = Palette::default();
            let mut colours = Colours::default();
            for y in (0..height).step_by(6) {
                let mut ends = [0; 256];
                for dy in 0..6.min(height - y) {
                    let row = (y + dy) * width * 4;
                    let pixels = image.pixels[row..row + width * 4].chunks_exact(4);
                    for (x, pixel) in pixels.enumerate() {
                        plot(planes, width, &mut ends, colours.of(pixel), x, dy);
                    }
                }
                write_band(planes, width, &ends, &mut defined, out)?;
                if y + 6 < height {
                    put(out, b"-")?;
                }
            }
        } else {
            if self.bands.len() < bands {
                self.bands.try_reserve(bands - self.bands.len()).map_err(|_| KittyError::Memory)?;
                self.bands.resize_with(bands, Band::default);
            }
            let bands = &mut self.bands[..bands];
            parallel::for_each_block(threads, bands, BANDS_PER_BLOCK, |first, block| {
                let mut colours = Colours::default();
                for (k, band) in block.iter_mut().enumerate() {
                    band.colour(image, (first + k) * 6, &mut colours);
                }
            });
            let mut defined = Palette::default();
            for band in bands.iter_mut() {
                band.defined = defined;
                for (all, used) in defined.iter_mut().zip(band.used) {
                    *all |= used;
                }
            }
            let planes = &mut self.planes[..threads];
            parallel::for_each_block_with(planes, bands, BANDS_PER_BLOCK, |planes, _, block| {
                for band in block {
                    band.write(planes, width);
                }
            });
            let last = bands.len() - 1;
            for (k, band) in bands.iter().enumerate() {
                if let Some(error) = band.failed {
                    return Err(error);
                }
                put(out, &band.bytes)?;
                if k < last {
                    put(out, b"-")?;
                }
            }
        }
        put(out, b"\x1b\\")?;
        Ok(out)
    }

    pub fn queue(&mut self, graphics: &mut KittyGraphics, image: &Image) -> Result<(), KittyError> {
        graphics.write_raw(self.encode(image)?)
    }

    /// [`Sixel::queue`] on up to `threads` threads.
    pub fn queue_in_parallel(
        &mut self,
        graphics: &mut KittyGraphics,
        threads: usize,
        image: &Image,
    ) -> Result<(), KittyError> {
        graphics.write_raw(self.encode_in_parallel(threads, image)?)
    }
}

impl Band {
    /// The palette index of every pixel of the band starting at row `y`.
    fn colour(&mut self, image: &Image, y: usize, colours: &mut Colours) {
        let width = image.width as usize;
        let rows = 6.min(image.height as usize - y);
        let pixels = &image.pixels[y * width * 4..(y + rows) * width * 4];
        self.used = Palette::default();
        self.indices.clear();
        self.failed = None;
        if self.indices.try_reserve_exact(width * rows).is_err() {
            self.failed = Some(KittyError::Memory);
            return;
        }
        for pixel in pixels.chunks_exact(4) {
            let c = colours.of(pixel);
            add(&mut self.used, c);
            self.indices.push(c as u8);
        }
    }

    /// The band's text, from its indices, in `planes`' rows.
    fn write(&mut self, planes: &mut [u8], width: usize) {
        self.bytes.clear();
        if self.failed.is_some() {
            return;
        }
        let mut ends = [0; 256];
        for (at, &c) in self.indices.iter().enumerate() {
            plot(planes, width, &mut ends, usize::from(c), at % width, at / width);
        }
        let mut defined = self.defined;
        if let Err(error) = write_band(planes, width, &ends, &mut defined, &mut self.bytes) {
            self.failed = Some(error);
        }
    }
}
