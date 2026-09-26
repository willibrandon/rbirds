//! The flock writes: a target per lit cell of the text, laid out in the space
//! the panel leaves, and a bird per target round robin. Translated from cbirds
//! `boids.c` (`formation`, `formation_layout`, `begin_the_intro`).

use super::Sim;
use crate::config::INTRO_SECONDS;
use crate::font::{self, FONT_ADVANCE, FONT_HEIGHT, FONT_WIDTH};
use crate::fp::mul_add;

pub const FORMATION_MAX_TARGETS: usize = 2048;

/// The C `formation` state. Targets live in fixed arrays as in the C, so a
/// layout never allocates.
#[derive(Clone, Debug)]
pub struct Formation {
    pub count: i32,
    pub x: Box<[f64; FORMATION_MAX_TARGETS]>,
    pub y: Box<[f64; FORMATION_MAX_TARGETS]>,
    /// Seconds on the clock at which to let go; negative is never.
    pub until: f64,
    pub writing: bool,
}

impl Default for Formation {
    fn default() -> Formation {
        Formation {
            count: 0,
            x: Box::new([0.0; FORMATION_MAX_TARGETS]),
            y: Box::new([0.0; FORMATION_MAX_TARGETS]),
            until: 0.0,
            writing: false,
        }
    }
}

impl Formation {
    /// `formation_clear`.
    pub fn clear(&mut self) {
        self.count = 0;
        self.writing = false;
    }

    /// `formation_target_of`: where bird `index` is headed while writing.
    #[inline]
    pub fn target_of(&self, index: i32) -> Option<(f64, f64)> {
        if !self.writing || self.count == 0 {
            return None;
        }
        let at = (index % self.count) as usize;
        Some((self.x[at], self.y[at]))
    }
}

impl Sim {
    /// `formation_layout`: lays the text out and returns how many targets it
    /// made, zero if it will not fit or has nothing to draw.
    pub fn formation_layout(&mut self, text: &[u8]) -> i32 {
        let pad = f64::from(self.config.bird_size) * 2.0;
        let left = if self.screen.legend_width > 0 {
            f64::from(self.screen.legend_width) + self.config.speed + pad
        } else {
            pad
        };
        let top = pad;
        let right = f64::from(self.screen.width) - pad;
        let bottom = f64::from(self.screen.height) - pad;
        let columns = font::text_width(text);

        self.formation.clear();
        if columns <= 0
            || right - left < f64::from(columns)
            || bottom - top < f64::from(FONT_HEIGHT)
        {
            return 0;
        }

        // As large as both dimensions allow, without stretching.
        let mut cell = (right - left) / f64::from(columns);
        let by_height = (bottom - top) / f64::from(FONT_HEIGHT);
        if by_height < cell {
            cell = by_height;
        }
        let width = f64::from(columns) * cell;
        let height = f64::from(FONT_HEIGHT) * cell;
        let origin_x = left + (right - left - width) / 2.0;
        let origin_y = top + (bottom - top - height) / 2.0;

        let mut column = 0;
        for &c in text {
            let Some(glyph) = font::glyph(c) else { continue };
            for row in 0..FONT_HEIGHT {
                for x in 0..FONT_WIDTH {
                    if glyph[(row * FONT_WIDTH + x) as usize] != b'#' {
                        continue;
                    }
                    if self.formation.count as usize >= FORMATION_MAX_TARGETS {
                        break;
                    }
                    // fma: boids.c:1210:38
                    let px = mul_add(f64::from(column + x) + 0.5, cell, origin_x);
                    // fma: boids.c:1211:38
                    let py = mul_add(f64::from(row) + 0.5, cell, origin_y);
                    if self.legend_turn_zone(px, py) {
                        continue;
                    }
                    let at = self.formation.count as usize;
                    self.formation.x[at] = px;
                    self.formation.y[at] = py;
                    self.formation.count += 1;
                }
            }
            column += FONT_ADVANCE;
        }
        self.formation.writing = self.formation.count > 0;
        self.formation.count
    }

    /// `begin_the_intro`: the flock writes its name for a moment.
    pub fn begin_the_intro(&mut self) {
        if self.formation_layout(b"BOIDS") != 0 {
            self.formation.until = f64::from(INTRO_SECONDS);
        }
    }
}
