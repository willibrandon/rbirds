//! Terminal output: text cells, the Kitty graphics protocol, composition and
//! the panel. `cells` and `kitty` are translated from cbirds `cells.c` and
//! `kitty_graphics.c`; `compose` and `panel` from the drawing half of
//! `boids.c`.

#![forbid(unsafe_code)]

pub mod atlas;
pub mod cells;
pub mod compose;
pub mod kitty;
pub mod panel;
pub mod sixel;

use crate::image::Image;
use crate::sprites::empty_catalogue;

pub use panel::{LegendBuffers, Stats};

/// The renderers' own state, formerly the C's `text_sprites`, `text_canvas`,
/// `text_cells`, `text_legend_was_drawn`, `legend_drawn` and `stats`.
#[derive(Debug)]
pub struct Renderer {
    /// The sprite catalogue as pixels, laid out as the image ids are.
    pub sprites: Vec<Image>,
    /// A canvas the size of the screen, for the text renderers.
    pub canvas: Image,
    /// The grid of cells diffed against the last frame.
    pub cells: cells::Cells,
    pub text_legend_was_drawn: bool,
    /// Whether the panel is currently on screen.
    pub legend_drawn: bool,
    pub stats: Stats,
    /// The panel's rows, rebuilt in place each frame.
    pub legend: LegendBuffers,
    /// Live text and Kitty sessions only send panel rows that changed.
    pub incremental_legend: bool,
    pub(crate) legend_cache: panel::LegendCache,
    pub sixel: sixel::Sixel,
    /// iTerm2 needs explicit image retirement before replacing a Sixel frame.
    pub erase_sixel_before_frame: bool,
    /// Crop only after the terminal confirms that the opaque backdrop glyph is one cell wide.
    pub crop_sixel_frames: bool,
    pub profile: Option<crate::timing::FrameProfile>,
    /// Text cells touched by this frame's sprite rectangles, including trails.
    pub occupied: Vec<bool>,
    pub atlas: Option<atlas::Atlas>,
    pub(crate) sprite_rows: Vec<compose::SpriteRows>,
}

impl Default for Renderer {
    fn default() -> Renderer {
        Renderer {
            sprites: empty_catalogue(),
            canvas: Image::empty(),
            cells: cells::Cells::default(),
            text_legend_was_drawn: false,
            legend_drawn: false,
            stats: Stats::default(),
            legend: LegendBuffers::default(),
            incremental_legend: false,
            legend_cache: panel::LegendCache::default(),
            sixel: sixel::Sixel::default(),
            erase_sixel_before_frame: false,
            crop_sixel_frames: false,
            profile: None,
            occupied: Vec::new(),
            atlas: None,
            sprite_rows: Vec::new(),
        }
    }
}
