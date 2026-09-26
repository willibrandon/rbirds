//! The sprite catalogue as pixels: the embedded bird, the drawn shapes or a
//! PNG of somebody's own, rotated through sixty headings, squashed for the wing
//! beats, tinted per shade, shrunk for the far sky and faded for the tails.
//! Translated from cbirds `boids.c` (shapes, `load_sprite`, `squash_wings`
//! through `rasterise_sprites`).

#![forbid(unsafe_code)]

use std::f64::consts::PI;
use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;

use crate::config::*;
use crate::fp::mul_add;
use crate::image::png::{self, TintMode};
use crate::image::{Image, PngError};
use crate::palette::Rgb;
use crate::simulation::Sim;

/// The embedded artwork, the same bytes as cbirds `matrix.png`/`sprite_png.h`.
pub static SPRITE_PNG: &[u8] = include_bytes!("../assets/sprite.png");

/// Four megabytes of PNG is a generous bird.
pub const SPRITE_FILE_MAX: usize = 1 << 22;

/// The number of images in a full catalogue: `ROTATION_FRAMES * MAX_SPRITE_SETS`.
pub const MAX_SPRITE_SETS: i32 =
    MAX_PALETTE_SHADES * (WING_PHASES + 1) + WING_PHASES + TRAIL_LENGTH;
pub const CATALOGUE_IMAGES: usize = (ROTATION_FRAMES * MAX_SPRITE_SETS) as usize;

/// The ground a composed picture is painted on (`PICTURE_GROUND`).
pub const PICTURE_GROUND: Rgb = [18, 18, 24];

/// `triangle_t`.
#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub x: [f64; 3],
    pub y: [f64; 3],
}

/// `shape_t`.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub name: &'static str,
    pub triangles: &'static [Triangle],
    /// Radius of a filled circle to union in, zero for none.
    pub roundness: f64,
}

const ARROW_TRIANGLES: [Triangle; 2] = [
    Triangle { x: [0.05, 0.95, 0.05], y: [0.15, 0.50, 0.85] },
    Triangle { x: [0.05, 0.45, 0.05], y: [0.35, 0.50, 0.65] },
];
const PLANE_TRIANGLES: [Triangle; 4] = [
    Triangle { x: [0.10, 0.95, 0.10], y: [0.44, 0.50, 0.56] },
    Triangle { x: [0.30, 0.55, 0.20], y: [0.48, 0.50, 0.08] },
    Triangle { x: [0.30, 0.55, 0.20], y: [0.52, 0.50, 0.92] },
    Triangle { x: [0.08, 0.22, 0.08], y: [0.30, 0.50, 0.70] },
];

pub const SHAPES: [Shape; 4] = [
    // The embedded drawing, not a shape at all.
    Shape { name: "bird", triangles: &[], roundness: 0.0 },
    Shape { name: "arrow", triangles: &ARROW_TRIANGLES, roundness: 0.0 },
    Shape { name: "plane", triangles: &PLANE_TRIANGLES, roundness: 0.0 },
    Shape { name: "dot", triangles: &[], roundness: 0.40 },
];
pub const SHAPE_NAMES: [&str; 4] = [SHAPES[0].name, SHAPES[1].name, SHAPES[2].name, SHAPES[3].name];

/// `inside_triangle`.
pub fn inside_triangle(t: &Triangle, x: f64, y: f64) -> bool {
    // fma: boids.c:984:53
    let d1 = mul_add(x - t.x[1], t.y[0] - t.y[1], -((t.x[0] - t.x[1]) * (y - t.y[1])));
    // fma: boids.c:985:53
    let d2 = mul_add(x - t.x[2], t.y[1] - t.y[2], -((t.x[1] - t.x[2]) * (y - t.y[2])));
    // fma: boids.c:986:53
    let d3 = mul_add(x - t.x[0], t.y[2] - t.y[0], -((t.x[2] - t.x[0]) * (y - t.y[0])));
    (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
}

/// `inside_shape`.
pub fn inside_shape(shape: &Shape, x: f64, y: f64) -> bool {
    if shape.roundness > 0.0 {
        let dx = x - 0.5;
        let dy = y - 0.5;
        // fma: boids.c:993:21
        if mul_add(dx, dx, dy * dy) <= shape.roundness * shape.roundness {
            return true;
        }
    }
    shape.triangles.iter().any(|t| inside_triangle(t, x, y))
}

/// `draw_shape`: four samples a pixel, white, alpha the coverage.
pub fn draw_shape(which: usize, size: i32) -> Result<Image, PngError> {
    let shape = &SHAPES[which];
    let mut out = Image::alloc(size, size)?;
    let side = f64::from(size);
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0;
            for sy in 0..2 {
                for sx in 0..2 {
                    // fma: boids.c:1012:60
                    let x = mul_add(f64::from(sx), 0.5, f64::from(px) + 0.25) / side;
                    // fma: boids.c:1013:53
                    let y = mul_add(f64::from(sy), 0.5, f64::from(py) + 0.25) / side;
                    hits += i32::from(inside_shape(shape, x, y));
                }
            }
            let at = out.offset(px, py);
            out.pixels[at] = 255;
            out.pixels[at + 1] = 255;
            out.pixels[at + 2] = 255;
            out.pixels[at + 3] = (hits * 255 / 4) as u8;
        }
    }
    Ok(out)
}

