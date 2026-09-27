#![cfg(unix)]

//! Differential tests of `render::cells` against the pinned C `cells.c`.
//!
//! Each scenario is a script of calls (`tools/oracle/cells_oracle.c` documents
//! the format). The C oracle runs it against the reference and prints a
//! transcript: every status, all of the emitted text, the text buffer's
//! length and capacity, the whole cell state (both grids, every field) and
//! every painted pixel. The same script runs here against the Rust translation
//! and must print exactly the same transcript. Scenarios run in fresh oracle
//! processes, and a mismatch reports the first divergent line.

mod support;

use std::fmt::Write as _;
use std::path::Path;

use rbirds::image::Image;
use rbirds::render::cells::{Cell, Cells, CellsStyle, cells_status_string};

// --- Scripts ------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Op {
    Init(bool),
    Truecolor(bool),
    Resize(i32, i32),
    Keep(i32, i32),
    Invalidate,
    Canvas(Image),
    Read(CellsStyle, i32, i32),
    Emit,
    Paint(CellsStyle, i32, i32, [u8; 3]),
    State,
    Set { before: bool, index: usize, cell: Cell },
}

fn style_code(style: CellsStyle) -> i32 {
    match style {
        CellsStyle::Braille => 0,
        CellsStyle::Sextants => 1,
        CellsStyle::Blocks => 2,
    }
}

fn script(ops: &[Op]) -> Vec<u8> {
    let mut out = Vec::new();
    for op in ops {
        let line = match op {
            Op::Init(t) => format!("I {}\n", i32::from(*t)),
            Op::Truecolor(t) => format!("T {}\n", i32::from(*t)),
            Op::Resize(c, r) => format!("R {c} {r}\n"),
            Op::Keep(c, r) => format!("K {c} {r}\n"),
            Op::Invalidate => "V\n".to_owned(),
            Op::Canvas(image) => {
                out.extend_from_slice(format!("C {} {}\n", image.width, image.height).as_bytes());
                out.extend_from_slice(&image.pixels);
                "\n".to_owned()
            }
            Op::Read(s, w, h) => format!("D {} {w} {h}\n", style_code(*s)),
            Op::Emit => "E\n".to_owned(),
            Op::Paint(s, w, h, g) => {
                format!("P {} {w} {h} {} {} {}\n", style_code(*s), g[0], g[1], g[2])
            }
            Op::State => "S\n".to_owned(),
            Op::Set { before, index, cell } => format!(
                "G {} {index} {} {} {} {} {} {} {} {} {}\n",
                i32::from(*before),
                cell.glyph,
                cell.fg[0],
                cell.fg[1],
                cell.fg[2],
                cell.bg[0],
                cell.bg[1],
                cell.bg[2],
                i32::from(cell.has_fg),
                i32::from(cell.has_bg)
            ),
        };
        out.extend_from_slice(line.as_bytes());
    }
    out
}

fn escaped(out: &mut String, bytes: &[u8]) {
    for &c in bytes {
        if (0x20..0x7f).contains(&c) && c != b'\\' {
            out.push(c as char);
        } else {
            let _ = write!(out, "\\x{c:02x}");
        }
    }
}

fn print_grid(out: &mut String, name: &str, cells: &Cells, grid: &[Cell]) {
    if grid.is_empty() {
        let _ = writeln!(out, "{name} null");
        return;
    }
    for row in 0..cells.rows {
        let _ = write!(out, "{name} {row}:");
        for col in 0..cells.cols {
            let c = &grid[row as usize * cells.cols as usize + col as usize];
            let _ = write!(
                out,
                " {:x},{:02x}{:02x}{:02x},{:02x}{:02x}{:02x},{}{}",
                c.glyph,
                c.fg[0],
                c.fg[1],
                c.fg[2],
                c.bg[0],
                c.bg[1],
                c.bg[2],
                i32::from(c.has_fg),
                i32::from(c.has_bg)
            );
        }
        out.push('\n');
    }
}

