//! Portable Sixel output. Every frame paints its background as well as birds:
//! transparent Sixel pixels would otherwise leave the previous frame behind.
//! The 256-entry palette is stable across frames (cube + greys + background).

use crate::image::Image;
use crate::render::kitty::{KittyError, KittyGraphics};
use crate::sprites::PICTURE_GROUND;

#[derive(Debug, Default)]
pub struct Sixel {
    bytes: Vec<u8>,
    /// One row of six-pixel columns per palette colour. Each band clears the
    /// rows it used, so the planes are all zero between bands.
    planes: Vec<u8>,
    /// Whether an encode stopped partway through a band, leaving bits set.
    dirty: bool,
}

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
/// The length of the run of `row[0]` at the start of `row`. Birds are sparse,
/// so most runs are empty columns; those are skipped eight at a time.
fn run_length(row: &[u8]) -> usize {
    let value = row[0];
    let mut length = 1;
    if value == 0 {
        let words = row[1..].chunks_exact(8);
        for word in words {
            if u64::from_ne_bytes(word.try_into().unwrap()) != 0 {
                break;
            }
            length += 8;
        }
    }
    length + row[length..].iter().position(|&b| b != value).unwrap_or(row.len() - length)
}

impl Sixel {
    /// Encodes an RGBA image, flattening alpha onto the picture background.
    /// Colour definitions are sent in each image so it is self-contained.
    pub fn encode(&mut self, image: &Image) -> Result<&[u8], KittyError> {
        let Sixel { bytes: out, planes, dirty } = self;
        out.clear();
        let width = usize::try_from(image.width).map_err(|_| KittyError::Argument)?;
        let height = usize::try_from(image.height).map_err(|_| KittyError::Argument)?;
        if width == 0
            || height == 0
            || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(image.pixels.len())
        {
            return Err(KittyError::Argument);
        }
        let plane_size = width.checked_mul(256).ok_or(KittyError::Memory)?;
        planes
            .try_reserve(plane_size.saturating_sub(planes.len()))
            .map_err(|_| KittyError::Memory)?;
        if *dirty {
            planes.fill(0);
            *dirty = false;
        }
        planes.resize(plane_size, 0);
        // P2=1 leaves pixels outside the raster alone. Inside the raster,
        // every pixel (including background) is explicitly painted.
        put(out, b"\x1bP0;1q\"1;1;")?;
        number(out, width)?;
        put(out, b";")?;
        number(out, height)?;
        let mut defined = [false; 256];
        for y in (0..height).step_by(6) {
            let mut ends = [0; 256];
            *dirty = true;
            for dy in 0..6.min(height - y) {
                for x in 0..width {
                    let pixel = ((y + dy) * width + x) * 4;
                    let c = colour(&image.pixels[pixel..pixel + 4]);
                    planes[c * width + x] |= 1 << dy;
                    ends[c] = ends[c].max(x + 1);
                }
            }
            let mut first = true;
            for c in 0..256 {
                let end = ends[c];
                if end == 0 {
                    continue;
                }
                if !first {
                    put(out, b"$")?;
                }
                first = false;
                put(out, b"#")?;
                number(out, c)?;
                if !defined[c] {
                    put(out, b";2")?;
                    for component in palette(c) {
                        put(out, b";")?;
                        number(out, (usize::from(component) * 100 + 127) / 255)?;
                    }
                    defined[c] = true;
                }
                let row = &planes[c * width..c * width + end];
                let mut x = 0;
                while x < end {
                    let length = run_length(&row[x..]);
                    run(out, row[x] + b'?', length)?;
                    x += length;
                }
            }
            // A colour's bits all lie before its end, and only the colours in
            // this band have any: clearing those rows leaves the planes zero.
            for (c, &end) in ends.iter().enumerate() {
                if end != 0 {
                    planes[c * width..c * width + end].fill(0);
                }
            }
            *dirty = false;
            if y + 6 < height {
                put(out, b"-")?;
            }
        }
        put(out, b"\x1b\\")?;
        Ok(out)
    }
    pub fn queue(&mut self, graphics: &mut KittyGraphics, image: &Image) -> Result<(), KittyError> {
        graphics.write_raw(self.encode(image)?)
    }
}
