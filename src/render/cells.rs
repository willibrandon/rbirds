//! A frame as a grid of terminal cells, for terminals that draw no images,
//! translated from cbirds `cells.c`.
//!
//! The simulation is rendered into a pixel canvas exactly as it is for a
//! recording, and the canvas is then read back as cells: eight braille dots a
//! cell, six sextant blocks, or two half blocks, each cell wearing the colour
//! of whatever bird is in it. What comes out is the escape text that puts that
//! grid on a screen — and only the cells that changed since the last frame,
//! because a terminal with no graphics protocol is usually also a terminal with
//! no spare bandwidth.
//!
//! Nothing here knows about birds. It knows RGBA pixels, cell sizes, and the
//! three sequences every terminal since the VT100 has understood: move, colour,
//! print.

#![forbid(unsafe_code)]

use std::fmt;

use crate::fp::mul_add;
use crate::image::Image;

/// `cells_status_t` without `CELLS_OK`, which is `Ok(..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellsError {
    Argument,
    Memory,
}

impl CellsError {
    /// `cells_status_string` for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            CellsError::Argument => "invalid argument",
            CellsError::Memory => "out of memory",
        }
    }
}

impl fmt::Display for CellsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for CellsError {}

/// `cells_status_string` over a whole status, `CELLS_OK` included.
pub fn cells_status_string(status: Result<(), CellsError>) -> &'static str {
    match status {
        Ok(()) => "ok",
        Err(error) => error.as_str(),
    }
}

/// `cells_style_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CellsStyle {
    /// Two by four dots a cell: the finest thing text can do.
    Braille,
    /// Two by three solid blocks a cell: bolder, nearly as fine.
    Sextants,
    /// Two half blocks a cell: coarser, and colour on every pixel.
    Blocks,
}

/// `cell_t`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Cell {
    /// A code point; zero is an empty cell.
    pub glyph: u32,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub has_fg: bool,
    pub has_bg: bool,
}

/// `cells_t`.
///
/// `now` and `before` hold `cols * rows` cells once sized; empty stands for
/// the C grids' NULL before the first successful [`Cells::resize`]. `text` is
/// what [`Cells::emit`] produced (its `len()` is the C `length`; the C NUL
/// terminator is not stored).
#[derive(Clone, Debug)]
pub struct Cells {
    pub cols: i32,
    pub rows: i32,
    pub now: Vec<Cell>,
    pub before: Vec<Cell>,
    /// The next emit redraws every cell, blanks included.
    pub draw_everything: bool,
    /// A top left rectangle that belongs to somebody else.
    pub keep_cols: i32,
    pub keep_rows: i32,
    /// 24 bit SGR; otherwise the nearest of the 256 colour cube.
    pub truecolor: bool,
    pub text: Vec<u8>,
    /// The C `capacity`, NUL slot included, which decides when `text` grows.
    capacity: usize,
}

/// How much of a dot's patch of pixels has to be ink before the dot lights: a
/// quarter. Less and every anti-aliased fringe is a dot and the birds are fat
/// blobs; more and the wing tips vanish and the birds are commas.
const INK_THRESHOLD: i32 = 64;

/// Braille numbers its dots down the left column then down the right, with the
/// fourth row bolted on afterwards: 1 2 3 7 on the left, 4 5 6 8 on the right.
const BRAILLE_BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

/// The braille code point for a two by four dot pattern: bit
/// `column + row * 2` for each lit dot, column 0..1, row 0..3
/// (`cells_braille`).
pub fn braille(dots: u32) -> u32 {
    let mut pattern = 0u32;
    for (row, bits) in BRAILLE_BIT.iter().enumerate() {
        for (column, bit) in bits.iter().enumerate() {
            if dots & (1u32 << (column + row * 2)) != 0 {
                pattern |= bit;
            }
        }
    }
    0x2800 + pattern
}

/// The sextant code point for a two by three block pattern, bit
/// `column + row * 2` for each filled block, row 0..2 (`cells_sextant`).
///
/// Unicode 13 laid the sextants out by the value of their pattern, top left the
/// lowest bit, and left out the patterns that already existed as characters:
/// the left and right halves, nothing, and everything.
pub fn sextant(blocks: u32) -> u32 {
    let blocks = blocks & 0x3F;
    if blocks == 0 {
        return u32::from(b' ');
    }
    if blocks == 0x3F {
        return 0x2588; // Full block.
    }
    if blocks == 0x15 {
        return 0x258C; // Left half: rows one, two and three, left.
    }
    if blocks == 0x2A {
        return 0x2590; // Right half.
    }
    let mut code = 0x1FB00 + blocks - 1;
    if blocks > 0x15 {
        code -= 1;
    }
    if blocks > 0x2A {
        code -= 1;
    }
    code
}

