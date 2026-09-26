//! The parameter panel in the top left corner: one slider a parameter, the
//! frame's cost, and how to quit. Translated from cbirds `boids.c`
//! (`legend_slider`, `legend_number`, `build_legend`, `queue_legend`).
//!
//! Rows are bytes, padded as `snprintf` pads them (by bytes) and cut as
//! `snprintf` cuts them at the C buffer sizes, so a row is the reference's
//! row even when a statistic outgrows its column. They are built into
//! buffers the renderer keeps, as the C builds them on its stack, so a frame
//! with the panel up allocates nothing.

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

/// The panel's rows and the scratch they are built with: the C's
/// `lines[LEGEND_MAX_ROWS][LEGEND_LINE_MAX]`, `value[]` and `measured[]`.
#[derive(Clone, Debug)]
pub struct LegendBuffers {
    pub lines: [Vec<u8>; LEGEND_MAX_ROWS as usize],
    value: Vec<u8>,
    measured: Vec<u8>,
    bar: Vec<u8>,
}

impl Default for LegendBuffers {
    fn default() -> LegendBuffers {
        LegendBuffers {
            lines: std::array::from_fn(|_| Vec::with_capacity(LEGEND_LINE_MAX)),
            value: Vec::with_capacity(LEGEND_VALUE_WIDTH as usize + 8),
            measured: Vec::with_capacity(LEGEND_LINE_MAX / 2),
            bar: Vec::with_capacity(LEGEND_BAR_CELLS as usize * 3),
        }
    }
}