/// Runs the script against the Rust translation, printing what the C oracle
/// prints.
fn transcript(ops: &[Op]) -> String {
    let mut out = String::new();
    let mut cells: Option<Cells> = None;
    let mut canvas = Image::empty();
    for op in ops {
        if !matches!(op, Op::Init(_)) {
            assert!(cells.is_some(), "scripts start with an init");
        }
        match op {
            Op::Init(t) => {
                let status = Cells::new(*t).map(|made| cells = Some(made));
                let _ = writeln!(out, "init {}", cells_status_string(status));
            }
            Op::Truecolor(t) => cells.as_mut().unwrap().truecolor = *t,
            Op::Resize(c, r) => {
                let status = cells.as_mut().unwrap().resize(*c, *r);
                let _ = writeln!(out, "resize {}", cells_status_string(status));
            }
            Op::Keep(c, r) => cells.as_mut().unwrap().keep_out_of(*c, *r),
            Op::Invalidate => cells.as_mut().unwrap().invalidate(),
            Op::Canvas(image) => canvas = image.clone(),
            Op::Read(s, w, h) => cells.as_mut().unwrap().read(*s, &canvas, *w, *h),
            Op::Emit => {
                let cells = cells.as_mut().unwrap();
                let status = cells.emit();
                let _ = write!(
                    out,
                    "emit {} {} {}\ntext ",
                    cells_status_string(status),
                    cells.text.len(),
                    cells.capacity()
                );
                escaped(&mut out, &cells.text);
                out.push('\n');
            }
            Op::Paint(s, w, h, g) => {
                let painted = cells.as_ref().unwrap().paint(*s, *w, *h, *g);
                let _ = write!(
                    out,
                    "paint {}",
                    cells_status_string(painted.as_ref().map(|_| ()).map_err(|e| *e))
                );
                match painted {
                    Ok(picture) => {
                        let _ = writeln!(out, " {} {}", picture.width, picture.height);
                        for line in picture.pixels.chunks(picture.width as usize * 4) {
                            for byte in line {
                                let _ = write!(out, "{byte:02x}");
                            }
                            out.push('\n');
                        }
                    }
                    Err(_) => out.push('\n'),
                }
            }
            Op::State => {
                let cells = cells.as_ref().unwrap();
                let _ = writeln!(
                    out,
                    "state {} {} {} {} {} {}",
                    cells.cols,
                    cells.rows,
                    i32::from(cells.draw_everything),
                    cells.keep_cols,
                    cells.keep_rows,
                    i32::from(cells.truecolor)
                );
                print_grid(&mut out, "now", cells, &cells.now);
                print_grid(&mut out, "before", cells, &cells.before);
            }
            Op::Set { before, index, cell } => {
                let cells = cells.as_mut().unwrap();
                let grid = if *before { &mut cells.before } else { &mut cells.now };
                if let Some(slot) = grid.get_mut(*index) {
                    *slot = *cell;
                }
            }
        }
    }
    out
}

fn oracle() -> Option<std::path::PathBuf> {
    support::oracle::build("cells_oracle", "cells_oracle.c", &["cells.c", "png.c"])
}

fn excerpt(line: &str, at: usize) -> String {
    let start = at.saturating_sub(60);
    let end = (at + 60).min(line.len());
    format!("[{start}..{end}] {}", line.get(start..end).unwrap_or(line))
}

