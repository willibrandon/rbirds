//! Colour: the palettes, the terminal's own theme turned into a ramp, and the
//! hawk's colour chosen against the flock's. Translated from cbirds `boids.c`
//! (`PALETTES`, `learn_the_theme` and its helpers, `hawk_colour`).

#![forbid(unsafe_code)]

use crate::fp::mul_add;
use crate::image::png::TintMode;

pub type Rgb = [u8; 3];

/// A palette: a list of tints applied to the one sprite. The theme palette's
/// tints are the ramp learned from the terminal, which is state rather than
/// a constant, so it is `None` here and supplied by the caller.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub help: &'static str,
    pub shades: i32,
    pub tints: Option<&'static [Rgb; 5]>,
    pub mode: TintMode,
}

const EMBER_TINTS: [Rgb; 5] =
    [[255, 214, 138], [255, 176, 66], [247, 122, 41], [224, 74, 39], [173, 44, 51]];
const ICE_TINTS: [Rgb; 5] =
    [[226, 246, 255], [160, 220, 250], [96, 176, 236], [58, 122, 206], [44, 74, 158]];
const ACID_TINTS: [Rgb; 5] =
    [[238, 255, 176], [186, 244, 96], [118, 214, 74], [54, 176, 108], [26, 122, 106]];
const MATRIX_TINTS: [Rgb; 5] =
    [[198, 255, 198], [120, 246, 120], [54, 210, 70], [26, 150, 48], [12, 92, 30]];
const AURORA_TINTS: [Rgb; 5] =
    [[206, 255, 222], [110, 240, 170], [44, 204, 170], [60, 140, 210], [110, 84, 200]];
// A ramp is read in order, not by brightness: prism goes round the rainbow and
// potion is two hues that meet with nothing between them.
const PRISM_TINTS: [Rgb; 5] =
    [[255, 92, 92], [255, 196, 64], [96, 220, 110], [80, 160, 255], [176, 110, 255]];
const POTION_TINTS: [Rgb; 5] =
    [[170, 255, 110], [72, 214, 104], [206, 160, 255], [160, 104, 240], [118, 64, 206]];
const DUSK_TINTS: [Rgb; 5] =
    [[255, 214, 170], [255, 148, 120], [232, 86, 136], [160, 70, 170], [92, 64, 168]];
const ASH_TINTS: [Rgb; 5] =
    [[244, 244, 246], [206, 208, 214], [164, 168, 178], [124, 128, 140], [88, 92, 104]];

/// What the embedded sprite is actually painted.
pub const SPRITE_OWN_COLOUR: Rgb = [237, 28, 36];

