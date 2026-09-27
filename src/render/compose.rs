//! Composition: the draw order every renderer shares, Kitty placements, and
//! the text renderers' frame read back from a composed canvas. Translated from
//! cbirds `boids.c` (`bird_placement`, `queue_render_frame`, `queue_text_frame`,
//! `blend_sprite`, `fill_ground`, `compose_onto`, `upload_sprite_sets`).

use super::Renderer;
use super::cells::{Cells, CellsStyle};
use super::kitty::{KittyError, KittyGraphics, Placement};
use crate::config::*;
use crate::image::{Image, png};
use crate::simulation::{Bird, RenderMode, Sim, set_image_id};
use crate::sprites::PICTURE_GROUND;

/// Nontransparent row bounds, built once for the renderer's fixed sprites.
#[derive(Debug)]
pub(crate) struct SpriteRows {
    rows: Vec<(i32, i32)>,
    bounds: (i32, i32, i32, i32),
}

enum Coverage<'a> {
    Cells(&'a mut [bool]),
    Bounds(&'a mut (i32, i32, i32, i32)),
}

/// iTerm can rebuild its entire display list after every placement. Composing
/// the scene first bounds the number of terminal placements to two.
#[derive(Debug)]
pub struct KittyRaster {
    placements: Vec<(Placement, usize)>,
    canvases: [Image; 2],
    encoder: super::pixel_runs::PixelRuns,
    next_bank: u32,
    #[cfg(target_os = "macos")]
    pub(crate) shared: Option<[crate::platform::SharedImage; 4]>,
}

impl Default for KittyRaster {
    fn default() -> Self {
        Self {
            placements: Vec::new(),
            canvases: [Image::empty(), Image::empty()],
            encoder: super::pixel_runs::PixelRuns::default(),
            next_bank: 0,
            #[cfg(target_os = "macos")]
            shared: None,
        }
    }
}

impl KittyRaster {
    fn queue(
        &mut self,
        graphics: &mut KittyGraphics,
        sim: &Sim,
        birds: &[Bird],
        sprites: &[Image],
        sprite_rows: &[SpriteRows],
        profile: Option<&mut crate::timing::FrameProfile>,
    ) -> Result<(), KittyError> {
        self.placements.clear();
        let count = birds.len()
            + sim.config.hawks as usize
            + if sim.config.trails {
                birds.len().div_ceil(TRAIL_EVERY as usize) * TRAIL_LENGTH as usize
            } else {
                0
            };
        self.placements.try_reserve(count).map_err(|_| KittyError::Memory)?;
        for_each_placement(sim, birds, |placement| {
            self.placements.push((placement, self.placements.len()));
            Ok(())
        })?;
        // Kitty orders equal-z images by image ID, then placement order. Keep
        // the same ordering when rotations share a texture in the atlas path.
        self.placements.sort_unstable_by_key(|(p, order)| (p.z_index, p.image_id, *order));
        // Separate surfaces retain far birds below text and near birds above
        // it. With no panel there is no text to cross, so one surface suffices.
        let split = sim.screen.legend_width > 0;
        let planes = if split { 2 } else { 1 };
        let mut bounds = [(sim.screen.width, sim.screen.height, 0, 0); 2];
        for canvas in &mut self.canvases[..planes] {
            if canvas.width != sim.screen.width || canvas.height != sim.screen.height {
                *canvas = Image::alloc(sim.screen.width, sim.screen.height)
                    .map_err(|_| KittyError::Memory)?;
                self.encoder.reserve(canvas.pixels.len())?;
            }
            canvas.pixels.fill(0);
        }
        let (cw, ch) = (sim.screen.cell_width, sim.screen.cell_height);
        for (placement, _) in &self.placements {
            let index = placement.image_id as usize - 1;
            let sprite = &sprites[index];
            let plane = usize::from(split && placement.z_index >= 0);
            let canvas = &mut self.canvases[plane];
            let (x, y) = (
                placement.column * cw + placement.x_offset,
                placement.row * ch + placement.y_offset,
            );
            let rows = sprite_rows.get(index);
            let rect = rows.map_or((0, 0, sprite.width, sprite.height), |r| r.bounds);
            let left = (x + rect.0).max(0);
            let top = (y + rect.1).max(0);
            let right = (x + rect.2).min(canvas.width);
            let bottom = (y + rect.3).min(canvas.height);
            if left < right && top < bottom {
                let b = &mut bounds[plane];
                b.0 = b.0.min(left);
                b.1 = b.1.min(top);
                b.2 = b.2.max(right);
                b.3 = b.3.max(bottom);
                blend_rows::<true, false>(canvas, sprite, x, y, rows.map(|r| r.rows.as_slice()));
            }
        }
        if let Some(profile) = profile {
            profile.composed();
        }
        // Stage the next textures while the current placements remain visible.
        // Reusing a displayed image ID makes iTerm invalidate its old image
        // reference before the replacement reaches the display.
        let mut next = [None; 2];
        for (plane, &(left, top, right, bottom)) in bounds[..planes].iter().enumerate() {
            if left >= right || top >= bottom {
                continue;
            }
            let (x, y) = (left / cw, top / ch);
            let (cols, rows) = ((right - 1) / cw + 1 - x, (bottom - 1) / ch + 1 - y);
            let width = (cols * cw).min(sim.screen.width - x * cw);
            let height = (rows * ch).min(sim.screen.height - y * ch);
            let id = self.next_bank * 2 + plane as u32 + 1;
            #[cfg(target_os = "macos")]
            let shared = if let Some(images) = self.shared.as_mut() {
                let image = &mut images[id as usize - 1];
                let canvas = &self.canvases[plane];
                let stride = canvas.width as usize * 4;
                let offset = (y * ch) as usize * stride + (x * cw) as usize * 4;
                if image
                    .stage(&canvas.pixels, stride, offset, width as usize * 4, height as usize)
                    .unwrap_or(false)
                {
                    graphics.shared_rgba(id, width, height, image.name(), false)?;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            #[cfg(not(target_os = "macos"))]
            let shared = false;
            if !shared {
                let compressed = self.encoder.encode_region(
                    &self.canvases[plane],
                    (x * cw) as usize,
                    (y * ch) as usize,
                    width as usize,
                    height as usize,
                )?;
                graphics.upload_compressed_rgba(id, width, height, compressed)?;
            }
            next[plane] = Some((id, x, y, if split && plane == 0 { -1 } else { 1 }));
        }
        graphics.begin_synchronized_update()?;
        graphics.delete_all_placements()?;
        for (id, x, y, z) in next.into_iter().flatten() {
            graphics.place_raster(id, x, y, z)?;
        }
        self.next_bank ^= 1;
        Ok(())
    }
}

impl SpriteRows {
    fn new(sprite: &Image) -> Result<Self, KittyError> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(sprite.height.max(0) as usize).map_err(|_| KittyError::Memory)?;
        let mut bounds = (sprite.width, sprite.height, 0, 0);
        if sprite.width > 0 {
            for (y, row) in sprite.pixels.chunks_exact(sprite.width as usize * 4).enumerate() {
                let left = row.chunks_exact(4).position(|p| p[3] != 0);
                let right = row.chunks_exact(4).rposition(|p| p[3] != 0);
                let span = left.zip(right).map_or((0, 0), |(a, b)| (a as i32, b as i32 + 1));
                if span.0 < span.1 {
                    bounds.0 = bounds.0.min(span.0);
                    bounds.1 = bounds.1.min(y as i32);
                    bounds.2 = bounds.2.max(span.1);
                    bounds.3 = bounds.3.max(y as i32 + 1);
                }
                rows.push(span);
            }
        }
        Ok(Self { rows, bounds })
    }
}

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

/// `blend_sprite`: `mix` blends edges, which a picture wants; without it the
/// more opaque pixel takes the place, which a text terminal wants.
pub fn blend_sprite(canvas: &mut Image, sprite: &Image, at_x: i32, at_y: i32, mix: bool) {
    if mix {
        blend_rows::<true, false>(canvas, sprite, at_x, at_y, None);
    } else {
        blend_rows::<false, false>(canvas, sprite, at_x, at_y, None);
    }
}

fn blend_rows<const MIX: bool, const OPAQUE: bool>(
    canvas: &mut Image,
    sprite: &Image,
    at_x: i32,
    at_y: i32,
    rows: Option<&[(i32, i32)]>,
) {
    // Clip once, then walk contiguous rows. Most sprites have transparent
    // margins; neither those nor the clipped pixels need destination work.
    let x0 = (-i64::from(at_x)).max(0);
    let y0 = (-i64::from(at_y)).max(0);
    let x1 = i64::from(sprite.width).min(i64::from(canvas.width) - i64::from(at_x));
    let y1 = i64::from(sprite.height).min(i64::from(canvas.height) - i64::from(at_y));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for y in y0..y1 {
        let (left, right) = rows.map_or((x0, x1), |rows| {
            let (left, right) = rows[y as usize];
            (x0.max(i64::from(left)), x1.min(i64::from(right)))
        });
        if left >= right {
            continue;
        }
        let length = (right - left) as usize * 4;
        let from = sprite.offset(left as i32, y as i32);
        let to = canvas.offset((i64::from(at_x) + left) as i32, (i64::from(at_y) + y) as i32);
        let source = sprite.pixels[from..from + length].chunks_exact(4);
        let destination = canvas.pixels[to..to + length].chunks_exact_mut(4);
        for (src, dst) in source.zip(destination) {
            if !MIX {
                // Select the complete pixel so the compiler can vectorize the
                // maximum-alpha operation without conditional byte stores.
                let src = u32::from_le_bytes(src.try_into().unwrap());
                let old = u32::from_le_bytes(dst[..].try_into().unwrap());
                let pixel = if src >> 24 > old >> 24 { src } else { old };
                dst.copy_from_slice(&pixel.to_le_bytes());
                continue;
            }
            let alpha = u32::from(src[3]);
            if alpha == 0 {
                continue;
            }
            if alpha == 255 {
                dst.copy_from_slice(src);
                continue;
            }
            // Straight alpha over: the colour underneath counts only for as
            // much of it as is there.
            // A filled background stays opaque after every blend. The general
            // formula then has a constant divisor, with the same truncation.
            let under = if OPAQUE { 255 - alpha } else { u32::from(dst[3]) * (255 - alpha) / 255 };
            let out_alpha = if OPAQUE { 255 } else { alpha + under };
            for c in 0..3 {
                dst[c] =
                    ((u32::from(src[c]) * alpha + u32::from(dst[c]) * under) / out_alpha) as u8;
            }
            dst[3] = out_alpha as u8;
        }
    }
}

/// `fill_ground`: opaque, so a picture looks like a terminal, not a cut out.
pub fn fill_ground(canvas: &mut Image) {
    for pixel in canvas.pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[PICTURE_GROUND[0], PICTURE_GROUND[1], PICTURE_GROUND[2], 255]);
    }
}