/// Runs one scenario through both and insists the transcripts are identical.
fn check(exe: &Path, name: &str, ops: &[Op]) {
    let input = script(ops);
    let output = support::oracle::run(exe, &[], &input);
    let expected = String::from_utf8(output.stdout).expect("oracle transcript is text");
    let actual = transcript(ops);
    if expected == actual {
        return;
    }
    let scratch = support::oracle::Scratch::new(&format!("cells_oracle-{name}"));
    std::fs::write(scratch.file("script"), &input).unwrap();
    std::fs::write(scratch.file("c.txt"), &expected).unwrap();
    std::fs::write(scratch.file("rust.txt"), &actual).unwrap();
    let (c_lines, rust_lines): (Vec<&str>, Vec<&str>) =
        (expected.lines().collect(), actual.lines().collect());
    for i in 0..c_lines.len().max(rust_lines.len()) {
        let (c, r) = (
            c_lines.get(i).copied().unwrap_or("<end>"),
            rust_lines.get(i).copied().unwrap_or("<end>"),
        );
        if c != r {
            let at =
                c.bytes().zip(r.bytes()).position(|(a, b)| a != b).unwrap_or(c.len().min(r.len()));
            panic!(
                "scenario {name}: first divergence at transcript line {} byte {at}\n  C:    {}\n  Rust: {}",
                i + 1,
                excerpt(c, at),
                excerpt(r, at)
            );
        }
    }
    panic!("scenario {name}: transcripts differ");
}

// --- Corpus -------------------------------------------------------------------

/// SplitMix64: a fixed, seedable source for the corpus.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u32) -> u32 {
        (self.next() % u64::from(n)) as u32
    }

    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1) as u32) as i32
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }

    fn percent(&mut self, p: u32) -> bool {
        self.below(100) < p
    }

    fn rgb(&mut self) -> [u8; 3] {
        [self.byte(), self.byte(), self.byte()]
    }
}

fn sky(width: i32, height: i32) -> Image {
    Image::alloc(width, height).expect("canvas")
}

fn fill(image: &mut Image, x: i32, y: i32, w: i32, h: i32, rgba: [u8; 4]) {
    for yy in y.max(0)..(y + h).min(image.height) {
        for xx in x.max(0)..(x + w).min(image.width) {
            let at = image.offset(xx, yy);
            image.pixels[at..at + 4].copy_from_slice(&rgba);
        }
    }
}

/// Transparent, but with colour in the transparent pixels, which must count
/// towards coverage and nothing else.
fn garbage_sky(rng: &mut Rng, width: i32, height: i32) -> Image {
    let mut image = sky(width, height);
    for px in image.pixels.chunks_mut(4) {
        px[..3].copy_from_slice(&rng.rgb());
    }
    image
}

/// Every pixel inked with probability `density`%, its alpha drawn by `alpha`,
/// its colour from a palette of `colours` (more than eight overflows a patch's
/// table and takes the mean).
fn noise(
    rng: &mut Rng,
    width: i32,
    height: i32,
    density: u32,
    colours: usize,
    alpha: fn(&mut Rng) -> u8,
) -> Image {
    let palette: Vec<[u8; 3]> = (0..colours).map(|_| rng.rgb()).collect();
    let mut image = sky(width, height);
    for px in image.pixels.chunks_mut(4) {
        if rng.percent(density) {
            let rgb = palette[rng.below(colours as u32) as usize];
            px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha(rng)]);
        }
    }
    image
}

fn opaque(_: &mut Rng) -> u8 {
    255
}

fn any_alpha(rng: &mut Rng) -> u8 {
    rng.byte()
}

/// Alphas about the ink threshold of 64.
fn faint(rng: &mut Rng) -> u8 {
    [60, 62, 63, 64, 65, 66, 70, 128, 255][rng.below(9) as usize]
}

#[derive(Clone)]
struct Bird {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    rgb: [u8; 3],
    fringe: bool,
}

/// Flat-tinted rectangles with an anti-aliased fringe, like the sprites, that
/// move a little between frames.
struct Flock {
    birds: Vec<Bird>,
}

impl Flock {
    fn new(
        rng: &mut Rng,
        count: usize,
        width: i32,
        height: i32,
        cell: (i32, i32),
        colours: usize,
    ) -> Flock {
        let palette: Vec<[u8; 3]> = (0..colours).map(|_| rng.rgb()).collect();
        let birds = (0..count)
            .map(|_| Bird {
                x: rng.range(-cell.0, width),
                y: rng.range(-cell.1, height),
                w: rng.range(1, cell.0 * 2 + 1),
                h: rng.range(1, cell.1 * 2 + 1),
                rgb: palette[rng.below(colours as u32) as usize],
                fringe: rng.percent(60),
            })
            .collect();
        Flock { birds }
    }