/// The ink in one rectangle of the canvas: how much of it there is, and what
/// colour it is. The colour is the one that fills most of the patch, not the
/// average: two birds of different shades sharing a cell used to average into a
/// colour that was neither, that changed a little every frame as they moved,
/// and that was different from every other cell's — ten thousand distinct
/// colours a recording, each costing a colour sequence. The sprites are flat
/// tints, so the colours in a patch are few and exact; a small table sorts them.
const PATCH_COLOURS: usize = 8;

#[derive(Clone, Copy)]
struct Patch {
    /// 0 to 255, mean alpha.
    coverage: f64,
    rgb: [u8; 3],
}

fn read_patch(canvas: &Image, x0: i32, y0: i32, width: i32, height: i32) -> Patch {
    let mut patch = Patch { coverage: 0.0, rgb: [0, 0, 0] };
    let mut colour = [[0u8; 3]; PATCH_COLOURS];
    let mut weight = [0.0f64; PATCH_COLOURS];
    let mut colours = 0usize;
    let (mut alpha_sum, mut red, mut green, mut blue) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut counted: i64 = 0;
    // The C walks the whole rectangle and skips what lies off the canvas; only
    // the pixels on it do anything, so the walk starts and stops at the canvas
    // edges, in the same order.
    let y_start = y0.max(0);
    let y_end = (i64::from(y0) + i64::from(height)).min(i64::from(canvas.height)) as i32;
    let x_start = x0.max(0);
    let x_end = (i64::from(x0) + i64::from(width)).min(i64::from(canvas.width)) as i32;
    for y in y_start..y_end {
        for x in x_start..x_end {
            let at = canvas.offset(x, y);
            let px = &canvas.pixels[at..at + 4];
            let a = f64::from(px[3]);
            counted += 1;
            alpha_sum += a;
            if a == 0.0 {
                continue;
            }
            // fma: cells.c:130:17
            red = mul_add(f64::from(px[0]), a, red);
            // fma: cells.c:131:19
            green = mul_add(f64::from(px[1]), a, green);
            // fma: cells.c:132:18
            blue = mul_add(f64::from(px[2]), a, blue);
            let rgb = [px[0], px[1], px[2]];
            let mut slot = 0;
            while slot < colours && colour[slot] != rgb {
                slot += 1;
            }
            if slot == colours && colours < PATCH_COLOURS {
                colour[colours] = rgb;
                colours += 1;
            }
            if slot < colours {
                weight[slot] += a;
            }
        }
    }
    if counted == 0 || alpha_sum == 0.0 {
        return patch;
    }
    patch.coverage = alpha_sum / counted as f64;
    let mut best: Option<usize> = None;
    for slot in 0..colours {
        if best.is_none_or(|best| weight[slot] > weight[best]) {
            best = Some(slot);
        }
    }
    match best {
        Some(best) if colours < PATCH_COLOURS => patch.rgb = colour[best],
        _ => {
            // More colours than the table holds: a sprite of somebody's own,
            // with shading of its own. The mean is the honest answer for that.
            patch.rgb[0] = (red / alpha_sum + 0.5) as u8;
            patch.rgb[1] = (green / alpha_sum + 0.5) as u8;
            patch.rgb[2] = (blue / alpha_sum + 0.5) as u8;
        }
    }
    patch
}

fn inked(patch: &Patch) -> bool {
    patch.coverage >= f64::from(INK_THRESHOLD)
}

