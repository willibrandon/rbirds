//! The parameter panel in the top left corner: one slider a parameter, the
//! frame's cost, and how to quit. Translated from cbirds `boids.c`
//! (`legend_slider`, `legend_number`, `build_legend`, `queue_legend`).
//!
//! Rows are bytes padded as `snprintf` pads them — by bytes — and cut as
//! `snprintf` cuts them at the C buffer sizes, so a row is the reference's
//! row even when a statistic outgrows its column.

use std::f64::consts::PI;

use super::Renderer;
use super::kitty::{KittyError, KittyGraphics};
use crate::cfmt;
use crate::config::*;
use crate::simulation::Sim;

/// What the panel's stats row reports, averaged over the last second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub frame_ms: f64,
    pub bytes: f64,
    pub rate: f64,
    pub counted: i64,
    pub window_started: f64,
    pub window_ms: f64,
    pub window_bytes: f64,
}

/// `snprintf(out, size, ...)`: at most `size - 1` bytes survive.
fn truncated(mut text: Vec<u8>, size: usize) -> Vec<u8> {
    text.truncate(size.saturating_sub(1));
    text
}

fn pad_right(out: &mut Vec<u8>, text: &[u8], width: usize) {
    out.extend_from_slice(text);
    out.resize(out.len() + width.saturating_sub(text.len()), b' ');
}

fn pad_left(out: &mut Vec<u8>, text: &[u8], width: usize) {
    out.resize(out.len() + width.saturating_sub(text.len()), b' ');
    out.extend_from_slice(text);
}

/// `legend_slider`: name, bar, value right aligned by cells, the two keys.
pub fn legend_slider(name: &str, notch: i32, value: &[u8], lower: u8, raise: u8) -> Vec<u8> {
    let mut bar = Vec::with_capacity(LEGEND_BAR_CELLS as usize * 3);
    for cell in 0..LEGEND_BAR_CELLS {
        bar.extend_from_slice(if cell < notch { "\u{2593}" } else { "\u{2591}" }.as_bytes());
    }
    // Padded by cells rather than bytes: a degree sign is two bytes, one cell.
    let bytes = value.len();
    let glyphs = value.iter().filter(|&&c| (c & 0xc0) != 0x80).count();
    let column = (LEGEND_VALUE_WIDTH + (bytes - glyphs) as i32) as usize;
    let mut line = Vec::with_capacity(LEGEND_LINE_MAX);
    line.extend_from_slice("\u{2502} ".as_bytes());
    pad_right(&mut line, name.as_bytes(), LEGEND_NAME_WIDTH as usize);
    line.push(b' ');
    line.extend_from_slice(&bar);
    line.push(b' ');
    pad_left(&mut line, value, column);
    line.extend_from_slice(b"  ");
    line.push(lower);
    line.push(b'/');
    line.push(raise);
    line.extend_from_slice(" \u{2502}".as_bytes());
    truncated(line, LEGEND_LINE_MAX)
}

/// `legend_number`: `%.*f` into a buffer of `LEGEND_VALUE_WIDTH + 8`.
fn legend_number(value: f64, decimals: usize) -> Vec<u8> {
    truncated(cfmt::fixed(value, decimals).into_bytes(), LEGEND_VALUE_WIDTH as usize + 8)
}

fn border(left: &str, right: &str) -> Vec<u8> {
    let mut line = Vec::with_capacity(LEGEND_LINE_MAX);
    line.extend_from_slice(left.as_bytes());
    for _ in 0..LEGEND_COLUMNS - 2 {
        line.extend_from_slice("\u{2500}".as_bytes());
    }
    line.extend_from_slice(right.as_bytes());
    line
}