pub const PALETTES: [Palette; 10] = [
    Palette {
        name: "theme",
        help: "the terminal's own colours, asked for at startup",
        shades: 5,
        tints: None,
        mode: TintMode::Replace,
    },
    Palette {
        name: "ember",
        help: "embers, pale gold to deep red",
        shades: 5,
        tints: Some(&EMBER_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "ice",
        help: "ice, white through to deep blue",
        shades: 5,
        tints: Some(&ICE_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "acid",
        help: "acid, lime through to teal",
        shades: 5,
        tints: Some(&ACID_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "matrix",
        help: "the green of the film it is named after",
        shades: 5,
        tints: Some(&MATRIX_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "aurora",
        help: "the northern lights, mint through to violet",
        shades: 5,
        tints: Some(&AURORA_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "prism",
        help: "light through a prism, red to violet",
        shades: 5,
        tints: Some(&PRISM_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "potion",
        help: "two potions that will not mix, green and violet",
        shades: 5,
        tints: Some(&POTION_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "dusk",
        help: "the sky at dusk, peach through to indigo",
        shades: 5,
        tints: Some(&DUSK_TINTS),
        mode: TintMode::Replace,
    },
    Palette {
        name: "ash",
        help: "ash, white through to slate grey",
        shades: 5,
        tints: Some(&ASH_TINTS),
        mode: TintMode::Replace,
    },
];
pub const PALETTE_COUNT: i32 = PALETTES.len() as i32;
pub const PALETTE_NAMES: [&str; 10] = {
    let mut names = [""; 10];
    let mut i = 0;
    while i < PALETTES.len() {
        names[i] = PALETTES[i].name;
        i += 1;
    }
    names
};

/// `palette_named`: the table's order is presentation, never load bearing;
/// an unknown name is the first palette.
pub fn palette_named(name: &str) -> i32 {
    PALETTES.iter().position(|p| p.name == name).map_or(0, |i| i as i32)
}

/// `FALLBACK_PALETTE`.
pub fn fallback_palette() -> i32 {
    palette_named("ember")
}

/// The terminal's own colours, as learned at startup (`theme_tints`,
/// `theme_is_known`). Zero until learned, as the C static is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Theme {
    pub tints: [Rgb; 5],
    pub known: bool,
}

impl Theme {
    /// `ramp_between`: five steps from the accent toward the background,
    /// stopping well short of it (a seventh of the way per step).
    pub fn ramp_between(&mut self, from: Rgb, to: Rgb) {
        for i in 0..5 {
            for c in 0..3 {
                let from_c = i32::from(from[c]);
                let to_c = i32::from(to[c]);
                self.tints[i][c] = (from_c + (to_c - from_c) * i as i32 / 7) as u8;
            }
        }
    }
}

/// The C string a reply is, read by C string functions: up to the first NUL.
fn c_string(bytes: &[u8]) -> &[u8] {
    match bytes.iter().position(|&b| b == 0) {
        Some(end) => &bytes[..end],
        None => bytes,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// `parse_osc_colour`: the first `rgb:RRRR/GGGG/BBBB` in a reply, scaled to
/// bytes. The channels are read by the C library's own `sscanf("%4x/%4x/%4x")`
/// so every oddity of that grammar is the reference's.
pub fn parse_osc_colour(reply: &[u8]) -> Option<Rgb> {
    let reply = c_string(reply);
    let at = find(reply, b"rgb:")?;
    let rest = &reply[at + 4..];
    let (r, g, b) = crate::platform::scan_osc_rgb(rest)?;
    // Four hex digits a channel is the usual answer; some terminals send two.
    let digits = rest.iter().position(|&c| c == b'/').map_or(4, |slash| slash as i32);
    let shift = if digits >= 4 { 8 } else { 0 };
    Some([(r >> shift) as u8, (g >> shift) as u8, (b >> shift) as u8])
}

/// `saturation_of`.
pub fn saturation_of(rgb: Rgb) -> i32 {
    let (r, g, b) = (i32::from(rgb[0]), i32::from(rgb[1]), i32::from(rgb[2]));
    let mut high = if r > g { r } else { g };
    let mut low = if r < g { r } else { g };
    if b > high {
        high = b;
    }
    if b < low {
        low = b;
    }
    high - low
}

/// `luminance_of`: the relative luminance the WCAG contrast ratio uses.
pub fn luminance_of(rgb: Rgb) -> f64 {
    let mut channel = [0.0_f64; 3];
    for c in 0..3 {
        let v = f64::from(rgb[c]) / 255.0;
        channel[c] = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    }
    // fma: boids.c:704:32
    // fma: boids.c:704:54
    mul_add(0.0722, channel[2], mul_add(0.2126, channel[0], 0.7152 * channel[1]))
}

/// `contrast_between`.
pub fn contrast_between(a: Rgb, b: Rgb) -> f64 {
    let mut high = luminance_of(a);
    let mut low = luminance_of(b);
    if high < low {
        std::mem::swap(&mut high, &mut low);
    }
    (high + 0.05) / (low + 0.05)
}

/// The accent chosen from palette entries one to six, by saturation times
/// contrast against the background (`learn_the_theme` minus the queries).
/// `answers[k]` is entry `k + 1`'s colour, `None` for no answer. Returns
/// whether any entry answered, having built the ramp if so.
pub fn learn_from_answers(theme: &mut Theme, background: Rgb, answers: &[Option<Rgb>; 6]) -> bool {
    let mut accent: Rgb = [0, 0, 0];
    let mut best = -1.0_f64;
    for rgb in answers.iter().flatten() {
        let score = f64::from(saturation_of(*rgb)) * contrast_between(*rgb, background);
        if score > best {
            best = score;
            accent = *rgb;
        }
    }
    if best < 0.0 {
        return false;
    }
    theme.ramp_between(accent, background);
    theme.known = true;
    true
}

/// Candidate hawk colours: scarlet, a hot near-white, an electric cyan.
pub const HAWK_COLOURS: [Rgb; 3] = [[255, 60, 72], [255, 246, 210], [96, 226, 255]];

/// `colour_distance`: the usual weighted RGB distance.
pub fn colour_distance(a: Rgb, b: Rgb) -> f64 {
    let mean_red = (f64::from(a[0]) + f64::from(b[0])) / 2.0;
    let dr = f64::from(i32::from(a[0]) - i32::from(b[0]));
    let dg = f64::from(i32::from(a[1]) - i32::from(b[1]));
    let db = f64::from(i32::from(a[2]) - i32::from(b[2]));
    // fma: boids.c:883:48
    let red_green = mul_add((2.0 + mean_red / 256.0) * dr, dr, 4.0 * dg * dg);
    // fma: boids.c:883:62
    mul_add((2.0 + (255.0 - mean_red) / 256.0) * db, db, red_green).sqrt()
}

/// `hawk_colour`: whichever candidate stands furthest from the nearest shade
/// the flock is wearing; plain scarlet against artwork of somebody's own.
pub fn hawk_colour(palette: &Palette, tints: Option<&[Rgb; 5]>, custom_sprite: bool) -> Rgb {
    if custom_sprite {
        return HAWK_COLOURS[0];
    }
    let mut best = 0;
    let mut best_gap = -1.0_f64;
    for (candidate, colour) in HAWK_COLOURS.iter().enumerate() {
        let mut gap = 1e9_f64;
        for shade in 0..palette.shades as usize {
            let tint = match tints {
                Some(tints) => tints[shade],
                None => SPRITE_OWN_COLOUR,
            };
            let against = colour_distance(*colour, tint);
            if against < gap {
                gap = against;
            }
        }
        if gap > best_gap {
            best_gap = gap;
            best = candidate;
        }
    }
    HAWK_COLOURS[best]
}
