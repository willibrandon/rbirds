//! Lossless RGBA transport for composed Kitty frames. Flat sprite interiors
//! and transparent sky compress with pixel runs; no hash-chain search or PNG
//! filtering is needed. The output is a standard zlib/DEFLATE stream.

use super::kitty::KittyError;
use crate::image::Image;

#[derive(Debug, Default)]
pub(crate) struct PixelRuns {
    bytes: Vec<u8>,
    bits: u64,
    count: u32,
    positions: Vec<(usize, usize)>,
}

impl PixelRuns {
    pub(crate) fn reserve(&mut self, length: usize) -> Result<(), KittyError> {
        let capacity = length
            .checked_add(length.div_ceil(8))
            .and_then(|n| n.checked_add(16))
            .ok_or(KittyError::Memory)?;
        self.bytes
            .try_reserve(capacity.saturating_sub(self.bytes.len()))
            .map_err(|_| KittyError::Memory)
    }

    fn bits(&mut self, value: u32, count: u32) {
        self.bits |= u64::from(value) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.bytes.push(self.bits as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }

    fn symbol(&mut self, symbol: u32) {
        let (code, bits) = match symbol {
            0..=143 => (symbol + 0x30, 8),
            144..=255 => (symbol - 144 + 0x190, 9),
            256..=279 => (symbol - 256, 7),
            _ => (symbol - 280 + 0xc0, 8),
        };
        self.bits(code.reverse_bits() >> (32 - bits), bits);
    }

    fn run(&mut self, length: usize, distance: usize) {
        const BASE: [usize; 28] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227,
        ];
        const EXTRA: [u32; 28] =
            [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5];
        let symbol = BASE.partition_point(|&base| base <= length) - 1;
        self.symbol(257 + symbol as u32);
        self.bits((length - BASE[symbol]) as u32, EXTRA[symbol]);
        if distance <= 4 {
            self.bits(((distance - 1) as u32).reverse_bits() >> 27, 5);
        } else {
            let log = usize::BITS - 1 - (distance - 1).leading_zeros();
            let extra = log - 1;
            let code = 2 * log + (((distance - 1) >> extra) & 1) as u32;
            let base = 1 + ((2 + (code as usize & 1)) << extra);
            self.bits(code.reverse_bits() >> 27, 5);
            self.bits((distance - base) as u32, extra);
        }
    }

