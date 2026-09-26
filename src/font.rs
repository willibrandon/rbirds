//! A 5 by 7 bitmap font, just wide enough for a flock to spell with,
//! translated from cbirds `font.c` glyph for glyph.
//!
//! Glyphs are seven rows of five characters, `#` where a bird goes. Lower
//! case maps to upper case: at five pixels wide there is no room for two.

#![forbid(unsafe_code)]

pub const FONT_WIDTH: i32 = 5;
pub const FONT_HEIGHT: i32 = 7;
pub const FONT_ADVANCE: i32 = FONT_WIDTH + 1;

macro_rules! glyph {
    ($c:expr, $r0:literal $r1:literal $r2:literal $r3:literal $r4:literal $r5:literal $r6:literal) => {
        ($c, concat!($r0, $r1, $r2, $r3, $r4, $r5, $r6))
    };
}

#[rustfmt::skip]
static GLYPHS: [(u8, &str); 57] = [
    glyph!(b' ', "....." "....." "....." "....." "....." "....." "....."),
    glyph!(b'A', ".###." "#...#" "#...#" "#####" "#...#" "#...#" "#...#"),
    glyph!(b'B', "####." "#...#" "#...#" "####." "#...#" "#...#" "####."),
    glyph!(b'C', ".###." "#...#" "#...." "#...." "#...." "#...#" ".###."),
    glyph!(b'D', "####." "#...#" "#...#" "#...#" "#...#" "#...#" "####."),
    glyph!(b'E', "#####" "#...." "#...." "####." "#...." "#...." "#####"),
    glyph!(b'F', "#####" "#...." "#...." "####." "#...." "#...." "#...."),
    glyph!(b'G', ".###." "#...#" "#...." "#..##" "#...#" "#...#" ".###."),
    glyph!(b'H', "#...#" "#...#" "#...#" "#####" "#...#" "#...#" "#...#"),
    glyph!(b'I', "#####" "..#.." "..#.." "..#.." "..#.." "..#.." "#####"),
    glyph!(b'J', "..###" "...#." "...#." "...#." "...#." "#..#." ".##.."),
    glyph!(b'K', "#...#" "#..#." "#.#.." "##..." "#.#.." "#..#." "#...#"),
    glyph!(b'L', "#...." "#...." "#...." "#...." "#...." "#...." "#####"),
    glyph!(b'M', "#...#" "##.##" "#.#.#" "#...#" "#...#" "#...#" "#...#"),
    glyph!(b'N', "#...#" "##..#" "#.#.#" "#..##" "#...#" "#...#" "#...#"),
    glyph!(b'O', ".###." "#...#" "#...#" "#...#" "#...#" "#...#" ".###."),
    glyph!(b'P', "####." "#...#" "#...#" "####." "#...." "#...." "#...."),
    glyph!(b'Q', ".###." "#...#" "#...#" "#...#" "#.#.#" "#..#." ".##.#"),
    glyph!(b'R', "####." "#...#" "#...#" "####." "#.#.." "#..#." "#...#"),
    glyph!(b'S', ".####" "#...." "#...." ".###." "....#" "....#" "####."),
    glyph!(b'T', "#####" "..#.." "..#.." "..#.." "..#.." "..#.." "..#.."),
    glyph!(b'U', "#...#" "#...#" "#...#" "#...#" "#...#" "#...#" ".###."),
    glyph!(b'V', "#...#" "#...#" "#...#" "#...#" "#...#" ".#.#." "..#.."),
    glyph!(b'W', "#...#" "#...#" "#...#" "#.#.#" "#.#.#" "##.##" "#...#"),
    glyph!(b'X', "#...#" "#...#" ".#.#." "..#.." ".#.#." "#...#" "#...#"),
    glyph!(b'Y', "#...#" "#...#" ".#.#." "..#.." "..#.." "..#.." "..#.."),
    glyph!(b'Z', "#####" "....#" "...#." "..#.." ".#..." "#...." "#####"),
    glyph!(b'0', ".###." "#...#" "#..##" "#.#.#" "##..#" "#...#" ".###."),
    glyph!(b'1', "..#.." ".##.." "..#.." "..#.." "..#.." "..#.." ".###."),
    glyph!(b'2', ".###." "#...#" "....#" "...#." "..#.." ".#..." "#####"),
    glyph!(b'3', "#####" "...#." "..#.." "...#." "....#" "#...#" ".###."),
    glyph!(b'4', "...#." "..##." ".#.#." "#..#." "#####" "...#." "...#."),
    glyph!(b'5', "#####" "#...." "####." "....#" "....#" "#...#" ".###."),
    glyph!(b'6', "..##." ".#..." "#...." "####." "#...#" "#...#" ".###."),
    glyph!(b'7', "#####" "....#" "...#." "..#.." ".#..." ".#..." ".#..."),
    glyph!(b'8', ".###." "#...#" "#...#" ".###." "#...#" "#...#" ".###."),
    glyph!(b'9', ".###." "#...#" "#...#" ".####" "....#" "...#." ".##.."),
    glyph!(b'.', "....." "....." "....." "....." "....." ".##.." ".##.."),
    glyph!(b',', "....." "....." "....." "....." ".##.." ".##.." "..#.."),
    glyph!(b'!', "..#.." "..#.." "..#.." "..#.." "..#.." "....." "..#.."),
    glyph!(b'?', ".###." "#...#" "....#" "...#." "..#.." "....." "..#.."),
    glyph!(b'\'', "..#.." "..#.." "....." "....." "....." "....." "....."),
    glyph!(b'-', "....." "....." "....." "#####" "....." "....." "....."),
    glyph!(b'+', "....." "..#.." "..#.." "#####" "..#.." "..#.." "....."),
    glyph!(b'=', "....." "....." "#####" "....." "#####" "....." "....."),
    glyph!(b':', "....." "..#.." "..#.." "....." "..#.." "..#.." "....."),
    glyph!(b'/', "....#" "....#" "...#." "..#.." ".#..." "#...." "#...."),
    glyph!(b'(', "...#." "..#.." ".#..." ".#..." ".#..." "..#.." "...#."),
    glyph!(b')', ".#..." "..#.." "...#." "...#." "...#." "..#.." ".#..."),
    glyph!(b'*', "....." "#...#" ".#.#." "#####" ".#.#." "#...#" "....."),
    glyph!(b'<', "...#." "..#.." ".#..." "#...." ".#..." "..#.." "...#."),
    glyph!(b'>', ".#..." "..#.." "...#." "....#" "...#." "..#.." ".#..."),
    glyph!(b'#', ".#.#." ".#.#." "#####" ".#.#." "#####" ".#.#." ".#.#."),
    glyph!(b'@', ".###." "#...#" "#.###" "#.#.#" "#.###" "#...." ".###."),
    glyph!(b'%', "#...#" "#..#." "..#.." ".#..." "..#.." ".#..#" "#...#"),
    glyph!(b'&', ".##.." "#..#." "#.#.." ".#..." "#.#.#" "#..#." ".##.#"),
    glyph!(b'_', "....." "....." "....." "....." "....." "....." "#####"),
];