/// The birds a frame draws: `config.birds` of them, never past the array.
/// (The C reads past its array only when a key grows the flock and quits in
/// the same blocked write; memory safety takes precedence there.)
fn drawn<'a>(sim: &Sim, birds: &'a [Bird]) -> &'a [Bird] {
    &birds[..(sim.config.birds.max(0) as usize).min(birds.len())]
}

/// `compose_onto`: tails, then the flock far to near, then the hawks, on a
/// ground for a picture or on nothing for a text terminal.
pub fn compose_onto(
    sim: &Sim,
    canvas: &mut Image,
    frames: &[Image],
    birds: &[Bird],
    with_ground: bool,
) {
    compose_with_coverage(sim, canvas, frames, birds, with_ground, None, &[]);
}

/// A sprite's rectangle is a conservative bound on the cells it can ink.
/// Skipping untouched cells avoids scanning the empty sky for every dot.
fn compose_with_coverage(
    sim: &Sim,
    canvas: &mut Image,
    frames: &[Image],
    birds: &[Bird],
    with_ground: bool,
    mut coverage: Option<Coverage<'_>>,
    sprite_rows: &[SpriteRows],
) {
    let mut blend = |canvas: &mut Image, index: usize, x: i32, y: i32| {
        let sprite = &frames[index];
        if let Some(coverage) = &mut coverage {
            let bounds = sprite_rows
                .get(index)
                .map_or((0, 0, sprite.width, sprite.height), |rows| rows.bounds);
            let left = (i64::from(x) + i64::from(bounds.0)).max(0);
            let top = (i64::from(y) + i64::from(bounds.1)).max(0);
            let right = (i64::from(x) + i64::from(bounds.2))
                .min(i64::from(canvas.width))
                .min(i64::from(sim.screen.cols) * i64::from(sim.screen.cell_width));
            let bottom = (i64::from(y) + i64::from(bounds.3))
                .min(i64::from(canvas.height))
                .min(i64::from(sim.screen.rows) * i64::from(sim.screen.cell_height));
            if right > left && bottom > top {
                match coverage {
                    Coverage::Cells(cells) => {
                        let columns = sim.screen.cols as usize;
                        let x0 = (left / i64::from(sim.screen.cell_width)) as usize;
                        let x1 = ((right - 1) / i64::from(sim.screen.cell_width)) as usize + 1;
                        let y0 = top / i64::from(sim.screen.cell_height);
                        let y1 = (bottom - 1) / i64::from(sim.screen.cell_height);
                        for row in y0..=y1 {
                            let offset = row as usize * columns;
                            cells[offset + x0..offset + x1].fill(true);
                        }
                    }
                    Coverage::Bounds(bounds) => {
                        bounds.0 = bounds.0.min(left as i32);
                        bounds.1 = bounds.1.min(top as i32);
                        bounds.2 = bounds.2.max(right as i32);
                        bounds.3 = bounds.3.max(bottom as i32);
                    }
                }
            }
        }
        let rows = sprite_rows.get(index).map(|rows| rows.rows.as_slice());
        if with_ground {
            blend_rows::<true, true>(canvas, sprite, x, y, rows);
        } else {
            blend_rows::<false, false>(canvas, sprite, x, y, rows);
        }
    };
    let shades = sim.palette_shades();
    if with_ground {
        fill_ground(canvas);
    } else {
        canvas.pixels.fill(0);
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
                    let index = (sim.trail_set(step) * ROTATION_FRAMES
                        + bird.frame % ROTATION_FRAMES) as usize;
                    let sprite = &frames[index];
                    if !sprite.is_empty() {
                        blend(canvas, index, bird.trail_x[age] as i32, bird.trail_y[age] as i32);
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
            let index = (set * ROTATION_FRAMES + bird.frame % ROTATION_FRAMES) as usize;
            let sprite = &frames[index];
            if sprite.is_empty() {
                continue;
            }
            blend(canvas, index, bird.x as i32, bird.y as i32);
        }
    }
    let offset = sim.hawk_draw_offset();
    for hawk in &sim.hawks[..sim.config.hawks as usize] {
        let set = sim.hawk_set(WING_SEQUENCE[(hawk.wing % WING_CYCLE) as usize]);
        let index = (set * ROTATION_FRAMES + hawk.frame % ROTATION_FRAMES) as usize;
        let sprite = &frames[index];
        if sprite.is_empty() {
            continue;
        }
        blend(canvas, index, hawk.x as i32 - offset, hawk.y as i32 - offset);
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

fn for_each_placement(
    sim: &Sim,
    birds: &[Bird],
    mut place: impl FnMut(Placement) -> Result<(), KittyError>,
) -> Result<(), KittyError> {
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
                    let age = ((bird.trail_at - 1 - step + TRAIL_LENGTH) % TRAIL_LENGTH) as usize;
                    let mut ghost = *bird;
                    ghost.x = bird.trail_x[age];
                    ghost.y = bird.trail_y[age];
                    if let Some(mut placement) = bird_placement(sim, &ghost) {
                        placement.image_id = set_image_id(sim.trail_set(step), ghost.frame);
                        place(placement)?;
                    }
                }
            }
        }
        for bird in birds {
            if bird.layer != layer {
                continue;
            }
            if let Some(placement) = bird_placement(sim, bird) {
                place(placement)?;
            }
        }
    }
    let offset = f64::from(sim.hawk_draw_offset());
    for hawk in &sim.hawks[..sim.config.hawks as usize] {
        let as_bird =
            Bird { x: hawk.x - offset, y: hawk.y - offset, frame: hawk.frame, ..Bird::default() };
        if let Some(mut placement) = bird_placement(sim, &as_bird) {
            placement.image_id = sim.hawk_image_id(hawk);
            place(placement)?;
        }
    }
    Ok(())
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
        self.sprite_rows.clear();
        self.sprite_rows
            .try_reserve(self.sprites.len())
            .map_err(|_| crate::sprites::SpriteError::Png(crate::image::PngError::Memory))?;
        for sprite in &self.sprites {
            self.sprite_rows.push(
                SpriteRows::new(sprite).map_err(|_| {
                    crate::sprites::SpriteError::Png(crate::image::PngError::Memory)
                })?,
            );
        }
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
        if self.cells.resize(sim.screen.cols, sim.screen.rows).is_err() {
            return false;
        }
        let count = self.cells.now.len();
        if self.occupied.try_reserve(count.saturating_sub(self.occupied.len())).is_err() {
            return false;
        }
        self.occupied.resize(count, false);
        true
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
        if let Some(raster) = &mut self.kitty_raster {
            raster.queue(
                graphics,
                sim,
                birds,
                &self.sprites,
                &self.sprite_rows,
                self.profile.as_mut(),
            )?;
            self.queue_legend(graphics, sim)?;
            return graphics.end_synchronized_update();
        }
        graphics.begin_synchronized_update()?;
        graphics.delete_all_placements()?;
        for_each_placement(sim, birds, |placement| {
            if let Some(atlas) = &self.atlas {
                atlas.place(graphics, &placement)
            } else {
                graphics.place(&placement)
            }
        })?;
        self.queue_legend(graphics, sim)?;
        graphics.end_synchronized_update()
    }

    /// Paint the Sixel frame, followed by the ordinary text panel.
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
        // A cropped image must end on cell boundaries: iTerm pads partial
        // cells with its default background. Keep the full raster when the
        // supplied pixel geometry cannot be represented by whole cells.
        let cropped = self.erase_sixel_before_frame
            && self.crop_sixel_frames
            && sim.screen.cols > 0
            && sim.screen.rows > 0
            && sim.screen.cell_width > 0
            && sim.screen.cell_height > 0
            && sim.screen.cols.checked_mul(sim.screen.cell_width) == Some(self.canvas.width)
            && sim.screen.rows.checked_mul(sim.screen.cell_height) == Some(self.canvas.height);
        graphics.begin_synchronized_update()?;
        // iTerm2 can release an overwritten image while its display still
        // references it, producing a full-screen brown placeholder. Retire it
        // explicitly within this synchronized update, before the replacement.
        if self.erase_sixel_before_frame
            || resized
            || self.legend_drawn && sim.screen.legend_width == 0
        {
            graphics.write_raw(b"\x1b[2J")?;
            self.legend_drawn = false;
        }
        graphics.write_raw(b"\x1b[H")?;
        let mut bounds = (self.canvas.width, self.canvas.height, 0, 0);
        compose_with_coverage(
            sim,
            &mut self.canvas,
            &self.sprites,
            birds,
            true,
            cropped.then_some(Coverage::Bounds(&mut bounds)),
            &self.sprite_rows,
        );
        if let Some(profile) = &mut self.profile {
            profile.composed();
        }
        if cropped {
            let (cw, ch) = (sim.screen.cell_width, sim.screen.cell_height);
            let crop = if bounds.2 > bounds.0 && bounds.3 > bounds.1 {
                (bounds.0 / cw, bounds.1 / ch, (bounds.2 - 1) / cw + 1, (bounds.3 - 1) / ch + 1)
            } else {
                (0, 0, 0, 0)
            };
            self.sixel.queue_ground(
                graphics,
                sim.screen.cols as usize,
                sim.screen.rows as usize,
            )?;
            graphics.write_raw(b"\x1b[0m")?;
            if crop.2 > crop.0 && crop.3 > crop.1 {
                graphics.write_text(crop.1, crop.0, b"")?;
                graphics.write_raw(self.sixel.encode_region(
                    &self.canvas,
                    (crop.0 * cw) as usize,
                    (crop.1 * ch) as usize,
                    ((crop.2 - crop.0) * cw) as usize,
                    ((crop.3 - crop.1) * ch) as usize,
                )?)?;
            }
        } else {
            self.sixel.queue(graphics, &self.canvas)?;
        }
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

        self.occupied.fill(false);
        compose_with_coverage(
            sim,
            &mut self.canvas,
            &self.sprites,
            birds,
            false,
            Some(Coverage::Cells(&mut self.occupied)),
            &self.sprite_rows,
        );
        if let Some(profile) = &mut self.profile {
            profile.composed();
        }
        self.cells.read_occupied(
            text_style(sim.render_mode),
            &self.canvas,
            sim.screen.cell_width,
            sim.screen.cell_height,
            &self.occupied,
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

#[cfg(test)]
mod row_tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn unread_shared_images_keep_their_frame_and_fall_back_to_identical_inline_output() {
        let mut sim = Sim::new();
        sim.render_mode = RenderMode::Kitty;
        sim.config.palette = 1;
        sim.config.birds = 2;
        sim.config.hawks = 0;
        sim.apply_screen_size(4, 4, 40, 40);
        sim.screen.legend_width = 1;
        let mut birds = [
            Bird { x: 8.0, y: 9.0, ..Bird::default() },
            Bird { x: 21.0, y: 21.0, layer: 1, ..Bird::default() },
        ];
        let far = sim.sprite_image_id(&birds[1]);
        let mut sprites = vec![Image::empty(); far as usize];
        for (id, rgba) in [(1, [255, 0, 0, 128]), (far, [0, 255, 0, 255])] {
            let mut sprite = Image::alloc(3, 3).unwrap();
            for pixel in sprite.pixels.chunks_exact_mut(4) {
                pixel.copy_from_slice(&rgba);
            }
            sprites[id as usize - 1] = sprite;
        }
        let rows: Vec<_> = sprites.iter().map(|s| SpriteRows::new(s).unwrap()).collect();
        let mut shared = KittyRaster {
            shared: Some([
                crate::platform::SharedImage::new().unwrap(),
                crate::platform::SharedImage::new().unwrap(),
                crate::platform::SharedImage::new().unwrap(),
                crate::platform::SharedImage::new().unwrap(),
            ]),
            ..KittyRaster::default()
        };
        let mut inline = KittyRaster::default();
        let mut a = KittyGraphics::new(1).unwrap();
        let mut b = KittyGraphics::new(1).unwrap();
        for frame in 0..6 {
            a.clear();
            b.clear();
            birds[0].x += 1.0;
            shared.queue(&mut a, &sim, &birds, &sprites, &rows, None).unwrap();
            inline.queue(&mut b, &sim, &birds, &sprites, &rows, None).unwrap();
            if frame < 2 {
                let output = String::from_utf8_lossy(a.buffer());
                assert_eq!(output.matches("t=s,").count(), 2);
                let swap = output.find("\x1b[?2026h").unwrap();
                assert!(output.rfind("a=t,").unwrap() < swap);
            } else {
                assert_eq!(a.buffer(), b.buffer(), "pending images must not be replaced");
            }
        }
    }

    #[test]
    fn kitty_surfaces_preserve_stacking_text_layers_clipping_and_empty_frames() {
        let mut sim = Sim::new();
        sim.render_mode = RenderMode::Kitty;
        sim.config.palette = 1;
        sim.config.birds = 5;
        sim.config.hawks = 0;
        sim.apply_screen_size(4, 4, 40, 40);
        sim.screen.legend_width = 1;
        let mut birds = [
            Bird { x: 8.0, y: 9.0, frame: 1, ..Bird::default() },
            Bird { x: 9.0, y: 10.0, ..Bird::default() },
            Bird { x: 8.0, y: 9.0, layer: 1, ..Bird::default() },
            Bird { x: 39.0, y: 39.0, ..Bird::default() },
            Bird { x: -1.0, y: 1.0, ..Bird::default() },
        ];
        let far_id = sim.sprite_image_id(&birds[2]);
        let mut sprites = vec![Image::empty(); far_id as usize];
        for (id, rgba) in [(1, [0, 0, 255, 128]), (2, [255, 0, 0, 255]), (far_id, [0, 255, 0, 255])]
        {
            let mut sprite = Image::alloc(3, 3).unwrap();
            for p in sprite.pixels.chunks_exact_mut(4) {
                p.copy_from_slice(&rgba);
            }
            sprites[id as usize - 1] = sprite;
        }
        let rows: Vec<_> = sprites.iter().map(|s| SpriteRows::new(s).unwrap()).collect();
        let mut raster = KittyRaster::default();
        let mut graphics = KittyGraphics::new(1).unwrap();
        raster.queue(&mut graphics, &sim, &birds, &sprites, &rows, None).unwrap();
        let pixel = |image: &Image, x, y| -> [u8; 4] {
            image.pixels[image.offset(x, y)..image.offset(x, y) + 4].try_into().unwrap()
        };
        assert_eq!(pixel(&raster.canvases[0], 9, 10), [0, 255, 0, 255]);
        // ID 2 covers ID 1 despite arriving first in the flock array.
        assert_eq!(pixel(&raster.canvases[1], 9, 10), [255, 0, 0, 255]);
        assert_eq!(pixel(&raster.canvases[1], 11, 12), [0, 0, 255, 128]);
        assert_eq!(pixel(&raster.canvases[1], 39, 39), [0, 0, 255, 128]);
        assert_eq!(pixel(&raster.canvases[1], 0, 1), [0; 4]);
        let protocol = String::from_utf8_lossy(graphics.buffer());
        assert!(protocol.contains("a=p,i=1,z=-1,C=1,q=2"));
        assert!(protocol.contains("a=p,i=2,z=1,C=1,q=2"));
        let swap = protocol.find("\x1b[?2026h").unwrap();
        assert!(protocol.rfind("a=t,").unwrap() < swap, "finish uploads before the swap");
        assert!(protocol.find("a=p,").unwrap() > swap);

        // Hiding the panel merges the layers and replaces the old placements.
        graphics.clear();
        sim.screen.legend_width = 0;
        raster.queue(&mut graphics, &sim, &birds, &sprites, &rows, None).unwrap();
        assert_eq!(pixel(&raster.canvases[0], 9, 10), [255, 0, 0, 255]);
        let protocol = String::from_utf8_lossy(graphics.buffer());
        assert_eq!(protocol.matches("a=p,").count(), 1);
        assert!(protocol.contains("a=p,i=3,z=1,"), "preserve the previous frame's images");

        // A resized viewport need not be an exact multiple of its cell size.
        graphics.clear();
        sim.screen.width = 25;
        sim.screen.height = 25;
        sim.screen.cell_width = 12;
        sim.screen.cell_height = 12;
        sim.screen.cols = 2;
        sim.screen.rows = 2;
        sim.config.birds = 1;
        birds[0].x = 23.0;
        birds[0].y = 23.0;
        raster.queue(&mut graphics, &sim, &birds, &sprites, &rows, None).unwrap();
        assert_eq!(pixel(&raster.canvases[0], 24, 24), [255, 0, 0, 255]);
        let protocol = String::from_utf8_lossy(graphics.buffer());
        assert!(protocol.contains("f=32,o=z,i=1,s=13,v=13,"));
        assert!(protocol.contains("\x1b[2;2H\x1b_Ga=p,i=1,z=1,C=1,q=2"));

        graphics.clear();
        sim.config.birds = 0;
        raster.queue(&mut graphics, &sim, &birds, &sprites, &rows, None).unwrap();
        assert!(raster.canvases[0].pixels.iter().all(|&b| b == 0));
        let mut deletion = KittyGraphics::new(1).unwrap();
        deletion.begin_synchronized_update().unwrap();
        deletion.delete_all_placements().unwrap();
        assert_eq!(graphics.buffer(), deletion.buffer());
    }

    #[test]
    fn opaque_rows_match_general_blending_for_every_alpha_and_overlapping_edges() {
        let mut sprite = Image::alloc(256, 3).unwrap();
        for (i, pixel) in sprite.pixels.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[(i * 17) as u8, (i * 31) as u8, (i * 43) as u8, i as u8]);
        }
        let rows = SpriteRows::new(&sprite).unwrap();
        let mut reference = Image::alloc(260, 5).unwrap();
        for (i, pixel) in reference.pixels.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[i as u8, (i * 7) as u8, (i * 11) as u8, 255]);
        }
        let mut opaque = reference.clone();
        for (x, y) in [(-257, -1), (-128, 0), (-1, 1), (0, 0), (1, 2), (128, 3), (255, 4)] {
            blend_sprite(&mut reference, &sprite, x, y, true);
            blend_rows::<true, true>(&mut opaque, &sprite, x, y, Some(&rows.rows));
            assert_eq!(opaque.pixels, reference.pixels, "{x}, {y}");
        }
    }

    #[test]
    fn cached_rows_preserve_blending_and_clipping_including_transparent_rgb() {
        let mut sprite = Image::alloc(23, 19).unwrap();
        for (i, pixel) in sprite.pixels.chunks_exact_mut(4).enumerate() {
            let x = i % 23;
            let y = i / 23;
            let alpha = if (3..20).contains(&x) && (2..17).contains(&y) {
                [0, 1, 64, 128, 254, 255][(x + y * 3) % 6]
            } else {
                0
            };
            pixel.copy_from_slice(&[(i * 13) as u8, (i * 7) as u8, i as u8, alpha]);
        }
        let rows = SpriteRows::new(&sprite).unwrap();
        assert_eq!(rows.bounds, (3, 2, 20, 17));
        for mix in [false, true] {
            for x in [i32::MIN, -24, -17, -1, 0, 11, 31, i32::MAX] {
                for y in [-20, -8, 0, 13, 28] {
                    let mut reference = Image::alloc(32, 29).unwrap();
                    for (i, pixel) in reference.pixels.chunks_exact_mut(4).enumerate() {
                        pixel.copy_from_slice(&[90, 45, 170, (i * 3) as u8]);
                    }
                    let mut cached = reference.clone();
                    blend_sprite(&mut reference, &sprite, x, y, mix);
                    if mix {
                        blend_rows::<true, false>(&mut cached, &sprite, x, y, Some(&rows.rows));
                    } else {
                        blend_rows::<false, false>(&mut cached, &sprite, x, y, Some(&rows.rows));
                    }
                    assert_eq!(cached.pixels, reference.pixels, "{x}, {y}, mix={mix}");
                }
            }
        }
        sprite.pixels.fill(0);
        let empty = SpriteRows::new(&sprite).unwrap();
        assert!(empty.rows.iter().all(|&span| span == (0, 0)));
        assert!(empty.bounds.0 >= empty.bounds.2);
    }
}
