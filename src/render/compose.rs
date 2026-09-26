//! Composition: the draw order every renderer shares, Kitty placements, and
//! the text renderers' frame read back from a composed canvas. Translated from
//! cbirds `boids.c` (`bird_placement`, `queue_render_frame`, `queue_text_frame`,
//! `blend_sprite`, `fill_ground`, `compose_onto`, `upload_sprite_sets`).

use super::Renderer;
use super::cells::{Cells, CellsStyle};
use super::kitty::{KittyError, KittyGraphics, Placement};
use crate::config::*;
use crate::image::{Image, png};
use crate::parallel;
use crate::simulation::{Bird, RenderMode, Sim, set_image_id};
use crate::sprites::PICTURE_GROUND;

/// `text_style`.
pub fn text_style(mode: RenderMode) -> CellsStyle {
    match mode {
        RenderMode::Sextants => CellsStyle::Sextants,
        RenderMode::Blocks => CellsStyle::Blocks,
        _ => CellsStyle::Braille,
    }
}

/// `terminal_has_truecolor`: COLORTERM is the convention.
pub fn terminal_has_truecolor() -> bool {
    match std::env::var_os("COLORTERM") {
        Some(value) => value == "truecolor" || value == "24bit",
        None => false,
    }
}

/// `bird_placement`: where a bird's sprite goes, or `None` off the screen.
pub fn bird_placement(sim: &Sim, bird: &Bird) -> Option<Placement> {
    if bird.x < 0.0 || bird.y < 0.0 {
        return None;
    }
    let screen = &sim.screen;
    let pixel_x = bird.x as i32;
    let pixel_y = bird.y as i32;
    let column = pixel_x / screen.cell_width;
    let row = pixel_y / screen.cell_height;
    if column >= screen.cols || row >= screen.rows {
        return None;
    }
    Some(Placement {
        image_id: sim.sprite_image_id(bird),
        placement_id: 0,
        row,
        column,
        x_offset: pixel_x % screen.cell_width,
        y_offset: pixel_y % screen.cell_height,
        // The far layer underneath.
        z_index: if bird.layer > 0 { -1 } else { 0 },
    })
}

/// Rows `top..top + rows` of a canvas `width` pixels wide: the whole canvas,
/// or the share of it one thread draws in big-flock mode.
struct Band<'a> {
    pixels: &'a mut [u8],
    width: i32,
    top: i32,
    rows: i32,
}

/// `blend_sprite`: `mix` blends edges, which a picture wants; without it the
/// more opaque pixel takes the place, which a text terminal wants.
pub fn blend_sprite(canvas: &mut Image, sprite: &Image, at_x: i32, at_y: i32, mix: bool) {
    let (width, rows) = (canvas.width, canvas.height);
    let mut band = Band { pixels: &mut canvas.pixels, width, top: 0, rows };
    blend_into(&mut band, sprite, at_x, at_y, mix);
}

/// `blend_sprite` for the part of the sprite that falls in `band`. The C
/// walks every pixel and skips those off the canvas; this walks only the ones
/// on it, a row at a time, which writes the same pixels.
fn blend_into(band: &mut Band, sprite: &Image, at_x: i32, at_y: i32, mix: bool) {
    let (at_x, at_y) = (i64::from(at_x), i64::from(at_y));
    let x0 = (-at_x).max(0);
    let x1 = (i64::from(band.width) - at_x).min(i64::from(sprite.width));
    let y0 = (i64::from(band.top) - at_y).max(0);
    let y1 = (i64::from(band.top) + i64::from(band.rows) - at_y).min(i64::from(sprite.height));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let sprite_row = sprite.width as usize * 4;
    let band_row = band.width as usize * 4;
    let length = (x1 - x0) as usize * 4;
    for y in y0..y1 {
        let from = y as usize * sprite_row + x0 as usize * 4;
        let to = (at_y + y - i64::from(band.top)) as usize * band_row + (at_x + x0) as usize * 4;
        let src = &sprite.pixels[from..from + length];
        let dst = &mut band.pixels[to..to + length];
        if mix {
            over(dst, src);
        } else {
            opaquer(dst, src);
        }
    }
}