/// Dots need only coverage. Computing a colour histogram for each dot would
/// repeat the whole cell's colour work up to eight times. Alpha sums are
/// integers, so comparing before division also preserves the exact threshold.
fn patch_inked(canvas: &Image, x0: i32, y0: i32, width: i32, height: i32) -> bool {
    let y_start = y0.max(0);
    let y_end = (i64::from(y0) + i64::from(height)).min(i64::from(canvas.height)) as i32;
    let x_start = x0.max(0);
    let x_end = (i64::from(x0) + i64::from(width)).min(i64::from(canvas.width)) as i32;
    if x_end <= x_start || y_end <= y_start {
        return false;
    }
    let pixels = (x_end - x_start) as u64 * (y_end - y_start) as u64;
    let mut alpha = 0_u64;
    for y in y_start..y_end {
        let start = canvas.offset(x_start, y);
        let end = start + (x_end - x_start) as usize * 4;
        alpha += canvas.pixels[start..end].chunks_exact(4).map(|p| u64::from(p[3])).sum::<u64>();
    }
    alpha >= INK_THRESHOLD as u64 * pixels
}

fn read_braille_cell(canvas: &Image, x0: i32, y0: i32, cell_width: i32, cell_height: i32) -> Cell {
    // Dots are the cell divided two by four; a cell narrower than two pixels
    // or shorter than four gets what it gets.
    let mut dots = 0u32;
    for row in 0..4 {
        for column in 0..2 {
            let dx0 = x0 + column * cell_width / 2;
            let mut dx1 = x0 + (column + 1) * cell_width / 2;
            let dy0 = y0 + row * cell_height / 4;
            let mut dy1 = y0 + (row + 1) * cell_height / 4;
            if dx1 <= dx0 {
                dx1 = dx0 + 1;
            }
            if dy1 <= dy0 {
                dy1 = dy0 + 1;
            }
            if patch_inked(canvas, dx0, dy0, dx1 - dx0, dy1 - dy0) {
                dots |= 1u32 << (column + row * 2);
            }
        }
    }
    let mut cell = Cell::default();
    if dots == 0 {
        return cell;
    }
    // The colour is the whole cell's, so two dots of one bird agree.
    let whole = read_patch(canvas, x0, y0, cell_width, cell_height);
    cell.glyph = braille(dots);
    cell.fg = whole.rgb;
    cell.has_fg = true;
    cell
}

fn read_sextant_cell(canvas: &Image, x0: i32, y0: i32, cell_width: i32, cell_height: i32) -> Cell {
    let mut blocks = 0u32;
    for row in 0..3 {
        for column in 0..2 {
            let bx0 = x0 + column * cell_width / 2;
            let mut bx1 = x0 + (column + 1) * cell_width / 2;
            let by0 = y0 + row * cell_height / 3;
            let mut by1 = y0 + (row + 1) * cell_height / 3;
            if bx1 <= bx0 {
                bx1 = bx0 + 1;
            }
            if by1 <= by0 {
                by1 = by0 + 1;
            }
            if patch_inked(canvas, bx0, by0, bx1 - bx0, by1 - by0) {
                blocks |= 1u32 << (column + row * 2);
            }
        }
    }
    let mut cell = Cell::default();
    if blocks == 0 {
        return cell;
    }
    let whole = read_patch(canvas, x0, y0, cell_width, cell_height);
    cell.glyph = sextant(blocks);
    cell.fg = whole.rgb;
    cell.has_fg = true;
    cell
}

fn read_block_cell(canvas: &Image, x0: i32, y0: i32, cell_width: i32, cell_height: i32) -> Cell {
    let mut half = cell_height / 2;
    if half < 1 {
        half = 1;
    }
    let top = read_patch(canvas, x0, y0, cell_width, half);
    let bottom = read_patch(canvas, x0, y0 + half, cell_width, cell_height - half);
    let (top_ink, bottom_ink) = (inked(&top), inked(&bottom));
    let mut cell = Cell::default();
    if !top_ink && !bottom_ink {
        return cell;
    }
    // Never paint the sky: an empty half is the terminal's own background, so
    // the upper half block is used when the top has ink and the lower when only
    // the bottom does, and the background colour is set only when both do.
    if top_ink {
        cell.glyph = 0x2580; // Upper half block.
        cell.fg = top.rgb;
        cell.has_fg = true;
        if bottom_ink {
            cell.bg = bottom.rgb;
            cell.has_bg = true;
        }
    } else {
        cell.glyph = 0x2584; // Lower half block.
        cell.fg = bottom.rgb;
        cell.has_fg = true;
    }
    cell
}

// --- Painting ---------------------------------------------------------------