/// `font_glyph`: the 35 cells of a glyph, row major, `#` set; `None` for a
/// character the font does not carry. Only ASCII letters change case, as
/// `toupper` does in the C locale.
pub fn glyph(character: u8) -> Option<&'static [u8]> {
    let wanted = character.to_ascii_uppercase();
    GLYPHS.iter().find(|&&(c, _)| c == wanted).map(|&(_, rows)| rows.as_bytes())
}

/// `font_text_cells`: how many cells the text would set.
pub fn text_cells(text: &[u8]) -> i32 {
    text.iter()
        .filter_map(|&c| glyph(c))
        .map(|rows| rows.iter().filter(|&&cell| cell == b'#').count() as i32)
        .sum()
}

/// `font_text_width`: width in glyph cells of the text, blanks included.
pub fn text_width(text: &[u8]) -> i32 {
    let glyphs = text.iter().filter(|&&c| glyph(c).is_some()).count() as i32;
    if glyphs > 0 { glyphs * FONT_ADVANCE - 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_five_by_seven_and_unique() {
        for (i, &(c, rows)) in GLYPHS.iter().enumerate() {
            assert_eq!(rows.len(), (FONT_WIDTH * FONT_HEIGHT) as usize, "{}", c as char);
            assert!(rows.bytes().all(|b| b == b'#' || b == b'.'));
            assert!(GLYPHS[..i].iter().all(|&(other, _)| other != c));
        }
    }

    #[test]
    fn case_folds_and_unknowns_are_skipped() {
        assert_eq!(glyph(b'b'), glyph(b'B'));
        assert!(glyph(b'~').is_none());
        assert!(glyph(0xe9).is_none());
        assert_eq!(text_width(b"BOIDS"), 29);
        assert_eq!(text_width(b"~"), 0);
        assert_eq!(text_cells(b" "), 0);
        assert_eq!(text_cells(b"I"), 15);
    }
}