/// The more opaque pixel takes the place; a transparent one never does.
/// Written as a choice between whole pixels, which the compiler can make
/// for several pixels at a time.
#[inline]
fn opaquer(dst: &mut [u8], src: &[u8]) {
    for (dst, src) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let from = u32::from_le_bytes([src[0], src[1], src[2], src[3]]);
        let to = u32::from_le_bytes([dst[0], dst[1], dst[2], dst[3]]);
        let kept = if from >> 24 > to >> 24 { from } else { to };
        dst.copy_from_slice(&kept.to_le_bytes());
    }
}

/// Straight alpha over: the colour underneath counts only for as much of it
/// as is there.
#[inline]
fn over(dst: &mut [u8], src: &[u8]) {
    for (dst, src) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let alpha = u32::from(src[3]);
        if alpha == 0 {
            continue;
        }
        // An opaque pixel covers: the formula gives the source exactly.
        if alpha == 255 {
            dst.copy_from_slice(src);
            continue;
        }
        // Over an opaque pixel, as all of a picture's ground is, `under` is
        // 255 - alpha and the result is opaque, so every division is by 255.
        if dst[3] == 255 {
            let under = 255 - alpha;
            for c in 0..3 {
                dst[c] = ((u32::from(src[c]) * alpha + u32::from(dst[c]) * under) / 255) as u8;
            }
            continue;
        }
        let under = u32::from(dst[3]) * (255 - alpha) / 255;
        let out_alpha = alpha + under;
        for c in 0..3 {
            dst[c] = ((u32::from(src[c]) * alpha + u32::from(dst[c]) * under) / out_alpha) as u8;
        }
        dst[3] = out_alpha as u8;
    }
}

/// `fill_ground`: opaque, so a picture looks like a terminal, not a cut out.
pub fn fill_ground(canvas: &mut Image) {
    fill_ground_into(&mut canvas.pixels);
}

fn fill_ground_into(pixels: &mut [u8]) {
    let pixel = [PICTURE_GROUND[0], PICTURE_GROUND[1], PICTURE_GROUND[2], 255];
    // Sixteen pixels at a time. A canvas is whole pixels, so what is left
    // over is too.
    let pattern: [u8; 64] = std::array::from_fn(|i| pixel[i % 4]);
    let mut chunks = pixels.chunks_exact_mut(pattern.len());
    for chunk in &mut chunks {
        chunk.copy_from_slice(&pattern);
    }
    let rest = chunks.into_remainder();
    rest.copy_from_slice(&pattern[..rest.len()]);
}

/// The birds a frame draws: `config.birds` of them, never past the array.
/// (The C reads past its array only when a key grows the flock and quits in
/// the same blocked write; memory safety takes precedence there.)
fn drawn<'a>(sim: &Sim, birds: &'a [Bird]) -> &'a [Bird] {
    &birds[..(sim.config.birds.max(0) as usize).min(birds.len())]
}

/// Bands a thread takes in turn: a few each, so a thread whose bands hold
/// the flock does not keep the others waiting.
const BANDS_PER_THREAD: usize = 3;

/// `compose_onto`: tails, then the flock far to near, then the hawks, on a
/// ground for a picture or on nothing for a text terminal. In big-flock mode
/// each thread draws the whole order into bands of rows of its own, so every
/// pixel is drawn over in the same order as on one thread.
pub fn compose_onto(
    sim: &Sim,
    canvas: &mut Image,
    frames: &[Image],
    birds: &[Bird],
    with_ground: bool,
) {
    let width = canvas.width;
    let row = width.max(0) as usize * 4;
    if row == 0 || canvas.pixels.is_empty() {
        return;
    }
    let rows = canvas.pixels.len() / row;
    let threads = sim.threads.max(1);
    let band_rows = rows.div_ceil(threads * BANDS_PER_THREAD);
    let band_rows = if threads > 1 { band_rows } else { rows };
    parallel::for_each_block(threads, &mut canvas.pixels, band_rows * row, |first, pixels| {
        let rows = (pixels.len() / row) as i32;
        let band = Band { pixels, width, top: (first / row) as i32, rows };
        compose_band(sim, band, frames, birds, with_ground);
    });
}