    /// Moves about a third of the birds by up to a cell, so most cells stay.
    fn step(&mut self, rng: &mut Rng, cell: (i32, i32)) {
        for bird in &mut self.birds {
            if rng.percent(35) {
                bird.x += rng.range(-cell.0, cell.0);
                bird.y += rng.range(-cell.1, cell.1);
            }
        }
    }

    fn draw(&self, rng: &mut Rng, width: i32, height: i32) -> Image {
        let mut image = sky(width, height);
        for bird in &self.birds {
            if bird.fringe {
                let [r, g, b] = bird.rgb;
                let a = rng.range(1, 254) as u8;
                fill(&mut image, bird.x - 1, bird.y - 1, bird.w + 2, bird.h + 2, [r, g, b, a]);
            }
            let [r, g, b] = bird.rgb;
            fill(&mut image, bird.x, bird.y, bird.w, bird.h, [r, g, b, 255]);
        }
        image
    }
}

const STYLES: [CellsStyle; 3] = [CellsStyle::Braille, CellsStyle::Sextants, CellsStyle::Blocks];

const GROUND: [u8; 3] = [18, 18, 24];

fn other(style: CellsStyle) -> CellsStyle {
    match style {
        CellsStyle::Braille => CellsStyle::Blocks,
        CellsStyle::Sextants => CellsStyle::Braille,
        CellsStyle::Blocks => CellsStyle::Sextants,
    }
}

fn frame(ops: &mut Vec<Op>, canvas: Image, style: CellsStyle, cw: i32, ch: i32) {
    ops.push(Op::Canvas(canvas));
    ops.push(Op::Read(style, cw, ch));
    ops.push(Op::Emit);
    ops.push(Op::State);
}

