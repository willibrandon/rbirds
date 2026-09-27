//! Kitty can place a pixel rectangle from an uploaded image. Keep the sixty
//! rotations of each sprite set in a few textures to reduce terminal lookups and
//! texture switches without changing the pixels or their stacking order.

use super::kitty::{KittyError, KittyGraphics, Placement};
use crate::config::ROTATION_FRAMES;
use crate::image::{Image, png};

#[derive(Clone, Copy, Debug, Default)]
pub struct Region {
    image: u32,
    x: i32,
    y: i32,
    width: i32,
    texture_width: i32,
    height: i32,
}

#[derive(Debug)]
pub struct Atlas {
    regions: Vec<Region>,
}

fn pack(rotations: &[Image], image: u32, regions: &mut [Region]) -> Result<Image, KittyError> {
    let width = rotations.iter().map(|frame| frame.width).max().unwrap_or(0);
    let height = rotations.iter().map(|frame| frame.height).max().unwrap_or(0);
    if width == 0 || height == 0 {
        return Ok(Image::empty());
    }
    let tile_height = height + 2;

    let mut atlas = Image::alloc(width, tile_height * rotations.len() as i32)
        .map_err(|_| KittyError::Memory)?;
    for (rotation, frame) in rotations.iter().enumerate() {
        if frame.is_empty() {
            continue;
        }
        let x = 0;
        let y = rotation as i32 * tile_height + 1;
        // Repeat the border pixels so filtering at a source rectangle
        // edge behaves like the original texture's clamp-to-edge.
        for dy in -1..=frame.height {
            for dx in 0..width {
                let from =
                    frame.offset(dx.clamp(0, frame.width - 1), dy.clamp(0, frame.height - 1));
                let to = atlas.offset(x + dx, y + dy);
                atlas.pixels[to..to + 4].copy_from_slice(&frame.pixels[from..from + 4]);
            }
        }
        regions[rotation] =
            Region { image, x, y, width: frame.width, texture_width: width, height: frame.height };
    }
    Ok(atlas)
}

impl Atlas {
    pub fn upload(graphics: &mut KittyGraphics, frames: &[Image]) -> Result<Self, KittyError> {
        let atlas = Self::queue_upload(graphics, frames)?;
        graphics.flush()?;
        Ok(atlas)
    }

    pub fn queue_upload(
        graphics: &mut KittyGraphics,
        frames: &[Image],
    ) -> Result<Self, KittyError> {
        let mut regions = Vec::new();
        regions.try_reserve_exact(frames.len()).map_err(|_| KittyError::Memory)?;
        regions.resize(frames.len(), Region::default());
        let mut image = 1;
        for (set, rotations) in frames.chunks(ROTATION_FRAMES as usize).enumerate() {
            let height = rotations.iter().map(|frame| frame.height).max().unwrap_or(0);
            // Bound each texture to 2048 pixels, including repeated borders.
            let per_texture = (2048 / (height + 2)).max(1) as usize;
            for (group, tiles) in rotations.chunks(per_texture).enumerate() {
                let offset = set * ROTATION_FRAMES as usize + group * per_texture;
                let atlas = pack(tiles, image, &mut regions[offset..offset + tiles.len()])?;
                if atlas.is_empty() {
                    continue;
                }
                let encoded = png::encode(&atlas).map_err(|_| KittyError::Memory)?;
                graphics.upload_png(image, &encoded)?;
                image += 1;
            }
        }
        graphics.delete_all_placements()?;
        Ok(Self { regions })
    }

    pub fn place(
        &self,
        graphics: &mut KittyGraphics,
        placement: &Placement,
    ) -> Result<(), KittyError> {
        let Some(region) = self
            .regions
            .get(placement.image_id.checked_sub(1).ok_or(KittyError::Argument)? as usize)
        else {
            return Err(KittyError::Argument);
        };
        let mut mapped = *placement;
        mapped.image_id = region.image;
        // The protocol orders equal-z images by their image id. Preserve that
        // original upload order explicitly now that rotations share an image.
        mapped.z_index = placement.image_id as i32;
        if placement.z_index < 0 {
            mapped.z_index -= self.regions.len() as i32 + 1;
        }
        let width = if region.width == region.texture_width { 0 } else { region.width };
        graphics.place_region(&mapped, [region.x, region.y, width, region.height])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_preserves_every_source_pixel_and_clamped_edge() {
        let mut frames = Vec::new();
        // Different sizes and nonzero edge colors expose crop/stride/filter
        // mistakes that square, transparent-bordered bird sprites can hide.
        for rotation in 0..60 {
            let mut frame = Image::alloc(2 + rotation % 7, 3 + rotation % 5).unwrap();
            for (i, pixel) in frame.pixels.chunks_exact_mut(4).enumerate() {
                pixel.copy_from_slice(&[rotation as u8, i as u8, (i * 13) as u8, (i * 17) as u8]);
            }
            frames.push(frame);
        }
        let mut regions = vec![Region::default(); frames.len()];
        let packed = pack(&frames, 7, &mut regions).unwrap();
        // Check the actual encoded/decoded texture, including the last row.
        let encoded = png::encode(&packed).unwrap();
        let decoded = png::decode(&encoded).unwrap();
        for (frame, region) in frames.iter().zip(regions) {
            assert_eq!(region.image, 7);
            assert_eq!((region.width, region.height), (frame.width, frame.height));
            for y in -1..=frame.height {
                for x in 0..region.texture_width {
                    let original =
                        frame.offset(x.clamp(0, frame.width - 1), y.clamp(0, frame.height - 1));
                    let crop = decoded.offset(region.x + x, region.y + y);
                    assert_eq!(
                        &frame.pixels[original..original + 4],
                        &decoded.pixels[crop..crop + 4]
                    );
                }
            }
        }
    }

    #[test]
    fn cropped_placements_keep_position_dimensions_and_stack_order() {
        let regions = (0..120)
            .map(|i| Region {
                image: i / 60 + 1,
                x: 33,
                y: 65,
                width: 30,
                texture_width: 99,
                height: 30,
            })
            .collect();
        let atlas = Atlas { regions };
        let mut graphics = KittyGraphics::new(1).unwrap();
        for (image_id, layer, expected_z) in
            [(1, -1, -120), (60, -1, -61), (61, -1, -60), (1, 0, 1), (120, 0, 120)]
        {
            graphics.clear();
            let placement = Placement {
                image_id,
                row: 2,
                column: 3,
                x_offset: 4,
                y_offset: 5,
                z_index: layer,
                ..Placement::default()
            };
            atlas.place(&mut graphics, &placement).unwrap();
            assert_eq!(graphics.buffer(), format!("\x1b[3;4H\x1b_Ga=p,I={},q=2,X=4,Y=5,z={expected_z},C=1,x=33,y=65,w=30,h=30\x1b\\", (image_id - 1) / 60 + 1).as_bytes());
        }
        assert_eq!(atlas.place(&mut graphics, &Placement::default()), Err(KittyError::Argument));
    }
}
