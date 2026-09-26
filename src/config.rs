//! Constants, the simulation's tunables, and the notch arithmetic that derives
//! every tunable from the slider position the keys move. Translated from the
//! constants, `config_t`, `apply_notches` and presets of cbirds `boids.c`.

#![forbid(unsafe_code)]

/// Sixty rotations, six degrees apart.
pub const ROTATION_FRAMES: i32 = 60;
/// A power of two so a rounded angle wraps with one mask.
pub const TRIG_LOOKUP_SIZE: i32 = 4096;
pub const TRIG_LOOKUP_MASK: i32 = TRIG_LOOKUP_SIZE - 1;
/// Wings out, half, folded; beaten in a cycle of four so the flap goes out and back.
pub const WING_PHASES: i32 = 3;
pub const WING_CYCLE: i32 = 4;
/// Near and far.
pub const LAYERS: i32 = 2;
pub const MAX_PALETTE_SHADES: i32 = 8;
pub const MAX_FLOCKS: i32 = 3;
/// Pixels from the pointer within which a bird feels it.
pub const MOUSE_REACH: i32 = 120;
pub const DEFAULT_VISION_NOTCH: i32 = 6;
pub const DEFAULT_TURNING_NOTCH: i32 = 8;
pub const TRAIL_EVERY: i32 = 4;
pub const TRAIL_LENGTH: i32 = 3;
pub const INTRO_SECONDS: i32 = 3;
pub const MAX_HAWKS: i32 = 4;
pub const HAWK_REACH: i32 = 150;
/// Sixty-hertz frames; converted to seconds where used.
pub const HAWK_COMMITMENT_FRAMES: i32 = 40;
/// Pixels past which a reconsidered chase is dropped.
pub const HAWK_GIVE_UP: i32 = 200;
/// And how far off it looks for the next bird.
pub const HAWK_STALK: i32 = 340;
/// Sixty-hertz frames; converted to seconds where used.
pub const HAWK_PASS_FRAMES: i32 = 14;
/// Pixels two hawks try to keep between them.
pub const HAWK_SPACING: i32 = 150;
/// The fastest rate a GIF's hundredth-second delays can really carry.
pub const MAX_RECORD_FPS: i32 = 50;
/// Seconds between one slider moving and the next.
pub const AUTOPILOT_PERIOD: i32 = 4;
pub const OUTRO_FRAMES_AT_SIXTY: i32 = 40;
pub const FRAME_ANGLE: i32 = 360 / ROTATION_FRAMES;
pub const SPRITE_SUPERSAMPLE: i32 = 6;
pub const SPRITE_WORK_MAX: i32 = 256;
pub const MIN_BIRD_SIZE: i32 = 4;
pub const MAX_BIRD_SIZE: i32 = 64;
pub const MAX_BIRDS: i32 = 4096;
pub const INPUT_BUFFER_SIZE: usize = 100;
pub const DEFAULT_COLS: i32 = 80;
pub const DEFAULT_ROWS: i32 = 24;
pub const DEFAULT_CELL_WIDTH: i32 = 8;
pub const DEFAULT_CELL_HEIGHT: i32 = 16;
/// Edge bands as a fraction of the viewport: a third, and half that at the bottom.
pub const TURN_BAND_DIVISOR: i32 = 3;
pub const BOTTOM_BAND_DIVISOR: i32 = 6;
/// Sixty is the rate every per-second quantity is divided by, not a setting.
pub const FRAME_RATE: i32 = 60;
pub const MAX_CAST_FPS: i32 = 120;
pub const DEFAULT_SPEED: i32 = 40;
pub const DEFAULT_BIRD_SIZE: i32 = 30;
pub const SPATIAL_CELL_SIZE: i32 = 12;
pub const MIN_VISION_RADIUS: i32 = 12;
pub const MAX_VISION_RADIUS: i32 = 60;
pub const DEFAULT_VISION_RADIUS: i32 = 36;
pub const MAX_VISION_CELLS: i32 = MAX_VISION_RADIUS / SPATIAL_CELL_SIZE;
/// The parameter panel, anchored to the top left corner, fixed in cells.
pub const LEGEND_COLUMNS: i32 = 38;
pub const LEGEND_ROWS: i32 = 10;
pub const LEGEND_MAX_ROWS: i32 = LEGEND_ROWS + 1;
/// One notch a keypress: the steps every parameter travels through.
pub const LEGEND_BAR_CELLS: i32 = 12;
pub const LEGEND_NAME_WIDTH: i32 = 10;
pub const LEGEND_VALUE_WIDTH: i32 = 5;
pub const LEGEND_MIN_COLS: i32 = 76;
pub const LEGEND_MIN_ROWS: i32 = 22;
pub const LEGEND_LINE_MAX: usize = 128;
pub const SPAWN_ATTEMPTS: i32 = 32;
/// Seconds without a key before the sliders start wandering by themselves.
pub const IDLE_SECONDS: i32 = 60;