/// One style, colour mode and cell size through a life: use before sizing,
/// sizing, a moving flock, every kind of canvas, the panel corner, an
/// invalidation, a colour mode change, resizes good and bad, and paintings in
/// the style read and in the others.
fn life(style: CellsStyle, truecolor: bool, cw: i32, ch: i32, seed: u64) -> Vec<Op> {
    let mut rng = Rng(seed);
    let (ecw, ech) = (cw.max(1), ch.max(1));
    let cols = rng.range(3, 14);
    let rows = rng.range(1, 6);
    let mut ops = vec![Op::Init(truecolor), Op::Emit, Op::Paint(style, 8, 16, GROUND), Op::State];
    // A read with no grid and no canvas does nothing.
    ops.push(Op::Read(style, cw, ch));
    ops.push(Op::Resize(cols, rows));
    ops.push(Op::State);
    // Before any canvas: the read is refused, the emit draws the blank grid.
    ops.push(Op::Read(style, cw, ch));
    ops.push(Op::Emit);
    ops.push(Op::State);

    // The canvas a little smaller or larger than the grid needs.
    let slack = |rng: &mut Rng| rng.range(-2, 3);
    let (mut width, mut height) =
        ((cols * ecw + slack(&mut rng)).max(1), (rows * ech + slack(&mut rng)).max(1));
    let (count, colours) = (rng.range(1, 8) as usize, rng.range(1, 5) as usize);
    let mut flock = Flock::new(&mut rng, count, width, height, (ecw, ech), colours);
    for f in 0..6 {
        let canvas = flock.draw(&mut rng, width, height);
        frame(&mut ops, canvas, style, cw, ch);
        if f == 2 {
            ops.push(Op::Paint(style, cw, ch, GROUND));
            ops.push(Op::Paint(style, 8, 16, [1, 2, 3]));
            ops.push(Op::Paint(other(style), ecw.max(2), ech.max(4), GROUND));
        }
        flock.step(&mut rng, (ecw, ech));
    }
    // The same frame twice: nothing to emit.
    let canvas = flock.draw(&mut rng, width, height);
    frame(&mut ops, canvas.clone(), style, cw, ch);
    frame(&mut ops, canvas, style, cw, ch);

    let kinds: [fn(&mut Rng, i32, i32) -> Image; 9] = [
        |rng, w, h| garbage_sky(rng, w, h),
        |_, w, h| {
            let mut image = sky(w, h);
            fill(&mut image, 0, 0, w, h, [200, 30, 90, 255]);
            image
        },
        |rng, w, h| noise(rng, w, h, 50, 3, opaque),
        |rng, w, h| noise(rng, w, h, 70, 7, any_alpha),
        |rng, w, h| noise(rng, w, h, 90, 8, any_alpha),
        |rng, w, h| noise(rng, w, h, 95, 40, any_alpha),
        |rng, w, h| noise(rng, w, h, 100, 2, faint),
        |rng, w, h| noise(rng, w, h, 30, 5, faint),
        |rng, w, h| {
            // Uniform alpha either side of the threshold, in bands.
            let mut image = sky(w, h);
            let band = (h / 3).max(1);
            for (i, alpha) in [63u8, 64, 65].into_iter().enumerate() {
                let rgb = rng.rgb();
                fill(&mut image, 0, i as i32 * band, w, band, [rgb[0], rgb[1], rgb[2], alpha]);
            }
            image
        },
    ];
    for kind in kinds {
        let canvas = kind(&mut rng, width, height);
        frame(&mut ops, canvas, style, cw, ch);
    }
    ops.push(Op::Paint(style, ecw.max(2), ech.max(4), GROUND));

    // The panel's corner: in, moved, out, and larger than the grid.
    for (kc, kr) in [(2, 1), (2, 1), (cols, rows), (cols + 3, rows + 2), (-1, 3), (0, 0)] {
        ops.push(Op::Keep(kc, kr));
        let canvas = noise(&mut rng, width, height, 60, 4, opaque);
        frame(&mut ops, canvas, style, cw, ch);
    }
    ops.push(Op::Invalidate);
    frame(&mut ops, flock.draw(&mut rng, width, height), style, cw, ch);
    ops.push(Op::Truecolor(!truecolor));
    flock.step(&mut rng, (ecw, ech));
    frame(&mut ops, flock.draw(&mut rng, width, height), style, cw, ch);

    // Resizes: refused, unchanged, then real.
    ops.push(Op::Resize(0, rows));
    ops.push(Op::Resize(cols, -1));
    ops.push(Op::Resize(cols, rows));
    ops.push(Op::State);
    let (cols, rows) = (cols + 2, rows + 1);
    ops.push(Op::Resize(cols, rows));
    ops.push(Op::State);
    // Read the old canvas into the new grid before a new one arrives.
    ops.push(Op::Read(style, cw, ch));
    ops.push(Op::Emit);
    ops.push(Op::State);
    width = cols * ecw;
    height = rows * ech;
    let mut flock = Flock::new(&mut rng, 6, width, height, (ecw, ech), 3);
    for _ in 0..3 {
        frame(&mut ops, flock.draw(&mut rng, width, height), style, cw, ch);
        flock.step(&mut rng, (ecw, ech));
    }
    // Styles change under a running grid, as the renderer switch allows.
    for read_as in STYLES {
        frame(&mut ops, flock.draw(&mut rng, width, height), read_as, cw, ch);
        for paint_as in STYLES {
            ops.push(Op::Paint(paint_as, ecw.max(2), ech.max(4), GROUND));
        }
    }
    ops.push(Op::Paint(style, 2, 3, GROUND));
    ops.push(Op::Paint(style, 1, 4, GROUND));
    ops.push(Op::Paint(style, 2, 4, [255, 255, 255]));
    ops.push(Op::Paint(style, 3, 13, GROUND));
    ops
}

const CELL_SIZES: [(i32, i32); 14] = [
    (1, 1),
    (8, 16),
    (2, 3),
    (2, 4),
    (3, 5),
    (7, 13),
    (4, 8),
    (5, 9),
    (1, 7),
    (9, 2),
    (6, 12),
    (10, 20),
    (0, 0),
    (-3, 5),
];