/// Why a sprite could not be had. `Fatal` is the C's message-and-exit(1)
/// inside `load_sprite`; the caller prints it and ends the run.
#[derive(Debug, PartialEq, Eq)]
pub enum SpriteError {
    Fatal(Vec<u8>),
    Png(PngError),
}

impl From<PngError> for SpriteError {
    fn from(error: PngError) -> SpriteError {
        SpriteError::Png(error)
    }
}

/// `load_sprite`: somebody's own PNG, a drawn shape, or the embedded bird.
/// `program` is the name messages are prefixed with, as invoked.
pub fn load_sprite(
    sprite_path: Option<&OsStr>,
    shape: i32,
    program: &[u8],
) -> Result<Image, SpriteError> {
    if let Some(path) = sprite_path {
        let message = |format: &[&[u8]]| -> SpriteError {
            let mut text = Vec::new();
            for piece in format {
                text.extend_from_slice(piece);
            }
            SpriteError::Fatal(text)
        };
        let name = path.as_bytes();
        let Ok(file) = std::fs::File::open(path) else {
            return Err(message(&[program, b": cannot open ", name, b"\n"]));
        };
        // One byte past the limit says whether the file is over it.
        let mut buffer = Vec::new();
        let read = file.take(SPRITE_FILE_MAX as u64 + 1).read_to_end(&mut buffer);
        let too_large = buffer.len() > SPRITE_FILE_MAX;
        let unreadable = read.is_err();
        if too_large {
            return Err(message(&[
                program,
                b": ",
                name,
                b" is over 4 MB, too large for a sprite\n",
            ]));
        }
        if unreadable {
            return Err(message(&[program, b": cannot read ", name, b"\n"]));
        }
        return Ok(png::decode(&buffer)?);
    }
    if shape != 0 {
        return Ok(draw_shape(shape as usize, SPRITE_WORK_MAX)?);
    }
    Ok(png::decode(SPRITE_PNG)?)
}

/// `squash_wings`: the span foreshortened across the axis of flight, set back
/// in the middle of its square.
pub fn squash_wings(square: &Image, span: f64) -> Result<Image, PngError> {
    // fma: boids.c:3130:46
    let mut height = mul_add(f64::from(square.height), span, 0.5) as i32;
    if height < 1 {
        height = 1;
    }
    let narrow = png::resize(square, square.width, height)?;
    let mut out = Image::alloc(square.width, square.height)?;
    let top = (square.height - height) / 2;
    let row = narrow.width as usize * 4;
    for y in 0..height as usize {
        let to = (top as usize + y) * out.width as usize * 4;
        out.pixels[to..to + row].copy_from_slice(&narrow.pixels[y * row..(y + 1) * row]);
    }
    Ok(out)
}

/// `fade_alpha`.
pub fn fade_alpha(image: &mut Image, factor: f64) {
    for alpha in image.pixels.iter_mut().skip(3).step_by(4) {
        // fma: boids.c:3148:80
        *alpha = mul_add(f64::from(*alpha), factor, 0.5) as u8;
    }
}

/// What tints one set of a geometry (`tint_fn` and its argument).
#[derive(Clone, Copy, Debug)]
enum Tint {
    Flock(i32),
    Far(i32),
    Hawk,
    Trail(i32),
}

impl Sim {
    /// `palette_tint`: shade zero of a tinted palette is still a tint;
    /// somebody's own artwork is left alone.
    pub fn palette_tint(&self, image: &mut Image, shade: i32) {
        if self.custom_sprite {
            return;
        }
        let palette = self.palette();
        let Some(tints) = self.palette_tints() else { return };
        let mut shade = shade;
        if shade < 0 {
            shade = 0;
        }
        if shade >= palette.shades {
            shade = palette.shades - 1;
        }
        let tint = tints[shade as usize];
        png::tint(image, tint[0], tint[1], tint[2], palette.mode);
    }

    /// `hawk_tint`.
    pub fn hawk_tint(&self, image: &mut Image) {
        let colour = self.hawk_colour();
        png::tint(image, colour[0], colour[1], colour[2], TintMode::Replace);
    }