/// The same scale as the original bottom edge turn.
pub const LEGEND_PUSH: f64 = 100000.0;
pub const EDGE_FIRM: f64 = 12.0;
pub const ESCAPE_PENALTY: f64 = 8.0;
pub const MOUSE_WEIGHT: f64 = 4.0;
pub const HAWK_WEIGHT: f64 = 3.0;
pub const HAWK_SPEED: f64 = 0.90;
pub const HAWK_TURN: f64 = 1.0;
/// At most the distance a bird covers in three sixty-hertz steps.
pub const HAWK_LEAD_DISTANCE: f64 = DEFAULT_SPEED as f64 * 3.0;
pub const HAWK_DIVE: f64 = 90.0;
pub const WING_HZ: f64 = 6.0;
/// Per beat completed.
pub const GLIDE_CHANCE: f64 = 0.06;
pub const GLIDE_SECONDS_MIN: f64 = 0.4;
pub const GLIDE_SECONDS_MAX: f64 = 1.2;
pub const FAR_SHARE: f64 = 0.35;
pub const FAR_SIZE: f64 = 0.62;
pub const FAR_PACE: f64 = 0.72;
/// How far toward the ground a far bird's tint goes.
pub const FAR_DIM: f64 = 0.40;
pub const WING_SPAN: [f64; WING_PHASES as usize] = [1.0, 0.72, 0.45];
pub const WING_SEQUENCE: [i32; WING_CYCLE as usize] = [0, 1, 2, 1];
pub const TRAIL_ALPHA: [f64; TRAIL_LENGTH as usize] = [0.40, 0.25, 0.12];
pub const TRAIL_SIZE: f64 = 0.85;
pub const HAWK_DIVE_SPEED: f64 = 1.45;
pub const HAWK_SWIRL: f64 = 0.9;
pub const HAWK_APART: f64 = 1.2;
pub const HAWK_WALL: f64 = 2.5;
/// The narrowest a flock's leash gets, for a small flock.
pub const FLOCK_LEASH: i32 = 150;
pub const LEASH_PER_ROOT_BIRD: f64 = 12.0;
pub const LEASH_WEIGHT: f64 = 2.5;
pub const WIND_WEIGHT: f64 = 0.5;

pub const DEFAULT_SEPARATION_W: f64 = 0.005;
pub const DEFAULT_ALIGNMENT_W: f64 = 1.5;
pub const COHESION_W: f64 = 0.01;
pub const DEFAULT_BOUNDARY_W: f64 = 0.2;

pub const BOUNDARY_MIN: f64 = 0.01;
pub const SEPARATION_MIN: f64 = 0.001;
pub const ALIGNMENT_MIN: f64 = 0.1;

/// Each default lands on the fourth of twelve notches.
pub const DEFAULT_NOTCH: i32 = 4;

/// `NOTCH_CEILING(minimum, default)`, evaluated as the C macro expands it.
const fn notch_ceiling(minimum: f64, default_value: f64) -> f64 {
    minimum + 3.0 * (default_value - minimum)
}

pub const BOUNDARY_MAX: f64 = notch_ceiling(0.01, DEFAULT_BOUNDARY_W);
pub const SEPARATION_MAX: f64 = notch_ceiling(0.001, DEFAULT_SEPARATION_W);
pub const ALIGNMENT_MAX: f64 = notch_ceiling(0.1, DEFAULT_ALIGNMENT_W);