#[test]
fn every_style_colour_mode_and_cell_size_matches_the_reference() {
    let Some(exe) = oracle() else { return };
    let mut seed = 1;
    for style in STYLES {
        for truecolor in [true, false] {
            for (cw, ch) in CELL_SIZES {
                seed += 1;
                let name = format!("life-{style:?}-{truecolor}-{cw}x{ch}");
                check(&exe, &name, &life(style, truecolor, cw, ch, seed));
            }
        }
    }
}

#[test]
fn filling_the_colour_table_switches_to_the_reference_mean() {
    let Some(exe) = oracle() else { return };
    for style in STYLES {
        for truecolor in [true, false] {
            let mut ops = vec![Op::Init(truecolor), Op::Resize(1, 1)];
            for (x, y) in [(0, 0), (7, 7), (3, 4)] {
                for alpha in [0, 1, 63, 64, 255] {
                    let mut image = sky(8, 8);
                    for col in 0..8 {
                        // Seven colours; the first has twice the weight.
                        let shade = (col % 7) as u8;
                        fill(&mut image, col, 0, 1, 8, [24 + shade * 16, 40, 70, 255]);
                    }
                    frame(&mut ops, image.clone(), style, 8, 8);
                    // An eighth colour, early or late in the scan, switches
                    // to the mean only when its alpha is nonzero.
                    fill(&mut image, x, y, 1, 1, [249, 250, 251, alpha]);
                    frame(&mut ops, image, style, 8, 8);
                }
            }
            check(&exe, &format!("colour-table-{style:?}-{truecolor}"), &ops);
        }
    }
}

/// Changed cells at chosen columns of a wide row: hops of 98 and 99 are a
/// cursor forward, 100 and 101 an absolute move, and the panel's corner
/// forgets where the cursor is.
#[test]
fn cursor_hops_match_the_reference() {
    let Some(exe) = oracle() else { return };
    for truecolor in [true, false] {
        let cols = 260;
        let mut ops = vec![Op::Init(truecolor), Op::Resize(cols, 2)];
        let patterns: [&[i32]; 12] = [
            &[],
            &[0, 99],
            &[],
            &[5, 105],
            &[],
            &[10, 111],
            &[],
            &[20, 122],
            &[0, 1, 2, 50, 150, 249, 259],
            &[1, 2, 3, 51, 149, 250],
            &[31, 32, 60, 159],
            &[],
        ];
        for (i, columns) in patterns.iter().enumerate() {
            if i == 10 {
                ops.push(Op::Keep(30, 1));
            }
            let mut canvas = sky(cols, 2);
            for &col in *columns {
                let shade = (40 + col) as u8;
                fill(&mut canvas, col, 0, 1, 1, [shade, 9, 9, 255]);
                fill(&mut canvas, cols - 1 - col, 1, 1, 1, [9, 9, 9, 255]);
            }
            frame(&mut ops, canvas, CellsStyle::Braille, 1, 1);
        }
        check(&exe, &format!("hops-{truecolor}"), &ops);
    }
}

fn random_cell(rng: &mut Rng, palette: &[[u8; 3]]) -> Cell {
    const GLYPHS: [u32; 16] = [
        0,
        0x20,
        0x41,
        0x7F,
        0x80,
        0x7FF,
        0x800,
        0x2801,
        0x28FF,
        0xFFFF,
        0x10000,
        0x1FB00,
        0x10FFFF,
        0x110000,
        0x7FFF_FFFF,
        0xFFFF_FFFF,
    ];
    Cell {
        glyph: GLYPHS[rng.below(GLYPHS.len() as u32) as usize],
        fg: palette[rng.below(palette.len() as u32) as usize],
        bg: palette[rng.below(palette.len() as u32) as usize],
        has_fg: rng.percent(60),
        has_bg: rng.percent(40),
    }
}

