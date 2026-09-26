//! The cbirds `tests/cells_test.c` suite, translated test for test: every
//! `test_*` its `main` calls, with the same fixtures, assertions and bounds.

use rbirds::image::Image;
use rbirds::render::cells::{Cell, Cells, CellsStyle, braille, sextant};

/// A canvas of cols*8 by rows*16 transparent pixels, with a paint brush.
fn blank(cols: i32, rows: i32) -> Image {
    Image::alloc(cols * 8, rows * 16).expect("canvas")
}

#[allow(clippy::too_many_arguments)]
fn paint(canvas: &mut Image, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    for yy in y..y + h {
        for xx in x..x + w {
            let at = (yy as usize * canvas.width as usize + xx as usize) * 4;
            canvas.pixels[at..at + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

/// After an emit the frame just read is in `before`.
fn at(cells: &Cells, row: i32, col: i32) -> &Cell {
    &cells.before[row as usize * cells.cols as usize + col as usize]
}

/// `strstr(haystack, needle) != NULL`; the emitted text holds no NUL.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// The C's `for (p = text; (p = strstr(p, needle)) != NULL; p++) n++`, which
/// counts overlapping occurrences.
fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|window| *window == needle).count()
}

fn clear(canvas: &mut Image) {
    canvas.pixels.fill(0);
}

#[test]
fn test_braille_dot_numbering() {
    // Braille numbers its dots 1 2 3 down the left, 4 5 6 down the right, then
    // 7 and 8 along the bottom; U+2800 plus the bits.
    assert_eq!(braille(0), 0x2800);
    assert_eq!(braille(1 << 0), 0x2801); // Top left: dot 1.
    assert_eq!(braille(1 << 1), 0x2808); // Top right: dot 4.
    assert_eq!(braille(1 << 2), 0x2802); // Second row left: dot 2.
    assert_eq!(braille(1 << 6), 0x2840); // Bottom left: dot 7.
    assert_eq!(braille(1 << 7), 0x2880); // Bottom right: dot 8.
    assert_eq!(braille(0xFF), 0x28FF); // Every dot.
}

#[test]
fn test_ink_becomes_dots_in_the_ink_colour() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(4, 2), Ok(()));
    let mut canvas = blank(4, 2);

    // A red square filling the top-left dot of cell (row 1, col 2) exactly: a
    // dot is 4 by 4 pixels of an 8 by 16 cell.
    paint(&mut canvas, 2 * 8, 16, 4, 4, 255, 0, 0);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));

    let cell = at(&cells, 1, 2);
    assert_eq!(cell.glyph, 0x2801);
    assert!(cell.has_fg && cell.fg[0] == 255 && cell.fg[1] == 0 && cell.fg[2] == 0);
    assert!(!cell.has_bg);
    // And the other cells are empty.
    assert_eq!(at(&cells, 0, 0).glyph, 0);
    assert_eq!(at(&cells, 1, 3).glyph, 0);

    // The text that was emitted positions each row once — a first frame draws
    // every cell, blanks as spaces, so the cursor runs along the row on its
    // own — sets the colour and prints the glyph.
    assert!(contains(&cells.text, b"\x1b[2;1H"));
    assert!(!contains(&cells.text, b"\x1b[2;3H"));
    assert!(contains(&cells.text, b"\x1b[38;2;255;0;0m"));
    assert!(contains(&cells.text, b"\xe2\xa0\x81")); // U+2801 in UTF-8.
    assert!(contains(&cells.text, b"\x1b[0m"));
}

#[test]
fn test_a_faint_fringe_is_not_a_dot() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(1, 1), Ok(()));
    let mut canvas = blank(1, 1);
    // One pixel of sixteen in the dot's patch: anti-aliasing, not a bird.
    paint(&mut canvas, 0, 0, 1, 1, 255, 255, 255);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(at(&cells, 0, 0).glyph, 0);
    // Half of it, and it is.
    paint(&mut canvas, 0, 0, 4, 2, 255, 255, 255);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(at(&cells, 0, 0).glyph, 0x2801);
}

