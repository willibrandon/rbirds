//! Portable Sixel output. Every frame paints its background as well as birds:
//! transparent Sixel pixels would otherwise leave the previous frame behind.
//! The 256-entry palette is stable across frames (cube + greys + background).

use crate::image::Image;
use crate::render::kitty::{KittyError, KittyGraphics};
use crate::sprites::PICTURE_GROUND;

const GROUND_BLOCK: [u8; 32] = {
    let mut bytes = [0; 32];
    let mut i = 0;
    while i < 8 {
        bytes[i * 4] = PICTURE_GROUND[0];
        bytes[i * 4 + 1] = PICTURE_GROUND[1];
        bytes[i * 4 + 2] = PICTURE_GROUND[2];
        bytes[i * 4 + 3] = 255;
        i += 1;
    }
    bytes
};

#[derive(Debug)]
pub struct Sixel {
    bytes: Vec<u8>,
    /// One row of six-pixel columns per palette colour. Each band clears the
    /// rows it used, so the planes are all zero between bands.
    planes: Vec<u8>,
    /// Whether an encode stopped partway through a band, leaving bits set.
    dirty: bool,
    /// Exact RGBA-to-palette memoization. Collisions replace an entry; they
    /// never approximate a colour. Flat sprites reuse very few colours.
    colours: [ColourEntry; 4096],
}

#[derive(Clone, Copy, Debug, Default)]
struct ColourEntry {
    rgba: u32,
    index: u8,
}

impl Default for Sixel {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            planes: Vec::new(),
            dirty: false,
            colours: [ColourEntry::default(); 4096],
        }
    }
}

fn cached_colour(cache: &mut [ColourEntry; 4096], pixel: &[u8]) -> usize {
    if pixel[3] == 0 || pixel[..3] == PICTURE_GROUND {
        return 0;
    }
    let rgba = u32::from_le_bytes(pixel.try_into().unwrap());
    let slot = (rgba.wrapping_mul(0x9e37_79b1) >> 20) as usize;
    let entry = &mut cache[slot];
    if entry.rgba != rgba {
        *entry = ColourEntry { rgba, index: colour(pixel) as u8 };
    }
    usize::from(entry.index)
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
    /// Opaque empty sky behind a whole-cell crop. Paint every cell: iTerm can
    /// retain stale regions when glyphs only surround a moving image. Background
    /// attributes alone become translucent; full blocks stay opaque.
    pub(crate) fn queue_ground(
        &mut self,
        graphics: &mut KittyGraphics,
        cols: usize,
        rows: usize,
    ) -> Result<(), KittyError> {
        self.bytes.clear();
        put(&mut self.bytes, b"\x1b[0;38;2")?;
        for component in PICTURE_GROUND {
            put(&mut self.bytes, b";")?;
            let percent = (usize::from(component) * 100 + 127) / 255;
            number(&mut self.bytes, (percent * 255 + 50) / 100)?;
        }
        put(&mut self.bytes, b"m")?;
        for row in 0..rows {
            put(&mut self.bytes, b"\x1b[")?;
            number(&mut self.bytes, row + 1)?;
            put(&mut self.bytes, ";1H█".as_bytes())?;
            if cols > 1 {
                put(&mut self.bytes, b"\x1b[")?;
                number(&mut self.bytes, cols - 1)?;
                put(&mut self.bytes, b"b")?;
            }
        }
        graphics.write_raw(&self.bytes)
    }

    /// Encodes an RGBA image, flattening alpha onto the picture background.
    /// Colour definitions are sent in each image so it is self-contained.
    pub fn encode(&mut self, image: &Image) -> Result<&[u8], KittyError> {
        self.encode_region(image, 0, 0, image.width.max(0) as usize, image.height.max(0) as usize)
    }

    /// Encode a rectangle directly from the image's existing row storage.
    pub fn encode_region(
        &mut self,
        image: &Image,
        left: usize,
        top: usize,
        width: usize,
        height: usize,
    ) -> Result<&[u8], KittyError> {
        let Sixel { bytes: out, planes, dirty, colours } = self;
        out.clear();
        let image_width = usize::try_from(image.width).map_err(|_| KittyError::Argument)?;
        let image_height = usize::try_from(image.height).map_err(|_| KittyError::Argument)?;
        if width == 0
            || height == 0
            || left > image_width
            || top > image_height
            || width > image_width - left
            || height > image_height - top
            || image_width.checked_mul(image_height).and_then(|n| n.checked_mul(4))
                != Some(image.pixels.len())
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
            let band_height = 6.min(height - y);
            ends[0] = width;
            *dirty = true;
            for dy in 0..band_height {
                let start = ((top + y + dy) * image_width + left) * 4;
                let row = &image.pixels[start..start + width * 4];
                // Paint a solid background before the foreground planes.
                // Empty sky needs neither palette lookups nor plane bits.
                for (block, pixels) in row.chunks(32).enumerate() {
                    let x = block * 8;
                    if pixels == GROUND_BLOCK {
                        continue;
                    } else {
                        for (offset, pixel) in pixels.chunks_exact(4).enumerate() {
                            let x = x + offset;
                            let c = cached_colour(colours, pixel);
                            if c == 0 {
                                continue;
                            }
                            planes[c * width + x] |= 1 << dy;
                            ends[c] = ends[c].max(x + 1);
                        }
                    }
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
                    // Some decoders (including WezTerm) only define the
                    // palette entry here. Select it explicitly before drawing.
                    put(out, b"#")?;
                    number(out, c)?;
                }
                if c == 0 {
                    run(out, ((1 << band_height) - 1) + b'?', width)?;
                    continue;
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
            for (c, &end) in ends.iter().enumerate().skip(1) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_cache_remains_exact_after_collisions_and_alpha_changes() {
        let mut cache = [ColourEntry::default(); 4096];
        let mut word = 1_u32;
        for _ in 0..100_000 {
            word = word.wrapping_mul(1664525).wrapping_add(1013904223);
            let pixel = word.to_le_bytes();
            assert_eq!(cached_colour(&mut cache, &pixel), colour(&pixel));
            assert_eq!(cached_colour(&mut cache, &pixel), colour(&pixel));
        }
        for alpha in 0..=255 {
            let pixel = [PICTURE_GROUND[0], PICTURE_GROUND[1], PICTURE_GROUND[2], alpha];
            assert_eq!(cached_colour(&mut cache, &pixel), colour(&pixel));
        }
    }
}