/// `compose_onto` for the rows of one band.
fn compose_band(sim: &Sim, mut band: Band, frames: &[Image], birds: &[Bird], with_ground: bool) {
    let shades = sim.palette_shades();
    if with_ground {
        fill_ground_into(band.pixels);
    } else {
        band.pixels.fill(0);
    }
    let birds = drawn(sim, birds);
    for layer in (0..LAYERS).rev() {
        if layer == 0 && sim.config.trails {
            for bird in birds.iter().step_by(TRAIL_EVERY as usize) {
                if bird.layer != 0 {
                    continue;
                }
                for step in 0..bird.trail_held {
                    let age = ((bird.trail_at - 1 - step + TRAIL_LENGTH) % TRAIL_LENGTH) as usize;
                    let sprite = &frames[(sim.trail_set(step) * ROTATION_FRAMES
                        + bird.frame % ROTATION_FRAMES)
                        as usize];
                    if !sprite.is_empty() {
                        blend_into(
                            &mut band,
                            sprite,
                            bird.trail_x[age] as i32,
                            bird.trail_y[age] as i32,
                            with_ground,
                        );
                    }
                }
            }
        }
        for bird in birds {
            if bird.layer != layer {
                continue;
            }
            let set = sim.flock_set(
                bird.shade % shades,
                WING_SEQUENCE[(bird.wing % WING_CYCLE) as usize],
                bird.layer,
            );
            let sprite = &frames[(set * ROTATION_FRAMES + bird.frame % ROTATION_FRAMES) as usize];
            if sprite.is_empty() {
                continue;
            }
            blend_into(&mut band, sprite, bird.x as i32, bird.y as i32, with_ground);
        }
    }
    let offset = sim.hawk_draw_offset();
    for hawk in &sim.hawks[..sim.config.hawks as usize] {
        let set = sim.hawk_set(WING_SEQUENCE[(hawk.wing % WING_CYCLE) as usize]);
        let sprite = &frames[(set * ROTATION_FRAMES + hawk.frame % ROTATION_FRAMES) as usize];
        if sprite.is_empty() {
            continue;
        }
        let (x, y) = (hawk.x as i32 - offset, hawk.y as i32 - offset);
        blend_into(&mut band, sprite, x, y, with_ground);
    }
}

/// `upload_sprite_sets`: every image of the catalogue encoded and sent, then
/// placements cleared, then flushed.
pub fn upload_sprite_sets(
    sim: &Sim,
    graphics: &mut KittyGraphics,
    frames: &[Image],
) -> Result<(), KittyError> {
    let images = (sim.sprite_set_count() * ROTATION_FRAMES) as usize;
    for (i, frame) in frames[..images].iter().enumerate() {
        if frame.is_empty() {
            continue;
        }
        let encoded = png::encode(frame).map_err(|_| KittyError::Memory)?;
        graphics.upload_png(i as u32 + 1, &encoded)?;
    }
    graphics.delete_all_placements()?;
    graphics.flush()
}

impl Renderer {
    /// `prepare_text_renderer`: the sprites as pixels and a fresh grid.
    pub fn prepare_text_renderer(
        &mut self,
        sim: &mut Sim,
        sprite_path: Option<&std::ffi::OsStr>,
        program: &[u8],
    ) -> Result<(), crate::sprites::SpriteError> {
        sim.rasterise_sprites(&mut self.sprites, sprite_path, program)?;
        self.cells = Cells::new(terminal_has_truecolor())
            .map_err(|_| crate::sprites::SpriteError::Png(crate::image::PngError::Memory))?;
        Ok(())
    }

    /// `text_renderer_fits_the_screen`: canvas and grid sized to the screen.
    pub fn text_renderer_fits_the_screen(&mut self, sim: &Sim) -> bool {
        if self.canvas.width != sim.screen.width || self.canvas.height != sim.screen.height {
            self.canvas.free();
            match Image::alloc(sim.screen.width, sim.screen.height) {
                Ok(canvas) => self.canvas = canvas,
                Err(_) => return false,
            }
        }
        self.cells.resize(sim.screen.cols, sim.screen.rows).is_ok()
    }