/// Cells set directly, as no canvas could make them: every UTF-8 length and
/// the values beyond it, background without foreground, colours without their
/// flags, stale grids after a swap. Checks the pen, the resets and the moves.
#[test]
fn directly_set_cells_emit_as_the_reference_does() {
    let Some(exe) = oracle() else { return };
    for (seed, truecolor) in [(11u64, true), (12, false), (13, true), (14, false)] {
        let mut rng = Rng(seed);
        let (cols, rows) = (23, 6);
        let palette: Vec<[u8; 3]> =
            (0..4).map(|_| rng.rgb()).chain([[0, 0, 0], [255, 255, 255], [25, 26, 76]]).collect();
        let mut ops = vec![Op::Init(truecolor), Op::Resize(cols, rows)];
        for round in 0..10 {
            let changes = if round == 0 { 100 } else { rng.range(0, 50) as u32 };
            for index in 0..(cols * rows) as usize {
                if rng.percent(changes) {
                    ops.push(Op::Set {
                        before: false,
                        index,
                        cell: random_cell(&mut rng, &palette),
                    });
                }
                if rng.percent(3) {
                    ops.push(Op::Set {
                        before: true,
                        index,
                        cell: random_cell(&mut rng, &palette),
                    });
                }
            }
            // Out of range: ignored by both.
            ops.push(Op::Set {
                before: false,
                index: 100_000,
                cell: random_cell(&mut rng, &palette),
            });
            match round {
                3 => ops.push(Op::Keep(4, 2)),
                5 => ops.push(Op::Invalidate),
                6 => ops.push(Op::Truecolor(!truecolor)),
                8 => ops.push(Op::Keep(0, 0)),
                _ => {}
            }
            ops.push(Op::Emit);
            ops.push(Op::State);
            for style in STYLES {
                ops.push(Op::Paint(style, 4, 8, GROUND));
            }
        }
        check(&exe, &format!("direct-{seed}"), &ops);
    }
}

/// The benchmark's grid: 200 by 50 cells of 8 by 16 pixels, a flock moving
/// through it, read in each style.
#[test]
fn a_full_size_screen_matches_the_reference() {
    let Some(exe) = oracle() else { return };
    let mut rng = Rng(99);
    let (cols, rows, cw, ch) = (200, 50, 8, 16);
    let (width, height) = (cols * cw, rows * ch);
    let mut flock = Flock::new(&mut rng, 300, width, height, (cw, ch), 5);
    let mut ops = vec![Op::Init(true), Op::Resize(cols, rows), Op::Keep(30, 12)];
    for style in STYLES {
        for _ in 0..3 {
            frame(&mut ops, flock.draw(&mut rng, width, height), style, cw, ch);
            flock.step(&mut rng, (cw, ch));
        }
    }
    ops.push(Op::Paint(CellsStyle::Blocks, cw, ch, GROUND));
    check(&exe, "full-size", &ops);
}

/// Paintings the image allocator refuses are out of memory, as in the C:
/// wider than 16384 pixels, or more than 2^26 of them.
#[test]
fn oversized_paintings_are_refused_as_the_reference_refuses_them() {
    let Some(exe) = oracle() else { return };
    let mut rng = Rng(7);
    let mut ops = vec![Op::Init(true), Op::Resize(200, 1)];
    frame(&mut ops, noise(&mut rng, 200, 1, 80, 3, opaque), CellsStyle::Blocks, 1, 1);
    ops.push(Op::Paint(CellsStyle::Blocks, 82, 4, GROUND));
    ops.push(Op::Paint(CellsStyle::Blocks, 100, 16, GROUND));
    ops.push(Op::Paint(CellsStyle::Blocks, 81, 4, GROUND));
    ops.push(Op::Resize(1, 1));
    frame(&mut ops, noise(&mut rng, 1, 1, 100, 1, opaque), CellsStyle::Braille, 1, 1);
    ops.push(Op::Paint(CellsStyle::Braille, 16384, 4097, GROUND));
    ops.push(Op::Paint(CellsStyle::Braille, 16385, 4, GROUND));
    ops.push(Op::Paint(CellsStyle::Braille, 2, 16385, GROUND));
    check(&exe, "oversized", &ops);
}