#[test]
fn test_only_what_changed_is_emitted() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(40, 10), Ok(()));
    let mut canvas = blank(40, 10);
    paint(&mut canvas, 0, 0, 8, 16, 0, 255, 0);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    let first = cells.text.len();

    // The same frame again: nothing to say.
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(cells.text.len(), 0);

    // The bird moves one cell right: the old cell is blanked, the new one
    // drawn, and that is all — a fraction of the first frame.
    clear(&mut canvas);
    paint(&mut canvas, 8, 0, 8, 16, 0, 255, 0);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert!(!cells.text.is_empty() && cells.text.len() < first / 4);
    assert!(contains(&cells.text, b"\x1b[1;1H")); // The blanked cell.
    assert!(contains(&cells.text, b"\x1b[38;2;0;255;0m"));
    // Two adjacent cells changed, so the cursor was positioned once.
    assert!(!contains(&cells.text, b"\x1b[1;2H"));

    // Told the screen was touched, it redraws everything again.
    cells.invalidate();
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert!(cells.text.len() >= first / 2);
}

#[test]
fn test_the_corner_left_to_the_panel_is_never_written() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(6, 4), Ok(()));
    cells.keep_out_of(3, 2);
    let mut canvas = blank(6, 4);
    // Ink everywhere.
    paint(&mut canvas, 0, 0, 6 * 8, 4 * 16, 200, 200, 200);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    for row in 0..2 {
        for col in 0..3 {
            assert_eq!(at(&cells, row, col).glyph, 0);
        }
    }
    assert_eq!(at(&cells, 0, 3).glyph, 0x28FF);
    assert_eq!(at(&cells, 2, 0).glyph, 0x28FF);
    // And no cursor move lands inside the corner.
    assert!(!contains(&cells.text, b"\x1b[1;1H"));
    assert!(!contains(&cells.text, b"\x1b[2;2H"));
    assert!(contains(&cells.text, b"\x1b[1;4H"));
}

#[test]
fn test_half_blocks_never_paint_the_sky() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(3, 1), Ok(()));
    let mut canvas = blank(3, 1);
    paint(&mut canvas, 0, 0, 8, 8, 255, 0, 0); // Cell 0: top half only.
    paint(&mut canvas, 8, 8, 8, 8, 0, 0, 255); // Cell 1: bottom half only.
    paint(&mut canvas, 16, 0, 8, 8, 255, 0, 0); // Cell 2: both.
    paint(&mut canvas, 16, 8, 8, 8, 0, 0, 255);
    cells.read(CellsStyle::Blocks, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));

    // Top only: the upper half block in the top's colour, background
    // untouched.
    assert!(
        at(&cells, 0, 0).glyph == 0x2580 && at(&cells, 0, 0).has_fg && !at(&cells, 0, 0).has_bg
    );
    assert_eq!(at(&cells, 0, 0).fg[0], 255);
    // Bottom only: the lower half block, foreground, background untouched.
    assert!(at(&cells, 0, 1).glyph == 0x2584 && !at(&cells, 0, 1).has_bg);
    assert_eq!(at(&cells, 0, 1).fg[2], 255);
    // Both: upper block, top colour in front, bottom colour behind.
    assert!(at(&cells, 0, 2).glyph == 0x2580 && at(&cells, 0, 2).has_bg);
    assert!(at(&cells, 0, 2).fg[0] == 255 && at(&cells, 0, 2).bg[2] == 255);
    assert!(contains(&cells.text, b"\x1b[48;2;0;0;255m"));
}

