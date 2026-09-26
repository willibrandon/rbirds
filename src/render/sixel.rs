//! Portable Sixel output. Every frame paints its background as well as birds:
//! transparent Sixel pixels would otherwise leave the previous frame behind.
//! The 256-entry palette is stable across frames (cube + greys + background).

use crate::image::Image;
use crate::render::kitty::{KittyError, KittyGraphics};
use crate::sprites::PICTURE_GROUND;

#[derive(Debug, Default)]
pub struct Sixel {
    bytes: Vec<u8>,
    planes: Vec<u8>,
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

impl Sixel {
    fn put(&mut self, bytes: &[u8]) -> Result<(), KittyError> {
        self.bytes.try_reserve(bytes.len()).map_err(|_| KittyError::Memory)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn number(&mut self, mut value: usize) -> Result<(), KittyError> {
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
        self.put(&digits[at..])
    }
    fn run(&mut self, value: u8, count: usize) -> Result<(), KittyError> {
        if count >= 4 {
            self.put(b"!")?;
            self.number(count)?;
            self.put(&[value])
        } else {
            for _ in 0..count {
                self.put(&[value])?;
            }
            Ok(())
        }
    }
    /// Encodes an RGBA image, flattening alpha onto the picture background.
    /// Colour definitions are sent in each image so it is self-contained.
    pub fn encode(&mut self, image: &Image) -> Result<&[u8], KittyError> {
        self.bytes.clear();
        let width = usize::try_from(image.width).map_err(|_| KittyError::Argument)?;
        let height = usize::try_from(image.height).map_err(|_| KittyError::Argument)?;
        if width == 0
            || height == 0
            || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(image.pixels.len())
        {
            return Err(KittyError::Argument);
        }
        let plane_size = width.checked_mul(256).ok_or(KittyError::Memory)?;
        self.planes
            .try_reserve(plane_size.saturating_sub(self.planes.len()))
            .map_err(|_| KittyError::Memory)?;
        self.planes.resize(plane_size, 0);
        // P2=1 leaves pixels outside the raster alone. Inside the raster,
        // every pixel (including background) is explicitly painted.
        self.put(b"\x1bP0;1q\"1;1;")?;
        self.number(width)?;
        self.put(b";")?;
        self.number(height)?;
        let mut defined = [false; 256];
        for y in (0..height).step_by(6) {
            self.planes.fill(0);
            let mut ends = [0; 256];
            for dy in 0..6.min(height - y) {
                for x in 0..width {
                    let pixel = ((y + dy) * width + x) * 4;
                    let c = colour(&image.pixels[pixel..pixel + 4]);
                    self.planes[c * width + x] |= 1 << dy;
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
                    self.put(b"$")?;
                }
                first = false;
                self.put(b"#")?;
                self.number(c)?;
                if !defined[c] {
                    self.put(b";2")?;
                    for component in palette(c) {
                        self.put(b";")?;
                        self.number((usize::from(component) * 100 + 127) / 255)?;
                    }
                    defined[c] = true;
                }
                let mut x = 0;
                while x < end {
                    let value = self.planes[c * width + x];
                    let mut next = x + 1;
                    while next < end && self.planes[c * width + next] == value {
                        next += 1;
                    }
                    self.run(value + b'?', next - x)?;
                    x = next;
                }
            }
            if y + 6 < height {
                self.put(b"-")?;
            }
        }
        self.put(b"\x1b\\")?;
        Ok(&self.bytes)
    }
    pub fn queue(&mut self, graphics: &mut KittyGraphics, image: &Image) -> Result<(), KittyError> {
        graphics.write_raw(self.encode(image)?)
    }
}