    pub(crate) fn encode_region(
        &mut self,
        image: &Image,
        left: usize,
        top: usize,
        width: usize,
        height: usize,
    ) -> Result<&[u8], KittyError> {
        let iw = usize::try_from(image.width).map_err(|_| KittyError::Argument)?;
        let ih = usize::try_from(image.height).map_err(|_| KittyError::Argument)?;
        if width == 0
            || height == 0
            || left > iw
            || top > ih
            || width > iw - left
            || height > ih - top
            || iw.checked_mul(ih).and_then(|n| n.checked_mul(4)) != Some(image.pixels.len())
        {
            return Err(KittyError::Argument);
        }
        let length = width * height * 4;
        // A literal takes at most nine bits. Runs are always shorter. Reserve
        // before encoding so no allocation can interrupt a partial stream.
        self.bytes.clear();
        self.reserve(length)?;
        self.bits = 0;
        self.count = 0;
        const SLOTS: usize = 16384;
        self.positions
            .try_reserve(SLOTS.saturating_sub(self.positions.len()))
            .map_err(|_| KittyError::Memory)?;
        self.positions.resize(SLOTS, (usize::MAX, 0));
        self.positions.fill((usize::MAX, 0));
        self.bytes.extend_from_slice(&[0x78, 0x01]);
        self.bits(3, 3); // Final block, fixed Huffman codes.
        let mut previous = None;
        let (mut a, mut b) = (1u32, 0u32);
        for y in top..top + height {
            let start = (y * iw + left) * 4;
            let row = &image.pixels[start..start + width * 4];
            let mut at = 0;
            while at < row.len() {
                let pixel: [u8; 4] = row[at..at + 4].try_into().unwrap();
                if previous == Some(pixel) {
                    let end = (at + 256).min(row.len());
                    let mut next = at + 4;
                    while next < end && row[next..next + 4] == pixel {
                        next += 4;
                    }
                    self.run(next - at, 4);
                    at = next;
                } else {
                    // One hash probe finds repeated sprite spans without a
                    // chain search. Store both canvas and cropped-stream
                    // offsets so distances exclude the skipped row margins.
                    if at + 8 <= row.len() {
                        let key = u64::from_le_bytes(row[at..at + 8].try_into().unwrap());
                        let slot = (key.wrapping_mul(0x9e3779b97f4a7c15) >> 50) as usize;
                        let logical = (y - top) * row.len() + at;
                        let (prior, source) = self.positions[slot];
                        self.positions[slot] = (logical, start + at);
                        if prior < logical && logical - prior <= 32768 {
                            let source_left = source % (iw * 4);
                            let limit =
                                256.min(row.len() - at).min((left + width) * 4 - source_left);
                            if limit >= 8 && image.pixels[source..source + 8] == row[at..at + 8] {
                                let mut length = 8;
                                while length < limit
                                    && image.pixels[source + length..source + length + 4]
                                        == row[at + length..at + length + 4]
                                {
                                    length += 4;
                                }
                                self.run(length, logical - prior);
                                previous =
                                    Some(row[at + length - 4..at + length].try_into().unwrap());
                                at += length;
                                continue;
                            }
                        }
                    }
                    for byte in pixel {
                        self.symbol(u32::from(byte));
                    }
                    previous = Some(pixel);
                    at += 4;
                }
            }
            // 5552 bytes bound both sums within u32 even for all-255 input.
            for chunk in row.chunks(5552) {
                for &byte in chunk {
                    a += u32::from(byte);
                    b += a;
                }
                a %= 65521;
                b %= 65521;
            }
        }
        self.symbol(256);
        if self.count > 0 {
            self.bits(0, 8 - self.count);
        }
        self.bytes.extend_from_slice(&((b << 16) | a).to_be_bytes());
        Ok(&self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::png;

    // The PNG reader's general inflater is independent of this encoder.
    fn decode(bytes: &[u8], length: usize) -> Vec<u8> {
        png::inflate_for_test(bytes, length).unwrap()
    }

    #[test]
    fn runs_literals_checksums_and_cropped_strides_decode_losslessly() {
        let mut encoder = PixelRuns::default();
        for pattern in 0..4 {
            let mut image = Image::alloc(2048, 2).unwrap();
            for (i, p) in image.pixels.chunks_exact_mut(4).enumerate() {
                let value = match pattern {
                    0 => 0,
                    1 => u32::MAX,
                    2 => (i as u32).wrapping_mul(0x9e3779b9),
                    _ => (i as u32 / 61).wrapping_mul(0x71e9148f),
                };
                p.copy_from_slice(&value.to_le_bytes());
            }
            for (left, top, width, height) in [
                (0, 0, 2048, 2),
                (1, 0, 73, 2),
                (2047, 1, 1, 1),
                (9, 1, 31, 1),
                (3, 0, 64, 1),
                (3, 0, 65, 1),
            ] {
                let expected: Vec<u8> = (top..top + height)
                    .flat_map(|y| {
                        let start = (y * 2048 + left) * 4;
                        image.pixels[start..start + width * 4].iter().copied()
                    })
                    .collect();
                let encoded = encoder.encode_region(&image, left, top, width, height).unwrap();
                assert_eq!(decode(encoded, expected.len()), expected);
            }
        }
    }

    #[test]
    fn repeated_spans_round_trip_across_distance_codes_and_window_boundary() {
        let mut encoder = PixelRuns::default();
        for pixels in [2, 3, 4, 5, 8, 9, 16, 31, 32, 63, 64, 65, 512, 2048, 8191, 8192, 8193] {
            let mut image = Image::alloc(pixels, 3).unwrap();
            for y in 0..3 {
                for x in 0..pixels {
                    let value = (x as u32).wrapping_mul(0x9e3779b9);
                    let at = image.offset(x, y);
                    image.pixels[at..at + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            let encoded = encoder.encode_region(&image, 0, 0, pixels as usize, 3).unwrap();
            assert_eq!(decode(encoded, image.pixels.len()), image.pixels, "width={pixels}");
        }
    }

    #[test]
    fn invalid_regions_and_storage_are_rejected() {
        let mut encoder = PixelRuns::default();
        let image = Image::alloc(4, 3).unwrap();
        for (left, top, width, height) in [
            (0, 0, 0, 1),
            (0, 0, 1, 0),
            (4, 0, 1, 1),
            (0, 3, 1, 1),
            (3, 0, 2, 1),
            (usize::MAX, 0, 1, 1),
        ] {
            assert_eq!(
                encoder.encode_region(&image, left, top, width, height),
                Err(KittyError::Argument)
            );
        }
        for image in [Image::empty(), Image { width: 4, height: 3, pixels: vec![0; 4] }] {
            assert_eq!(encoder.encode_region(&image, 0, 0, 1, 1), Err(KittyError::Argument));
        }
    }
}