/// A hop along the row the cursor is already on is a cursor forward, not a
/// fresh absolute position: half the bytes, over a frame of scattered changes.
#[test]
fn test_a_hop_along_the_row_is_a_cursor_forward() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(12, 2), Ok(()));
    let mut canvas = blank(12, 2);
    paint(&mut canvas, 0, 0, 8, 16, 9, 9, 9);
    paint(&mut canvas, 5 * 8, 0, 8, 16, 9, 9, 9);
    paint(&mut canvas, 3 * 8, 16, 8, 16, 9, 9, 9);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(())); // Everything, first time.
    // Now move the two birds on the top row one cell right each.
    clear(&mut canvas);
    paint(&mut canvas, 8, 0, 8, 16, 9, 9, 9);
    paint(&mut canvas, 6 * 8, 0, 8, 16, 9, 9, 9);
    paint(&mut canvas, 3 * 8, 16, 8, 16, 9, 9, 9);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    // Row one: an absolute move to the start, then cells 0 and 1 in a run,
    // then a cursor forward of three to cell 5, then cells 5 and 6. Row two:
    // nothing.
    assert!(contains(&cells.text, b"\x1b[1;1H"));
    assert!(contains(&cells.text, b"\x1b[3C"));
    assert!(!contains(&cells.text, b"\x1b[1;6H"));
    assert!(!contains(&cells.text, b"\x1b[2;"));
}

#[test]
fn test_the_pen_is_not_reset_between_cells_of_one_colour() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(8, 1), Ok(()));
    let mut canvas = blank(8, 1);
    paint(&mut canvas, 0, 0, 8 * 8, 16, 10, 20, 30);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    // Eight cells, one colour: the SGR appears once and the move once.
    let colours = occurrences(&cells.text, b"\x1b[38;2;");
    let moves = occurrences(&cells.text, b"H");
    assert_eq!(colours, 1);
    assert_eq!(moves, 1);
}

#[test]
fn test_without_truecolor_the_cube_is_used() {
    let mut cells = Cells::new(false).unwrap();
    assert_eq!(cells.resize(1, 1), Ok(()));
    let mut canvas = blank(1, 1);
    paint(&mut canvas, 0, 0, 8, 16, 255, 0, 0);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert!(contains(&cells.text, b"\x1b[38;5;196m")); // Pure red in the cube.
    assert!(!contains(&cells.text, b";2;"));
}

#[test]
fn test_a_resize_redraws_everything() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(4, 1), Ok(()));
    let canvas = blank(4, 1);
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(cells.resize(4, 1), Ok(())); // Same size: nothing changes.
    assert!(!cells.draw_everything);
    assert_eq!(cells.resize(5, 2), Ok(()));
    assert!(cells.draw_everything);
    assert!(cells.cols == 5 && cells.rows == 2);
}

#[test]
fn test_painting_shows_what_the_terminal_showed() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(2, 1), Ok(()));
    let mut canvas = blank(2, 1);
    paint(&mut canvas, 0, 0, 4, 4, 255, 0, 0); // Top left dot of cell 0, red.
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));

    let ground = [1, 2, 3];
    let picture = cells.paint(CellsStyle::Braille, 8, 16, ground).expect("paint");
    assert!(picture.width == 16 && picture.height == 16);
    // The dot's patch is red inside its one pixel margin, the ground elsewhere.
    let inside = &picture.pixels[(16 + 1) * 4..];
    let corner = &picture.pixels[0..];
    let elsewhere = &picture.pixels[(10 * 16 + 12) * 4..];
    assert!(inside[0] == 255 && inside[1] == 0 && inside[2] == 0);
    assert!(corner[0] == 1 && corner[1] == 2 && corner[2] == 3);
    assert!(elsewhere[0] == 1 && elsewhere[1] == 2 && elsewhere[2] == 3);

    // Blocks: the upper half block fills the top half of the cell.
    clear(&mut canvas);
    paint(&mut canvas, 8, 0, 8, 8, 0, 255, 0);
    cells.read(CellsStyle::Blocks, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    let picture = cells.paint(CellsStyle::Blocks, 8, 16, ground).expect("paint");
    let top = &picture.pixels[(3 * 16 + 12) * 4..];
    let bottom = &picture.pixels[(12 * 16 + 12) * 4..];
    assert_eq!(top[1], 255);
    assert_eq!(bottom[1], 2); // Ground: the sky is never painted.
}