    /// `queue_render_frame`: one frame into the output buffer.
    pub fn queue_render_frame(
        &mut self,
        graphics: &mut KittyGraphics,
        sim: &Sim,
        birds: &[Bird],
    ) -> Result<(), KittyError> {
        if sim.render_mode == RenderMode::Sixel {
            return self.queue_sixel_frame(graphics, sim, birds);
        }
        if sim.drawing_with_text() {
            return self.queue_text_frame(graphics, sim, birds);
        }
        graphics.begin_synchronized_update()?;
        graphics.delete_all_placements()?;
        let birds = drawn(sim, birds);
        // Far birds first and underneath, then the tails, the near birds,
        // the hawks.
        for layer in (0..LAYERS).rev() {
            if layer == 0 && sim.config.trails {
                for bird in birds.iter().step_by(TRAIL_EVERY as usize) {
                    if bird.layer != 0 {
                        continue;
                    }
                    for step in 0..bird.trail_held {
                        // The newest ghost is the strongest.
                        let age =
                            ((bird.trail_at - 1 - step + TRAIL_LENGTH) % TRAIL_LENGTH) as usize;
                        let mut ghost = *bird;
                        ghost.x = bird.trail_x[age];
                        ghost.y = bird.trail_y[age];
                        if let Some(mut placement) = bird_placement(sim, &ghost) {
                            placement.image_id = set_image_id(sim.trail_set(step), ghost.frame);
                            graphics.place(&placement)?;
                        }
                    }
                }
            }
            for bird in birds {
                if bird.layer != layer {
                    continue;
                }
                if let Some(placement) = bird_placement(sim, bird) {
                    graphics.place(&placement)?;
                }
            }
        }
        let offset = f64::from(sim.hawk_draw_offset());
        for hawk in &sim.hawks[..sim.config.hawks as usize] {
            let as_bird = Bird {
                x: hawk.x - offset,
                y: hawk.y - offset,
                frame: hawk.frame,
                ..Bird::default()
            };
            if let Some(mut placement) = bird_placement(sim, &as_bird) {
                placement.image_id = sim.hawk_image_id(hawk);
                graphics.place(&placement)?;
            }
        }
        self.queue_legend(graphics, sim)?;
        graphics.end_synchronized_update()
    }

    /// Paint a complete Sixel raster, followed by the ordinary text panel.
    pub fn queue_sixel_frame(
        &mut self,
        graphics: &mut KittyGraphics,
        sim: &Sim,
        birds: &[Bird],
    ) -> Result<(), KittyError> {
        let resized =
            self.canvas.width != sim.screen.width || self.canvas.height != sim.screen.height;
        if resized {
            self.canvas = Image::alloc(sim.screen.width, sim.screen.height)
                .map_err(|_| KittyError::Memory)?;
        }
        graphics.begin_synchronized_update()?;
        if resized || self.legend_drawn && sim.screen.legend_width == 0 {
            graphics.write_raw(b"\x1b[2J")?;
            self.legend_drawn = false;
        }
        graphics.write_raw(b"\x1b[H")?;
        compose_onto(sim, &mut self.canvas, &self.sprites, birds, true);
        self.sixel.queue_in_parallel(graphics, sim.threads, &self.canvas)?;
        graphics.write_raw(b"\x1b[H")?;
        self.queue_legend(graphics, sim)?;
        graphics.end_synchronized_update()
    }

    /// `queue_text_frame`: the panel first, then the cells that changed.
    pub fn queue_text_frame(
        &mut self,
        graphics: &mut KittyGraphics,
        sim: &Sim,
        birds: &[Bird],
    ) -> Result<(), KittyError> {
        if !self.text_renderer_fits_the_screen(sim) {
            return Err(KittyError::Memory);
        }
        let mut status = graphics.begin_synchronized_update();
        if status.is_ok() {
            status = self.queue_legend(graphics, sim);
        }
        if self.legend_drawn != self.text_legend_was_drawn {
            self.cells.invalidate();
            self.text_legend_was_drawn = self.legend_drawn;
        }
        let (keep_cols, keep_rows) =
            if self.legend_drawn { (LEGEND_COLUMNS, sim.legend_rows()) } else { (0, 0) };
        self.cells.keep_out_of(keep_cols, keep_rows);

        compose_onto(sim, &mut self.canvas, &self.sprites, birds, false);
        self.cells.read_in_parallel(
            sim.threads,
            text_style(sim.render_mode),
            &self.canvas,
            sim.screen.cell_width,
            sim.screen.cell_height,
        );
        if self.cells.emit().is_err() {
            return Err(KittyError::Memory);
        }
        if status.is_ok() {
            status = graphics.write_raw(&self.cells.text);
        }
        if status.is_ok() {
            status = graphics.end_synchronized_update();
        }
        status
    }
}
