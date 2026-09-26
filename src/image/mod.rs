//! RGBA images and the codecs that read and write them.
//!
//! Translated from the image helpers of cbirds `png.c` (`png_image_t`,
//! `png_image_alloc`, `png_status_string`); the PNG codec and transforms live
//! in [`png`] and the GIF writer in [`gif`].

#![forbid(unsafe_code)]

use std::fmt;

pub mod gif;
pub mod png;

/// Images over this many pixels a side are refused (`PNG_MAX_DIMENSION`).
pub const MAX_DIMENSION: i32 = 16384;
/// Images over this many pixels are refused: 256 MB once expanded to RGBA
/// (`PNG_MAX_PIXELS`).
pub const MAX_PIXELS: i64 = 1 << 26;

/// `png_status_t` without `PNG_OK`, which is `Ok(..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PngError {
    Memory,
    Argument,
    Truncated,
    Signature,
    Chunk,
    Crc,
    Unsupported,
    Deflate,
}

impl PngError {
    /// `png_status_string` for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            PngError::Memory => "out of memory",
            PngError::Argument => "invalid argument",
            PngError::Truncated => "truncated file",
            PngError::Signature => "not a PNG file",
            PngError::Chunk => "malformed chunk",
            PngError::Crc => "chunk checksum mismatch",
            PngError::Unsupported => "unsupported PNG variant",
            PngError::Deflate => "corrupted compressed data",
        }
    }
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for PngError {}

/// `png_status_string` over a whole status, `PNG_OK` included.
pub fn png_status_string(status: Result<(), PngError>) -> &'static str {
    match status {
        Ok(()) => "ok",
        Err(error) => error.as_str(),
    }
}

/// An 8 bit RGBA image with straight (not premultiplied) alpha, row major,
/// `width * height * 4` bytes (`png_image_t`).
///
/// The empty image (zero by zero, no pixels) stands for the C image whose
/// `pixels` pointer is NULL: never allocated, or freed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// The unallocated image (`{0, 0, NULL}`).
    pub const fn empty() -> Image {
        Image { width: 0, height: 0, pixels: Vec::new() }
    }

    /// Whether this is the unallocated image (`pixels == NULL`).
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// A fully transparent image (`png_image_alloc`), refused with the same
    /// statuses in the same order as the C: a non-positive side is an
    /// argument error, an oversized one unsupported, and an allocation that
    /// cannot be made is out of memory rather than an abort.
    pub fn alloc(width: i32, height: i32) -> Result<Image, PngError> {
        if width <= 0 || height <= 0 {
            return Err(PngError::Argument);
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(PngError::Unsupported);
        }
        if i64::from(width) * i64::from(height) > MAX_PIXELS {
            return Err(PngError::Unsupported);
        }
        // Bounded above by MAX_PIXELS * 4, so this cannot overflow.
        let length = width as usize * height as usize * 4;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(length).map_err(|_| PngError::Memory)?;
        pixels.resize(length, 0);
        Ok(Image { width, height, pixels })
    }

    /// Releases the pixels and returns to the empty image (`png_image_free`).
    pub fn free(&mut self) {
        *self = Image::empty();
    }

    /// Byte offset of pixel (x, y); the caller guarantees it is inside.
    #[inline]
    pub fn offset(&self, x: i32, y: i32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_refuses_in_the_c_order() {
        assert_eq!(Image::alloc(0, 1), Err(PngError::Argument));
        assert_eq!(Image::alloc(1, -1), Err(PngError::Argument));
        assert_eq!(Image::alloc(MAX_DIMENSION + 1, 1), Err(PngError::Unsupported));
        assert_eq!(Image::alloc(MAX_DIMENSION, MAX_DIMENSION), Err(PngError::Unsupported));
        let image = Image::alloc(3, 2).unwrap();
        assert_eq!((image.width, image.height, image.pixels.len()), (3, 2, 24));
        assert!(image.pixels.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn status_strings_are_the_c_ones() {
        assert_eq!(png_status_string(Ok(())), "ok");
        assert_eq!(png_status_string(Err(PngError::Deflate)), "corrupted compressed data");
    }
}