/// Two birds in one cell: the cell wears the colour of the one that fills more
/// of it, not a blend that is neither and different in every cell.
#[test]
fn test_a_shared_cell_wears_the_bigger_bird() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(1, 1), Ok(()));
    let mut canvas = blank(1, 1);
    paint(&mut canvas, 0, 0, 8, 10, 200, 0, 0); // Red, ten rows of sixteen.
    paint(&mut canvas, 0, 10, 8, 6, 0, 0, 200); // Blue, six.
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    let cell = &cells.before[0];
    assert!(cell.fg[0] == 200 && cell.fg[1] == 0 && cell.fg[2] == 0);
    // Weighted by how much of each pixel is ink, not by pixels.
    for y in 0..10 {
        for x in 0..8 {
            canvas.pixels[(y * 8 + x) * 4 + 3] = 60; // Faint red.
        }
    }
    cells.read(CellsStyle::Braille, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(cells.before[0].fg[2], 200); // Six solid rows of blue outweigh ten faint of red.
}

/// Sextants: two by three solid blocks a cell, laid out by Unicode 13 in order
/// of their pattern with the four that already existed left out.
#[test]
fn test_sextant_code_points() {
    assert_eq!(sextant(0), u32::from(b' '));
    assert_eq!(sextant(0x01), 0x1FB00); // Top left alone: SEXTANT-1.
    assert_eq!(sextant(0x02), 0x1FB01); // SEXTANT-2.
    assert_eq!(sextant(0x03), 0x1FB02); // SEXTANT-12.
    assert_eq!(sextant(0x14), 0x1FB13); // SEXTANT-35, the last before the left half.
    assert_eq!(sextant(0x15), 0x258C); // SEXTANT-135 is the left half block.
    assert_eq!(sextant(0x16), 0x1FB14); // And the count skips it.
    assert_eq!(sextant(0x2A), 0x2590); // SEXTANT-246 is the right half block.
    assert_eq!(sextant(0x2B), 0x1FB28); // Skipping both.
    assert_eq!(sextant(0x3E), 0x1FB3B); // SEXTANT-23456, the last in the block.
    assert_eq!(sextant(0x3F), 0x2588); // Everything is the full block.
    // Every pattern gets its own character, and the block is exactly filled.
    let mut seen = [0; 0x40];
    for p in 1..0x3Fu32 {
        let code = sextant(p);
        if (0x1FB00..=0x1FB3B).contains(&code) {
            seen[(code - 0x1FB00) as usize] += 1;
        }
    }
    for count in &seen[..=0x3B] {
        assert_eq!(*count, 1);
    }
}

#[test]
fn test_ink_becomes_sextants_too() {
    let mut cells = Cells::new(true).unwrap();
    assert_eq!(cells.resize(2, 1), Ok(()));
    let mut canvas = blank(2, 1);
    // The top third of cell 1, both columns: rows of 16 split 5/5/6.
    paint(&mut canvas, 8, 0, 8, 5, 0, 200, 0);
    cells.read(CellsStyle::Sextants, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(cells.before[1].glyph, 0x1FB02); // SEXTANT-12.
    assert_eq!(cells.before[1].fg[1], 200);
    assert_eq!(cells.before[0].glyph, 0);
    // The whole cell is the full block, which is not in the sextant block.
    paint(&mut canvas, 8, 0, 8, 16, 0, 200, 0);
    cells.read(CellsStyle::Sextants, &canvas, 8, 16);
    assert_eq!(cells.emit(), Ok(()));
    assert_eq!(cells.before[1].glyph, 0x2588);
    // And a painting of it fills the cell.
    let ground = [1, 2, 3];
    let picture = cells.paint(CellsStyle::Sextants, 8, 16, ground).expect("paint");
    assert_eq!(picture.pixels[(15 * 16 + 15) * 4 + 1], 200);
    assert_eq!(picture.pixels[(15 * 16 + 2) * 4 + 1], 2);
}