pub const DEFAULT_PACE_NOTCH: i32 = 1;
pub const DEFAULT_PACE: f64 = 0.4;
pub const PACE_STEP: f64 = 0.2;

/// At the top notch a stranger in sight weighs what the flock's own heading does.
pub const AVOID_WEIGHT_MAX: f64 = 1.5;

/// `config_t`: the tunables, and the notches every one of them derives from.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub birds: i32,
    pub bird_size: i32,
    pub palette: i32,
    pub flocks: i32,
    pub trails: bool,
    pub hawks: i32,
    pub shape: i32,
    pub turning_notch: i32,
    /// Pixels a bird covers this frame, at the chosen pace.
    pub speed: f64,
    /// The same at pace one, which is what the way out flies at.
    pub base_speed: f64,
    /// The speed slider's factor; one is the shipped flock.
    pub pace: f64,
    pub vision_cells: i32,
    pub vision_radius: i32,
    pub vision_radius_squared: i32,
    pub separation: f64,
    pub alignment: f64,
    pub boundary: f64,
    pub boundary_notch: i32,
    pub separation_notch: i32,
    pub alignment_notch: i32,
    pub vision_notch: i32,
    pub pace_notch: i32,
    pub avoid_notch: i32,
    /// How much of kin a stranger is: one at the bottom, then halves.
    pub avoid_kinship: f64,
    /// The room between flock homes, as a share of 2 * FLOCK_LEASH.
    pub avoid_room: f64,
    /// The weight of a bird's wariness of strangers.
    pub avoid_weight: f64,
}

impl Default for Config {
    /// The C's static initializer, before any option or notch is applied.
    fn default() -> Config {
        Config {
            birds: 800,
            speed: DEFAULT_SPEED as f64,
            base_speed: DEFAULT_SPEED as f64,
            pace: DEFAULT_PACE,
            // Not given: settle_the_bird_size makes it 30.
            bird_size: 0,
            palette: 0,
            flocks: 1,
            trails: false,
            hawks: 0,
            shape: 0,
            turning_notch: DEFAULT_TURNING_NOTCH,
            vision_cells: DEFAULT_VISION_RADIUS / SPATIAL_CELL_SIZE,
            vision_radius: DEFAULT_VISION_RADIUS,
            vision_radius_squared: DEFAULT_VISION_RADIUS * DEFAULT_VISION_RADIUS,
            separation: DEFAULT_SEPARATION_W,
            alignment: DEFAULT_ALIGNMENT_W,
            boundary: DEFAULT_BOUNDARY_W,
            boundary_notch: DEFAULT_NOTCH,
            separation_notch: DEFAULT_NOTCH,
            alignment_notch: DEFAULT_NOTCH,
            vision_notch: DEFAULT_VISION_NOTCH,
            pace_notch: DEFAULT_PACE_NOTCH,
            avoid_notch: DEFAULT_NOTCH,
            avoid_kinship: 0.0,
            avoid_room: 0.0,
            avoid_weight: 0.0,
        }
    }
}

/// `notch_value`: a notch's value between two ends.
pub fn notch_value(notch: i32, minimum: f64, maximum: f64) -> f64 {
    minimum + (maximum - minimum) * f64::from(notch) / f64::from(LEGEND_BAR_CELLS)
}

/// `notch_for_integer`: which notch a whole value belongs to.
pub fn notch_for_integer(value: i32, minimum: i32, maximum: i32) -> i32 {
    ((value - minimum) * LEGEND_BAR_CELLS + (maximum - minimum) / 2) / (maximum - minimum)
}

/// `notch_integer`: a notch's whole value between two ends.
pub fn notch_integer(notch: i32, minimum: i32, maximum: i32) -> i32 {
    minimum + ((maximum - minimum) * notch + LEGEND_BAR_CELLS / 2) / LEGEND_BAR_CELLS
}

/// `ldexp(1.0, -exponent)` for the small exponents the avoidance slider uses;
/// halving is exact, so this gives exactly the C library's answer.
fn negative_power_of_two(exponent: i32) -> f64 {
    let mut value = 1.0_f64;
    for _ in 0..exponent {
        value *= 0.5;
    }
    value
}

