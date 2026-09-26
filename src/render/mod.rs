//! Terminal output: text cells, the Kitty graphics protocol, composition and
//! the panel. `cells` and `kitty` are translated from cbirds `cells.c` and
//! `kitty_graphics.c`; `compose` and `panel` from the drawing half of
//! `boids.c`.

#![forbid(unsafe_code)]

pub mod cells;
pub mod compose;
pub mod kitty;
pub mod panel;

use crate::image::Image;
use crate::sprites::empty_catalogue;

pub use panel::Stats;

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
        }
    }
}