/// `build_legend`: the panel's rows, `legend_rows()` of them.
pub fn build_legend(sim: &Sim, stats: &Stats) -> Vec<Vec<u8>> {
    let config = &sim.config;
    let rows = sim.legend_rows() as usize;
    let inner = (LEGEND_COLUMNS - 2) as usize;
    let value_size = LEGEND_VALUE_WIDTH as usize + 8;
    let mut lines = vec![Vec::new(); LEGEND_MAX_ROWS as usize];

    lines[0] = border("\u{256d}", "\u{256e}");
    lines[1] = legend_slider(
        "boundary",
        config.boundary_notch,
        &legend_number(config.boundary, 2),
        b'b',
        b'B',
    );
    lines[2] = legend_slider(
        "separation",
        config.separation_notch,
        &legend_number(config.separation, 3),
        b's',
        b'S',
    );
    lines[3] = legend_slider(
        "alignment",
        config.alignment_notch,
        &legend_number(config.alignment, 2),
        b'a',
        b'A',
    );
    let degrees = format!("{}\u{b0}", cfmt::fixed(sim.turning_notch_radians() * 180.0 / PI, 0));
    lines[4] = legend_slider(
        "turning",
        config.turning_notch,
        &truncated(degrees.into_bytes(), value_size),
        b't',
        b'T',
    );
    let pixels = format!("{}px", config.vision_radius);
    lines[5] = legend_slider(
        "perception",
        config.vision_notch,
        &truncated(pixels.into_bytes(), value_size),
        b'p',
        b'P',
    );
    let pace = format!("{}\u{d7}", cfmt::fixed(config.pace, 1));
    lines[6] = legend_slider(
        "speed",
        config.pace_notch,
        &truncated(pace.into_bytes(), value_size),
        b'v',
        b'V',
    );
    if config.flocks > 1 {
        let avoid = format!("{}\u{d7}", cfmt::fixed(f64::from(config.avoid_notch) / 4.0, 2));
        lines[7] = legend_slider(
            "avoidance",
            config.avoid_notch,
            &truncated(avoid.into_bytes(), value_size),
            b'g',
            b'G',
        );
    }

    let mut measured = Vec::new();
    pad_right(&mut measured, b"frame", LEGEND_NAME_WIDTH as usize);
    measured.push(b' ');
    measured.extend_from_slice(cfmt::fixed_width(stats.frame_ms, 5, 1).as_bytes());
    measured.extend_from_slice(b"ms ");
    measured.extend_from_slice(cfmt::fixed_width(stats.bytes / 1024.0, 5, 0).as_bytes());
    measured.extend_from_slice(b"KB ");
    measured.extend_from_slice(cfmt::fixed_width(stats.rate, 3, 0).as_bytes());
    measured.extend_from_slice(b"fps");
    let measured = truncated(measured, LEGEND_LINE_MAX / 2);
    let mut row = Vec::new();
    row.extend_from_slice("\u{2502} ".as_bytes());
    pad_right(&mut row, &measured, inner - 2);
    row.extend_from_slice(" \u{2502}".as_bytes());
    lines[rows - 3] = truncated(row, LEGEND_LINE_MAX);

    let mut quit = Vec::new();
    quit.extend_from_slice("\u{2502} ".as_bytes());
    pad_right(&mut quit, b"quit", LEGEND_NAME_WIDTH as usize);
    quit.extend_from_slice(b" q");
    pad_left(&mut quit, b"", inner - LEGEND_NAME_WIDTH as usize - 4);
    quit.extend_from_slice(" \u{2502}".as_bytes());
    lines[rows - 2] = truncated(quit, LEGEND_LINE_MAX);

    lines[rows - 1] = border("\u{2570}", "\u{256f}");
    lines.truncate(rows);
    lines
}

impl Renderer {
    /// `queue_legend`: the panel's rows, or, once, the erase of the rows it
    /// held when a viewport shrank under it — never a screen erase, which
    /// would take the uploaded sprites with it.
    pub fn queue_legend(
        &mut self,
        graphics: &mut KittyGraphics,
        sim: &Sim,
    ) -> Result<(), KittyError> {
        if sim.screen.legend_width == 0 {
            if !self.legend_drawn {
                return Ok(());
            }
            for row in 0..sim.legend_rows() {
                graphics.write_text(row, 0, b"\x1b[K")?;
            }
            self.legend_drawn = false;
            return Ok(());
        }
        let lines = build_legend(sim, &self.stats);
        for (row, line) in lines.iter().enumerate() {
            graphics.write_text(row as i32, 0, line)?;
        }
        self.legend_drawn = true;
        Ok(())
    }
}