/// `snprintf(out, size, ...)`: at most `size - 1` bytes survive.
fn cut(text: &mut Vec<u8>, size: usize) {
    text.truncate(size.saturating_sub(1));
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
fn legend_slider(
    line: &mut Vec<u8>,
    bar: &mut Vec<u8>,
    name: &str,
    notch: i32,
    value: &[u8],
    lower: u8,
    raise: u8,
) {
    bar.clear();
    for cell in 0..LEGEND_BAR_CELLS {
        bar.extend_from_slice(if cell < notch { "\u{2593}" } else { "\u{2591}" }.as_bytes());
    }
    // Padded by cells rather than bytes: a degree sign is two bytes, one cell.
    let bytes = value.len();
    let glyphs = value.iter().filter(|&&c| (c & 0xc0) != 0x80).count();
    let column = (LEGEND_VALUE_WIDTH + (bytes - glyphs) as i32) as usize;
    line.clear();
    line.extend_from_slice("\u{2502} ".as_bytes());
    pad_right(line, name.as_bytes(), LEGEND_NAME_WIDTH as usize);
    line.push(b' ');
    line.extend_from_slice(bar);
    line.push(b' ');
    pad_left(line, value, column);
    line.extend_from_slice(b"  ");
    line.push(lower);
    line.push(b'/');
    line.push(raise);
    line.extend_from_slice(" \u{2502}".as_bytes());
    cut(line, LEGEND_LINE_MAX);
}

fn border(line: &mut Vec<u8>, left: &str, right: &str) {
    line.clear();
    line.extend_from_slice(left.as_bytes());
    for _ in 0..LEGEND_COLUMNS - 2 {
        line.extend_from_slice("\u{2500}".as_bytes());
    }
    line.extend_from_slice(right.as_bytes());
}

/// A slider's value: `%.*f` followed by a unit, in the C's
/// `char value[LEGEND_VALUE_WIDTH + 8]`.
fn value_of(value: &mut Vec<u8>, number: f64, decimals: usize, unit: &str) {
    value.clear();
    cfmt::push_fixed(value, number, decimals);
    value.extend_from_slice(unit.as_bytes());
    cut(value, LEGEND_VALUE_WIDTH as usize + 8);
}

/// `build_legend`: the panel's rows into `buffers.lines`; returns how many,
/// `legend_rows()`.
pub fn build_legend_into(sim: &Sim, stats: &Stats, buffers: &mut LegendBuffers) -> usize {
    let config = &sim.config;
    let rows = sim.legend_rows() as usize;
    let inner = (LEGEND_COLUMNS - 2) as usize;
    let LegendBuffers { lines, value, measured, bar } = buffers;

    border(&mut lines[0], "\u{256d}", "\u{256e}");
    value_of(value, config.boundary, 2, "");
    legend_slider(&mut lines[1], bar, "boundary", config.boundary_notch, value, b'b', b'B');
    value_of(value, config.separation, 3, "");
    legend_slider(&mut lines[2], bar, "separation", config.separation_notch, value, b's', b'S');
    value_of(value, config.alignment, 2, "");
    legend_slider(&mut lines[3], bar, "alignment", config.alignment_notch, value, b'a', b'A');
    value_of(value, sim.turning_notch_radians() * 180.0 / PI, 0, "\u{b0}");
    legend_slider(&mut lines[4], bar, "turning", config.turning_notch, value, b't', b'T');
    value.clear();
    {
        use std::io::Write;
        let _ = write!(value, "{}px", config.vision_radius);
    }
    cut(value, LEGEND_VALUE_WIDTH as usize + 8);
    legend_slider(&mut lines[5], bar, "perception", config.vision_notch, value, b'p', b'P');
    value_of(value, config.pace, 1, "\u{d7}");
    legend_slider(&mut lines[6], bar, "speed", config.pace_notch, value, b'v', b'V');
    if config.flocks > 1 {
        value_of(value, f64::from(config.avoid_notch) / 4.0, 2, "\u{d7}");
        legend_slider(&mut lines[7], bar, "avoidance", config.avoid_notch, value, b'g', b'G');
    }

    measured.clear();
    pad_right(measured, b"frame", LEGEND_NAME_WIDTH as usize);
    measured.push(b' ');
    cfmt::push_fixed_width(measured, stats.frame_ms, 5, 1);
    measured.extend_from_slice(b"ms ");
    cfmt::push_fixed_width(measured, stats.bytes / 1024.0, 5, 0);
    measured.extend_from_slice(b"KB ");
    cfmt::push_fixed_width(measured, stats.rate, 3, 0);
    measured.extend_from_slice(b"fps");
    cut(measured, LEGEND_LINE_MAX / 2);
    let row = &mut lines[rows - 3];
    row.clear();
    row.extend_from_slice("\u{2502} ".as_bytes());
    pad_right(row, measured, inner - 2);
    row.extend_from_slice(" \u{2502}".as_bytes());
    cut(row, LEGEND_LINE_MAX);

    let quit = &mut lines[rows - 2];
    quit.clear();
    quit.extend_from_slice("\u{2502} ".as_bytes());
    pad_right(quit, b"quit", LEGEND_NAME_WIDTH as usize);
    quit.extend_from_slice(b" q");
    pad_left(quit, b"", inner - LEGEND_NAME_WIDTH as usize - 4);
    quit.extend_from_slice(" \u{2502}".as_bytes());
    cut(quit, LEGEND_LINE_MAX);

    border(&mut lines[rows - 1], "\u{2570}", "\u{256f}");
    rows
}

/// `build_legend` as owned rows, for tests and callers outside a frame.
pub fn build_legend(sim: &Sim, stats: &Stats) -> Vec<Vec<u8>> {
    let mut buffers = LegendBuffers::default();
    let rows = build_legend_into(sim, stats, &mut buffers);
    buffers.lines[..rows].to_vec()
}

impl Renderer {
    /// `queue_legend`: the panel's rows, or, once, the erase of the rows it
    /// held when a viewport shrank under it. Never a screen erase, which would
    /// take the uploaded sprites with it.
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
        let rows = build_legend_into(sim, &self.stats, &mut self.legend);
        for (row, line) in self.legend.lines[..rows].iter().enumerate() {
            graphics.write_text(row as i32, 0, line)?;
        }
        self.legend_drawn = true;
        Ok(())
    }
}