fn paint_rect(out: &mut Image, x: i32, y: i32, width: i32, height: i32, rgb: [u8; 3]) {
    // As in read_patch, the rectangle is clipped to the image rather than
    // walked and skipped; the pixels written are the same.
    let y_start = y.max(0);
    let y_end = (i64::from(y) + i64::from(height)).min(i64::from(out.height)) as i32;
    let x_start = x.max(0);
    let x_end = (i64::from(x) + i64::from(width)).min(i64::from(out.width)) as i32;
    for yy in y_start..y_end {
        for xx in x_start..x_end {
            let at = out.offset(xx, yy);
            out.pixels[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
}

// --- Emission ---------------------------------------------------------------

/// Room for one `snprintf` into the C's `scratch[48]`: every sequence built
/// here is at most 26 bytes, so nothing is ever truncated.
struct Scratch {
    bytes: [u8; 48],
    length: usize,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch { bytes: [0; 48], length: 0 }
    }

    fn text(&mut self, text: &[u8]) -> &mut Scratch {
        self.bytes[self.length..self.length + text.len()].copy_from_slice(text);
        self.length += text.len();
        self
    }

    /// `%d`.
    fn int(&mut self, value: i32) -> &mut Scratch {
        if value < 0 {
            self.text(b"-");
        }
        let mut digits = [0u8; 10];
        let mut at = digits.len();
        let mut rest = value.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        self.text(&digits[at..])
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

/// The nearest entry of the 6x6x6 cube, for a terminal that has no 24 bit
/// colour.
fn cube_index(rgb: [u8; 3]) -> i32 {
    let levels = rgb.map(|v| (i32::from(v) + 25) / 51);
    16 + 36 * levels[0] + 6 * levels[1] + levels[2]
}

/// `pen_t`: what the terminal's pen is known to hold.
#[derive(Clone, Copy, Default)]
struct Pen {
    has_fg: bool,
    has_bg: bool,
    fg: [u8; 3],
    bg: [u8; 3],
}

fn same_cell(a: &Cell, b: &Cell) -> bool {
    if a.glyph != b.glyph || a.has_fg != b.has_fg || a.has_bg != b.has_bg {
        return false;
    }
    if a.has_fg && a.fg != b.fg {
        return false;
    }
    if a.has_bg && a.bg != b.bg {
        return false;
    }
    true
}

/// A zeroed grid of `count` cells, or `None` where the C `calloc` fails.
fn zeroed(count: usize) -> Option<Vec<Cell>> {
    let mut grid = Vec::new();
    grid.try_reserve_exact(count).ok()?;
    grid.resize(count, Cell::default());
    Some(grid)
}

impl Default for Cells {
    /// The C's zero-initialized static `cells_t` before `cells_init`.
    fn default() -> Cells {
        Cells {
            cols: 0,
            rows: 0,
            now: Vec::new(),
            before: Vec::new(),
            draw_everything: false,
            keep_cols: 0,
            keep_rows: 0,
            truecolor: false,
            text: Vec::new(),
            capacity: 0,
        }
    }
}

impl Cells {
    /// `cells_init`: an unsized grid that will draw everything first.
    /// `truecolor` picks 24 bit SGR; otherwise the nearest of the 256 colour
    /// cube. It cannot fail; the C's only refusal is a NULL context.
    pub fn new(truecolor: bool) -> Result<Cells, CellsError> {
        Ok(Cells {
            cols: 0,
            rows: 0,
            now: Vec::new(),
            before: Vec::new(),
            draw_everything: true,
            keep_cols: 0,
            keep_rows: 0,
            truecolor,
            text: Vec::new(),
            capacity: 0,
        })
    }

    /// The C `capacity` of the text buffer, NUL slot included: 0 until the
    /// first emit that writes anything, then 8192 doubling as needed.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// `cells_resize`. Both grids are replaced with blank ones and everything
    /// is drawn next time, unless the size is unchanged, which changes
    /// nothing. On failure the old grids stay.
    pub fn resize(&mut self, cols: i32, rows: i32) -> Result<(), CellsError> {
        if cols <= 0 || rows <= 0 {
            return Err(CellsError::Argument);
        }
        if cols == self.cols && rows == self.rows {
            return Ok(());
        }
        let count = cols as usize * rows as usize;
        let (Some(now), Some(before)) = (zeroed(count), zeroed(count)) else {
            return Err(CellsError::Memory);
        };
        self.now = now;
        self.before = before;
        self.cols = cols;
        self.rows = rows;
        self.draw_everything = true;
        Ok(())
    }

    /// `cells_keep_out_of`: the top left corner to leave alone (the parameter
    /// panel draws itself). A change of corner redraws everything.
    pub fn keep_out_of(&mut self, cols: i32, rows: i32) {
        if cols != self.keep_cols || rows != self.keep_rows {
            self.draw_everything = true;
        }
        self.keep_cols = cols;
        self.keep_rows = rows;
    }

    /// `cells_invalidate`: something else touched the screen.
    pub fn invalidate(&mut self) {
        self.draw_everything = true;
    }

    /// `cells_read`: reads the canvas into the current grid. The canvas is
    /// `cols * cell_width` by `rows * cell_height` pixels or larger, with alpha
    /// marking ink: transparent is sky. Pixels off the canvas are not counted.
    /// An empty canvas or an unsized grid reads nothing.
    pub fn read(&mut self, style: CellsStyle, canvas: &Image, cell_width: i32, cell_height: i32) {
        self.read_inner(style, canvas, cell_width, cell_height, None);
    }

    /// As `read`, with conservative sprite coverage supplied by composition.
    pub fn read_occupied(
        &mut self,
        style: CellsStyle,
        canvas: &Image,
        cell_width: i32,
        cell_height: i32,
        occupied: &[bool],
    ) {
        // Tiny cells sample beyond their nominal bounds when a dot rounds to
        // zero pixels. Keep the general reader for those unusual dimensions.
        let occupied = (cell_width >= 2 && cell_height >= 4 && occupied.len() == self.now.len())
            .then_some(occupied);
        self.read_inner(style, canvas, cell_width, cell_height, occupied);
    }

    fn read_inner(
        &mut self,
        style: CellsStyle,
        canvas: &Image,
        cell_width: i32,
        cell_height: i32,
        occupied: Option<&[bool]>,
    ) {
        if canvas.is_empty() || self.now.is_empty() {
            return;
        }
        let cell_width = cell_width.max(1);
        let cell_height = cell_height.max(1);
        for row in 0..self.rows {
            for col in 0..self.cols {
                let at = row as usize * self.cols as usize + col as usize;
                if col < self.keep_cols && row < self.keep_rows
                    || occupied.is_some_and(|mask| !mask[at])
                {
                    self.now[at] = Cell::default();
                    continue;
                }
                let (x0, y0) = (col * cell_width, row * cell_height);
                self.now[at] = match style {
                    CellsStyle::Braille => {
                        read_braille_cell(canvas, x0, y0, cell_width, cell_height)
                    }
                    CellsStyle::Sextants => {
                        read_sextant_cell(canvas, x0, y0, cell_width, cell_height)
                    }
                    CellsStyle::Blocks => read_block_cell(canvas, x0, y0, cell_width, cell_height),
                };
            }
        }
    }

    /// `cells_paint`: the last emitted grid painted the way a terminal shows
    /// it — dots or half blocks in their colours on `ground` — `cell_width` by
    /// `cell_height` pixels a cell, so that a snapshot of a text terminal is a
    /// picture of what was on it and not of the pixels it was read from.
    ///
    /// The C writes into a caller's `png_image_t` with `png_image_alloc`, which
    /// overwrites the pointer without freeing it (every caller passes an empty
    /// image), leaves it untouched on an argument or size refusal, and nulls
    /// its pixels when the allocation itself fails. Here the picture is
    /// returned instead, and the caller's image is never touched. An unsized
    /// grid or a cell under 2 by 4 is an argument error; any refusal of the
    /// image (including a size the C `int` product could not hold, where the C
    /// is undefined) is out of memory, as in the C.
    pub fn paint(
        &self,
        style: CellsStyle,
        cell_width: i32,
        cell_height: i32,
        ground: [u8; 3],
    ) -> Result<Image, CellsError> {
        if self.before.is_empty() {
            return Err(CellsError::Argument);
        }
        if cell_width < 2 || cell_height < 4 {
            return Err(CellsError::Argument);
        }
        let (Some(width), Some(height)) =
            (self.cols.checked_mul(cell_width), self.rows.checked_mul(cell_height))
        else {
            return Err(CellsError::Memory);
        };
        let mut out = Image::alloc(width, height).map_err(|_| CellsError::Memory)?;
        let (out_width, out_height) = (out.width, out.height);
        paint_rect(&mut out, 0, 0, out_width, out_height, ground);

        let (dot_w, dot_h) = (cell_width / 2, cell_height / 4);
        // A dot is drawn a pixel in from its patch on every side, which is
        // roughly how a font draws one: round, with air between it and its
        // neighbours.
        let gap_w = if dot_w > 2 { 1 } else { 0 };
        let gap_h = if dot_h > 2 { 1 } else { 0 };
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = &self.before[row as usize * self.cols as usize + col as usize];
                if cell.glyph == 0 {
                    continue;
                }
                let (x0, y0) = (col * cell_width, row * cell_height);
                match style {
                    CellsStyle::Sextants => {
                        // Back from the code point to the pattern, the way it
                        // was made.
                        let mut blocks = 0u32;
                        for candidate in 1..0x40u32 {
                            if sextant(candidate) == cell.glyph {
                                blocks = candidate;
                            }
                        }
                        let third = cell_height / 3;
                        for r in 0..3 {
                            for c in 0..2 {
                                if blocks & (1u32 << (c + r * 2)) != 0 {
                                    let h = if r == 2 { cell_height - 2 * third } else { third };
                                    paint_rect(
                                        &mut out,
                                        x0 + c * dot_w,
                                        y0 + r * third,
                                        dot_w,
                                        h,
                                        cell.fg,
                                    );
                                }
                            }
                        }
                    }
                    CellsStyle::Braille => {
                        // Unsigned in the C: a glyph below U+2800 wraps.
                        let bits = cell.glyph.wrapping_sub(0x2800);
                        for (r, row_bits) in BRAILLE_BIT.iter().enumerate() {
                            for (c, bit) in row_bits.iter().enumerate() {
                                if bits & bit != 0 {
                                    let (r, c) = (r as i32, c as i32);
                                    paint_rect(
                                        &mut out,
                                        x0 + c * dot_w + gap_w,
                                        y0 + r * dot_h + gap_h,
                                        dot_w - 2 * gap_w,
                                        dot_h - 2 * gap_h,
                                        cell.fg,
                                    );
                                }
                            }
                        }
                    }
                    CellsStyle::Blocks => {
                        let half = cell_height / 2;
                        if cell.glyph == 0x2580 {
                            paint_rect(&mut out, x0, y0, cell_width, half, cell.fg);
                            if cell.has_bg {
                                paint_rect(
                                    &mut out,
                                    x0,
                                    y0 + half,
                                    cell_width,
                                    cell_height - half,
                                    cell.bg,
                                );
                            }
                        } else {
                            paint_rect(
                                &mut out,
                                x0,
                                y0 + half,
                                cell_width,
                                cell_height - half,
                                cell.fg,
                            );
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Grows the text buffer as the C `reserve` does: from 8192, doubling.
    fn reserve(&mut self, extra: usize) -> Result<(), CellsError> {
        let needed = self.text.len() + extra + 1;
        if needed <= self.capacity {
            return Ok(());
        }
        let mut capacity = if self.capacity != 0 { self.capacity } else { 8192 };
        while capacity < needed {
            // The C doubling would wrap and spin forever; no grid gets here.
            capacity = capacity.checked_mul(2).ok_or(CellsError::Memory)?;
        }
        self.text.try_reserve_exact(capacity - self.text.len()).map_err(|_| CellsError::Memory)?;
        self.capacity = capacity;
        Ok(())
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), CellsError> {
        self.reserve(bytes.len())?;
        self.text.extend_from_slice(bytes);
        Ok(())
    }

    fn put_glyph(&mut self, glyph: u32) -> Result<(), CellsError> {
        let glyph = if glyph == 0 { u32::from(b' ') } else { glyph };
        // Byte by byte as the C does it, `(char)` truncation included, so a
        // value no UTF-8 allows still comes out as the C would print it.
        let mut utf8 = [0u8; 4];
        let length = if glyph < 0x80 {
            utf8[0] = glyph as u8;
            1
        } else if glyph < 0x800 {
            utf8[0] = (0xC0 | (glyph >> 6)) as u8;
            utf8[1] = (0x80 | (glyph & 0x3F)) as u8;
            2
        } else if glyph < 0x10000 {
            utf8[0] = (0xE0 | (glyph >> 12)) as u8;
            utf8[1] = (0x80 | ((glyph >> 6) & 0x3F)) as u8;
            utf8[2] = (0x80 | (glyph & 0x3F)) as u8;
            3
        } else {
            utf8[0] = (0xF0 | (glyph >> 18)) as u8;
            utf8[1] = (0x80 | ((glyph >> 12) & 0x3F)) as u8;
            utf8[2] = (0x80 | ((glyph >> 6) & 0x3F)) as u8;
            utf8[3] = (0x80 | (glyph & 0x3F)) as u8;
            4
        };
        self.put(&utf8[..length])
    }

    fn set_colour(&mut self, background: bool, rgb: [u8; 3]) -> Result<(), CellsError> {
        let which = if background { 48 } else { 38 };
        let mut scratch = Scratch::new();
        if self.truecolor {
            scratch.text(b"\x1b[").int(which).text(b";2;").int(i32::from(rgb[0]));
            scratch.text(b";").int(i32::from(rgb[1])).text(b";").int(i32::from(rgb[2])).text(b"m");
        } else {
            scratch.text(b"\x1b[").int(which).text(b";5;").int(cube_index(rgb)).text(b"m");
        }
        self.put(scratch.as_bytes())
    }

    /// Brings the terminal's pen to what the cell wants, emitting as little as
    /// it can: a reset only when something has to go back to default, a colour
    /// only when it differs from the one already set.
    fn dress(&mut self, pen: &mut Pen, cell: &Cell) -> Result<(), CellsError> {
        let mut status = Ok(());
        let drop_fg = pen.has_fg && !cell.has_fg;
        let drop_bg = pen.has_bg && !cell.has_bg;
        if drop_fg || drop_bg {
            status = self.put(b"\x1b[0m");
            pen.has_fg = false;
            pen.has_bg = false;
        }
        if status.is_ok() && cell.has_fg && (!pen.has_fg || pen.fg != cell.fg) {
            status = self.set_colour(false, cell.fg);
            pen.fg = cell.fg;
            pen.has_fg = true;
        }
        if status.is_ok() && cell.has_bg && (!pen.has_bg || pen.bg != cell.bg) {
            status = self.set_colour(true, cell.bg);
            pen.bg = cell.bg;
            pen.has_bg = true;
        }
        status
    }

    /// `cells_emit`: produces the escape text that turns the previous frame
    /// into this one, into `text`, and makes this frame the previous one. On
    /// failure `text` holds what was produced before it and nothing is swapped.
    pub fn emit(&mut self) -> Result<(), CellsError> {
        if self.now.is_empty() {
            return Err(CellsError::Argument);
        }
        self.text.clear();

        let mut pen = Pen::default();
        let mut status = Ok(());
        // Only ever compared with zero; wide enough that no grid overflows it.
        let mut emitted: usize = 0;
        let mut row = 0;
        while row < self.rows && status.is_ok() {
            // Where the terminal's cursor is on this row, if known.
            let mut cursor_col = -1;
            let mut col = 0;
            while col < self.cols && status.is_ok() {
                let at = row as usize * self.cols as usize + col as usize;
                let cell = self.now[at];
                if col < self.keep_cols && row < self.keep_rows {
                    cursor_col = -1;
                    col += 1;
                    continue;
                }
                if !self.draw_everything && same_cell(&cell, &self.before[at]) {
                    col += 1;
                    continue;
                }
                // Position only when the cursor is not already here: a run of
                // changed cells costs one move, not one per cell — and a short
                // hop along the same row is a cursor forward, four bytes,
                // rather than an absolute move at eight or nine.
                let mut scratch = Scratch::new();
                if cursor_col >= 0 && col > cursor_col && col - cursor_col < 100 {
                    scratch.text(b"\x1b[").int(col - cursor_col).text(b"C");
                    status = self.put(scratch.as_bytes());
                } else if cursor_col != col {
                    scratch.text(b"\x1b[").int(row + 1).text(b";").int(col + 1).text(b"H");
                    status = self.put(scratch.as_bytes());
                }
                if status.is_ok() {
                    status = self.dress(&mut pen, &cell);
                }
                if status.is_ok() {
                    status = self.put_glyph(cell.glyph);
                }
                cursor_col = col + 1;
                emitted += 1;
                col += 1;
            }
            row += 1;
        }
        if status.is_ok() && (pen.has_fg || pen.has_bg || emitted > 0) {
            status = self.put(b"\x1b[0m");
        }
        status?;

        // This frame is now the one on the screen.
        std::mem::swap(&mut self.before, &mut self.now);
        self.draw_everything = false;
        Ok(())
    }
}