impl Config {
    /// The notch-derived part of `apply_notches`; the caller follows it with
    /// `update_speed`, which needs the screen and the frame's time.
    pub fn derive_from_notches(&mut self) {
        self.boundary = notch_value(self.boundary_notch, BOUNDARY_MIN, BOUNDARY_MAX);
        self.separation = notch_value(self.separation_notch, SEPARATION_MIN, SEPARATION_MAX);
        self.alignment = notch_value(self.alignment_notch, ALIGNMENT_MIN, ALIGNMENT_MAX);

        self.vision_radius = notch_integer(self.vision_notch, MIN_VISION_RADIUS, MAX_VISION_RADIUS);
        self.vision_radius_squared = self.vision_radius * self.vision_radius;
        // The block of cells swept has to cover the radius, so it rounds up.
        self.vision_cells = (self.vision_radius + SPATIAL_CELL_SIZE - 1) / SPATIAL_CELL_SIZE;

        // Counted in whole fifths, so at the default notch the pace is exact.
        self.pace = PACE_STEP * f64::from(self.pace_notch + 1);

        self.avoid_kinship = 0.0;
        self.avoid_room = 0.0;
        self.avoid_weight = 0.0;
        if self.avoid_notch < DEFAULT_NOTCH {
            self.avoid_kinship = if self.avoid_notch >= 0 {
                negative_power_of_two(self.avoid_notch)
            } else {
                // Unreachable through options or keys, which clamp at zero.
                2.0_f64.powi(-self.avoid_notch)
            };
        } else {
            let above = f64::from(self.avoid_notch - DEFAULT_NOTCH)
                / f64::from(LEGEND_BAR_CELLS - DEFAULT_NOTCH);
            self.avoid_room = 2.0 * above;
            self.avoid_weight = AVOID_WEIGHT_MAX * above;
        }
    }

    /// `apply_preset`'s notch assignments.
    pub fn set_preset_notches(&mut self, preset: &Preset) {
        self.boundary_notch = preset.notch[0];
        self.separation_notch = preset.notch[1];
        self.alignment_notch = preset.notch[2];
        self.vision_notch = preset.notch[3];
    }

    /// `apply_preset_defaults`'s notch assignments: back to the shipped look.
    pub fn set_default_notches(&mut self) {
        self.boundary_notch = DEFAULT_NOTCH;
        self.separation_notch = DEFAULT_NOTCH;
        self.alignment_notch = DEFAULT_NOTCH;
        self.vision_notch = DEFAULT_VISION_NOTCH;
        self.turning_notch = DEFAULT_TURNING_NOTCH;
        self.pace_notch = DEFAULT_PACE_NOTCH;
        self.avoid_notch = DEFAULT_NOTCH;
    }
}

/// A preset is the four flocking notches together: boundary, separation,
/// alignment, perception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub help: &'static str,
    pub notch: [i32; 4],
}

pub const PRESETS: [Preset; 3] = [
    // No preset takes the boundary below 7, whatever else it does.
    Preset {
        name: "murmuration",
        help: "one great restless body, the starling look",
        notch: [7, 3, 9, 8],
    },
    Preset { name: "swarm", help: "tight, fast and nervous, like insects", notch: [7, 8, 3, 3] },
    Preset { name: "storm", help: "loose and violent, thrown about", notch: [9, 10, 2, 6] },
];
pub const PRESET_COUNT: i32 = PRESETS.len() as i32;
pub const PRESET_NAMES: [&str; 3] = [PRESETS[0].name, PRESETS[1].name, PRESETS[2].name];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceilings_expand_as_the_c_macro_does() {
        assert_eq!(BOUNDARY_MAX.to_bits(), (0.01_f64 + 3.0 * (0.2 - 0.01)).to_bits());
    }

    #[test]
    fn kinship_halves_below_the_default() {
        let mut config = Config::default();
        for (notch, kinship) in [(0, 1.0), (1, 0.5), (2, 0.25), (3, 0.125)] {
            config.avoid_notch = notch;
            config.derive_from_notches();
            assert_eq!(config.avoid_kinship, kinship);
            assert_eq!((config.avoid_room, config.avoid_weight), (0.0, 0.0));
        }
    }
}