    /// `far_tint`: the shade's own, pulled toward the ground; artwork of
    /// somebody's own is only dimmed.
    pub fn far_tint(&self, image: &mut Image, shade: i32) {
        let palette = self.palette();
        let tints = match self.palette_tints() {
            Some(tints) if !self.custom_sprite => tints,
            _ => {
                png::tint(image, 150, 150, 150, TintMode::Multiply);
                return;
            }
        };
        let mut shade = shade;
        if shade >= palette.shades {
            shade = palette.shades - 1;
        }
        let tint = tints[shade as usize];
        let mut rgb = [0_u8; 3];
        for c in 0..3 {
            let own = i32::from(tint[c]);
            let toward = i32::from(PICTURE_GROUND[c]) - own;
            // fma: boids.c:3162:52
            rgb[c] = (mul_add(f64::from(toward), FAR_DIM, f64::from(own)) + 0.5) as u8;
        }
        png::tint(image, rgb[0], rgb[1], rgb[2], palette.mode);
    }

    fn apply_tint(&self, image: &mut Image, tint: Tint) {
        match tint {
            Tint::Flock(shade) => self.palette_tint(image, shade),
            Tint::Far(shade) => self.far_tint(image, shade),
            Tint::Hawk => self.hawk_tint(image),
            Tint::Trail(step) => {
                self.palette_tint(image, self.palette_shades() / 2);
                fade_alpha(image, TRAIL_ALPHA[step as usize]);
            }
        }
    }

    /// `rasterise_geometry`: one geometry rotated once per frame, and every
    /// set sharing it copied and tinted from that.
    fn rasterise_geometry(
        &self,
        source: &Image,
        frames: &mut [Image],
        size: i32,
        span: f64,
        sets: &[(i32, Tint)],
    ) -> Result<(), PngError> {
        let mut work = size * SPRITE_SUPERSAMPLE;
        if work > SPRITE_WORK_MAX {
            work = SPRITE_WORK_MAX;
        }
        if work > source.width {
            work = source.width;
        }
        let mut square = png::resize(source, work, work)?;
        if span < 1.0 {
            square = squash_wings(&square, span)?;
        }
        for i in 0..ROTATION_FRAMES {
            let radians = f64::from(i * FRAME_ANGLE) * PI / 180.0;
            let base = png::rotate_resize(&square, radians, size, size)?;
            for &(set, tint) in sets {
                let mut frame = Image::alloc(base.width, base.height)?;
                frame.pixels.copy_from_slice(&base.pixels);
                self.apply_tint(&mut frame, tint);
                frames[(set * ROTATION_FRAMES + i) as usize] = frame;
            }
        }
        Ok(())
    }

    /// `rasterise_sprites`: every set of the catalogue, laid out exactly as
    /// the image ids are. `frames` holds [`CATALOGUE_IMAGES`] images.
    pub fn rasterise_sprites(
        &mut self,
        frames: &mut [Image],
        sprite_path: Option<&OsStr>,
        program: &[u8],
    ) -> Result<(), SpriteError> {
        self.settle_the_bird_size();
        let source = load_sprite(sprite_path, self.config.shape, program)?;
        let shades = self.palette_shades();

        // Near birds: one geometry a wing phase, every shade off each.
        for wing in 0..WING_PHASES {
            let sets: Vec<(i32, Tint)> = (0..shades)
                .map(|shade| (self.flock_set(shade, wing, 0), Tint::Flock(shade)))
                .collect();
            self.rasterise_geometry(
                &source,
                frames,
                self.config.bird_size,
                WING_SPAN[wing as usize],
                &sets,
            )?;
        }
        // Far birds: smaller, wings out, dimmed.
        // fma: boids.c:3238:58
        let mut far_size = mul_add(f64::from(self.config.bird_size), FAR_SIZE, 0.5) as i32;
        if far_size < MIN_BIRD_SIZE {
            far_size = MIN_BIRD_SIZE;
        }
        let sets: Vec<(i32, Tint)> =
            (0..shades).map(|shade| (self.flock_set(shade, 0, 1), Tint::Far(shade))).collect();
        self.rasterise_geometry(&source, frames, far_size, 1.0, &sets)?;
        // Hawks, at twice the size, at each wing phase.
        for wing in 0..WING_PHASES {
            self.rasterise_geometry(
                &source,
                frames,
                self.hawk_sprite_size(),
                WING_SPAN[wing as usize],
                &[(self.hawk_set(wing), Tint::Hawk)],
            )?;
        }
        // Tails: one geometry, a set a step of fading.
        // fma: boids.c:3255:62
        let mut trail_size = mul_add(f64::from(self.config.bird_size), TRAIL_SIZE, 0.5) as i32;
        if trail_size < MIN_BIRD_SIZE {
            trail_size = MIN_BIRD_SIZE;
        }
        let sets: Vec<(i32, Tint)> =
            (0..TRAIL_LENGTH).map(|step| (self.trail_set(step), Tint::Trail(step))).collect();
        self.rasterise_geometry(&source, frames, trail_size, 1.0, &sets)?;
        Ok(())
    }
}

/// An empty catalogue (`static png_image_t frames[...]`, all NULL).
pub fn empty_catalogue() -> Vec<Image> {
    vec![Image::empty(); CATALOGUE_IMAGES]
}

/// `free_sprites`.
pub fn free_sprites(frames: &mut [Image]) {
    for frame in frames {
        frame.free();
    }
}
