//! The flock, as owned state: what cbirds `boids.c` keeps in process globals
//! (`config`, `screen`, `hawks`, `formation`, the clocks, the random state, the
//! flock measurements) lives in one [`Sim`], so every test starts fresh and no
//! test depends on another's leftovers.
//!
//! Each method is the C function of the same name, operation for operation:
//! the same expression order, the same `double`/`float` storage, the same
//! random draws in the same order, and a fused multiply-add exactly where the
//! canonical C build contracts one (see [`crate::fp`]). The step order is the
//! C's: snapshot the birds, build the grid from the snapshot, hunt, update the
//! birds from the snapshot, repeat per speed substep, then derive headings.

#![forbid(unsafe_code)]

mod flock;
mod formation;
mod hawks;

use std::f64::consts::PI;
use std::sync::LazyLock;

use crate::config::*;
use crate::fp;
use crate::palette::{PALETTES, Palette, Rgb, Theme};
use crate::rng::Rng;
use crate::spatial_grid::SpatialGrid;

pub use formation::{FORMATION_MAX_TARGETS, Formation};

/// `vector_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector {
    pub x: f64,
    pub y: f64,
}

/// `trig_entry_t`: stored as `float`, as in the C.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrigEntry {
    pub cosine: f32,
    pub sine: f32,
}

/// `trig_lookup_table`, built once (`trig_lookup_init`): immutable data.
pub static TRIG_LOOKUP_TABLE: LazyLock<[TrigEntry; TRIG_LOOKUP_SIZE as usize]> =
    LazyLock::new(|| {
        let mut table = [TrigEntry::default(); TRIG_LOOKUP_SIZE as usize];
        for (i, entry) in table.iter_mut().enumerate() {
            let angle = i as f64 * 2.0 * PI / f64::from(TRIG_LOOKUP_SIZE);
            entry.cosine = fp::cos(angle) as f32;
            entry.sine = fp::sin(angle) as f32;
        }
        table
    });

/// `trig_lookup`: the unit vector read by the neighbour loop, rounded to the
/// nearest table entry.
#[inline]
pub fn trig_lookup(angle: f64) -> TrigEntry {
    let scaled = angle * (f64::from(TRIG_LOOKUP_SIZE) / (2.0 * PI));
    let nearest = (scaled + if scaled >= 0.0 { 0.5 } else { -0.5 }) as i32;
    TRIG_LOOKUP_TABLE[(nearest as u32 & TRIG_LOOKUP_MASK as u32) as usize]
}

/// `bird_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bird {
    pub x: f64,
    pub y: f64,
    pub direction: f64,
    pub frame: i32,
    /// Index into the palette, and half of the image id.
    pub shade: i32,
    /// Which flock it reads: separation ignores this, the rest does not.
    pub flock: i32,
    /// Near or far; the two never see each other.
    pub layer: i32,
    /// Where in the beat it is: an index into WING_SEQUENCE.
    pub wing: i32,
    pub wing_clock: f64,
    /// Seconds of wings held out and still.
    pub gliding: f64,
    pub trail_x: [f64; TRAIL_LENGTH as usize],
    pub trail_y: [f64; TRAIL_LENGTH as usize],
    pub trail_at: i32,
    pub trail_held: i32,
}

/// `hawk_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hawk {
    pub x: f64,
    pub y: f64,
    pub direction: f64,
    pub frame: i32,
    /// Index of the bird it is chasing, negative for none.
    pub prey: i32,
    /// Seconds before it may change its mind.
    pub commitment: f64,
    /// Seconds left of a straight run out of the flock.
    pub passing: f64,
    /// A hawk soars, wings out, and beats them only in the dive.
    pub wing: i32,
    pub wing_clock: f64,
}

/// `screen_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub width: i32,
    pub height: i32,
    pub cols: i32,
    pub rows: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub turn_x: i32,
    pub turn_y: i32,
    pub turn_bottom: i32,
    /// The panel in pixels, zero when there is none.
    pub legend_width: i32,
    pub legend_height: i32,
}

/// Where the pointer is, in pixels, and whether it has ever been seen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mouse {
    pub present: bool,
    pub x: f64,
    pub y: f64,
}

/// `clock_state`: a frame counter and the seconds, for whatever animates on
/// its own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clock {
    pub frame: i64,
    pub seconds: f64,
}

/// `render_mode_t`, with the internal unset state kept until mode selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    /// Not asked for: braille live, sprites in a recording.
    Unset = -1,
    Kitty = 0,
    Braille = 1,
    /// Solid two by three blocks: bolder than dots, needs a 2020 font.
    Sextants = 2,
    Blocks = 3,
}

pub const RENDER_NAMES: [&str; 4] = ["kitty", "braille", "sextants", "blocks"];

impl RenderMode {
    /// The option table's enum index, `-1` for unset.
    pub fn from_index(index: i32) -> RenderMode {
        match index {
            0 => RenderMode::Kitty,
            1 => RenderMode::Braille,
            2 => RenderMode::Sextants,
            3 => RenderMode::Blocks,
            _ => RenderMode::Unset,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RenderMode::Unset => "(unset)",
            RenderMode::Kitty => RENDER_NAMES[0],
            RenderMode::Braille => RENDER_NAMES[1],
            RenderMode::Sextants => RENDER_NAMES[2],
            RenderMode::Blocks => RENDER_NAMES[3],
        }
    }
}

/// The Konami code's last ten keys (`konami_seen`, `konami_at`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Konami {
    pub seen: [u8; KONAMI_LENGTH],
    pub at: i32,
}

pub const KONAMI: &[u8; 10] = b"AABBDCDCba";
pub const KONAMI_LENGTH: usize = 10;

/// Everything the simulation reads and writes, formerly process globals.
#[derive(Clone, Debug)]
pub struct Sim {
    pub config: Config,
    pub screen: Screen,
    /// Hidden until --panel or h asks for it.
    pub legend_enabled: bool,
    pub render_mode: RenderMode,
    /// --depth: a second plane of birds further off.
    pub deep_look: bool,
    pub mouse: Mouse,
    pub clock: Clock,
    /// How much real or recorded time the next step represents.
    pub frame_seconds: f64,
    pub paused: bool,
    pub step_once: bool,
    pub population_changed: bool,
    pub theme: Theme,
    /// A PNG of somebody's own: it keeps its own colours.
    pub custom_sprite: bool,
    pub formation: Formation,
    /// The rain falls, and nothing else has a wind.
    pub rain: bool,
    pub hawks: [Hawk; MAX_HAWKS as usize],
    /// The hawks' images always go up, so k can summon one at any time.
    pub hawk_sets_built: bool,
    pub rng: Rng,
    pub flock_center_x: [f64; MAX_FLOCKS as usize],
    pub flock_center_y: [f64; MAX_FLOCKS as usize],
    pub flock_home_x: [f64; MAX_FLOCKS as usize],
    pub flock_home_y: [f64; MAX_FLOCKS as usize],
    pub flock_leash: [f64; MAX_FLOCKS as usize],
    pub last_key_at: f64,
    pub last_drift_at: f64,
    pub konami: Konami,
    pub requested_preset: i32,
}

impl Default for Sim {
    fn default() -> Sim {
        Sim::new()
    }
}

/// `normalized_angle`: `atan2` in `[0, 2π)`.
#[inline]
pub fn normalized_angle(y: f64, x: f64) -> f64 {
    let angle = y.atan2(x);
    if angle < 0.0 { angle + 2.0 * PI } else { angle }
}

/// `direction_frame`: the rotation frame for a heading.
#[inline]
pub fn direction_frame(radians: f64) -> i32 {
    let degrees = (radians * 180.0 / PI) as i32;
    ((degrees % 360 + 360) % 360) / FRAME_ANGLE
}

/// `turn_towards`: at most `most` radians from `from` toward `to`.
pub fn turn_towards(from: f64, to: f64, most: f64) -> f64 {
    let mut delta = fp::sin(to - from).atan2(fp::cos(to - from));
    if delta > most {
        delta = most;
    }
    if delta < -most {
        delta = -most;
    }
    let mut turned = from + delta;
    if turned < 0.0 {
        turned += 2.0 * PI;
    }
    if turned >= 2.0 * PI {
        turned -= 2.0 * PI;
    }
    turned
}

/// `set_image_id`: zero is not an id.
#[inline]
pub fn set_image_id(set: i32, frame: i32) -> u32 {
    (set * ROTATION_FRAMES + frame) as u32 + 1
}

impl Sim {
    /// The state the C program starts in, before options: its static
    /// initializers.
    pub fn new() -> Sim {
        Sim {
            config: Config::default(),
            screen: Screen::default(),
            legend_enabled: false,
            render_mode: RenderMode::Unset,
            deep_look: false,
            mouse: Mouse::default(),
            clock: Clock::default(),
            frame_seconds: 1.0 / f64::from(FRAME_RATE),
            paused: false,
            step_once: false,
            population_changed: false,
            theme: Theme::default(),
            custom_sprite: false,
            formation: Formation::default(),
            rain: false,
            hawks: [Hawk::default(); MAX_HAWKS as usize],
            hawk_sets_built: false,
            rng: Rng::default(),
            flock_center_x: [0.0; MAX_FLOCKS as usize],
            flock_center_y: [0.0; MAX_FLOCKS as usize],
            flock_home_x: [0.0; MAX_FLOCKS as usize],
            flock_home_y: [0.0; MAX_FLOCKS as usize],
            flock_leash: [0.0; MAX_FLOCKS as usize],
            last_key_at: 0.0,
            last_drift_at: 0.0,
            konami: Konami::default(),
            requested_preset: -1,
        }
    }

    // ----- time and speed -------------------------------------------------

    /// `flight_seconds`: the frame's time at the pace the slider asks for.
    #[inline]
    pub fn flight_seconds(&self) -> f64 {
        self.frame_seconds * self.config.pace
    }

    /// `update_speed`: the step a bird takes, capped once by the screen.
    pub fn update_speed(&mut self) {
        let mut pixels_per_second = f64::from(DEFAULT_SPEED * FRAME_RATE);
        let shorter = f64::from(self.screen.width.min_c(self.screen.height));
        let safe_per_second = shorter / 10.0 * f64::from(FRAME_RATE);
        if shorter > 0.0 && pixels_per_second > safe_per_second {
            pixels_per_second = safe_per_second;
        }
        self.config.base_speed = pixels_per_second * self.frame_seconds;
        self.config.speed = self.config.base_speed * self.config.pace;
    }

    /// `set_frame_seconds`: zero time means zero movement, never a fallback.
    pub fn set_frame_seconds(&mut self, seconds: f64) {
        self.frame_seconds = if seconds > 0.0 { seconds } else { 0.0 };
        self.update_speed();
    }

    /// `apply_notches`: every tunable from its notch, then the speed.
    pub fn apply_notches(&mut self) {
        self.config.derive_from_notches();
        self.update_speed();
    }

    /// `apply_preset`.
    pub fn apply_preset(&mut self, which: i32) {
        self.config.set_preset_notches(&PRESETS[which as usize]);
        self.apply_notches();
    }

    /// `apply_preset_defaults`: back to the shipped look.
    pub fn apply_preset_defaults(&mut self) {
        self.config.set_default_notches();
        self.apply_notches();
    }

    /// `settle_the_bird_size`: thirty pixels when --size was not given.
    pub fn settle_the_bird_size(&mut self) {
        if self.config.bird_size == 0 {
            self.config.bird_size = DEFAULT_BIRD_SIZE;
        }
    }

    /// `turning_notch_radians`: what the notch means before elapsed time.
    pub fn turning_notch_radians(&self) -> f64 {
        if self.config.turning_notch >= LEGEND_BAR_CELLS {
            return 2.0 * PI;
        }
        PI / 6.0
            + (PI / 2.0 - PI / 6.0) * f64::from(self.config.turning_notch)
                / f64::from(LEGEND_BAR_CELLS)
    }

    /// `turn_limit`: the notch scaled by elapsed flight time.
    pub fn turn_limit(&self) -> f64 {
        if self.config.turning_notch >= LEGEND_BAR_CELLS {
            return 2.0 * PI;
        }
        let scaled =
            self.turning_notch_radians() * f64::from(FRAME_RATE) * self.flight_seconds();
        if scaled > 2.0 * PI { 2.0 * PI } else { scaled }
    }

    // ----- the screen and the panel ---------------------------------------

    /// `update_turn_distances`: edge bands proportional to the viewport.
    pub fn update_turn_distances(&mut self) {
        let screen = &mut self.screen;
        screen.turn_x = screen.width / TURN_BAND_DIVISOR;
        screen.turn_y = screen.height / TURN_BAND_DIVISOR;
        screen.turn_bottom = screen.height / BOTTOM_BAND_DIVISOR;
        if screen.turn_x < 1 {
            screen.turn_x = 1;
        }
        if screen.turn_y < 1 {
            screen.turn_y = 1;
        }
        if screen.turn_bottom < 1 {
            screen.turn_bottom = 1;
        }
    }

    /// `legend_rows`: one more row, the avoidance slider, with two flocks.
    pub fn legend_rows(&self) -> i32 {
        if self.config.flocks > 1 { LEGEND_MAX_ROWS } else { LEGEND_ROWS }
    }

    /// `measure_legend`: the panel in pixels, or none below the minimum.
    pub fn measure_legend(&mut self) {
        self.screen.legend_width = 0;
        self.screen.legend_height = 0;
        if !self.legend_enabled {
            return;
        }
        if self.screen.cols < LEGEND_MIN_COLS || self.screen.rows < LEGEND_MIN_ROWS {
            return;
        }
        self.screen.legend_width = LEGEND_COLUMNS * self.screen.cell_width;
        self.screen.legend_height = self.legend_rows() * self.screen.cell_height;
    }

    /// `apply_screen_size`: the derivation behind the ioctl, with the C's
    /// fallbacks for a terminal that reports nothing.
    pub fn apply_screen_size(&mut self, cols: i32, rows: i32, pixel_width: i32, pixel_height: i32) {
        let screen = &mut self.screen;
        screen.cols = if cols > 0 { cols } else { DEFAULT_COLS };
        screen.rows = if rows > 0 { rows } else { DEFAULT_ROWS };
        screen.width = pixel_width;
        screen.height = pixel_height;
        if screen.width <= 0 || screen.height <= 0 {
            screen.width = screen.cols * DEFAULT_CELL_WIDTH;
            screen.height = screen.rows * DEFAULT_CELL_HEIGHT;
        }
        screen.cell_width = screen.width / screen.cols;
        screen.cell_height = screen.height / screen.rows;
        if screen.cell_width < 1 {
            screen.cell_width = 1;
        }
        if screen.cell_height < 1 {
            screen.cell_height = 1;
        }
        // Before the distances: the panel's turn zone depends on the speed.
        self.update_speed();
        self.measure_legend();
        self.update_turn_distances();
    }

    /// `legend_turn_zone`: the panel grown by one frame of travel.
    #[inline]
    pub fn legend_turn_zone(&self, x: f64, y: f64) -> bool {
        self.screen.legend_width > 0
            && x < f64::from(self.screen.legend_width) + self.config.speed
            && y < f64::from(self.screen.legend_height) + self.config.speed
    }

    /// `legend_repels`: out through the nearer of the two open sides.
    pub fn legend_repels(&self, bird: &Bird, boundary: &mut Vector) -> bool {
        if !self.legend_turn_zone(bird.x, bird.y) {
            return false;
        }
        let escape_x = f64::from(self.screen.legend_width) + self.config.speed - bird.x;
        let escape_y = f64::from(self.screen.legend_height) + self.config.speed - bird.y;
        if escape_x <= escape_y {
            boundary.x = LEGEND_PUSH;
        } else {
            boundary.y = LEGEND_PUSH;
        }
        true
    }

    // ----- the look ---------------------------------------------------------

    /// `palette()`.
    pub fn palette(&self) -> &'static Palette {
        &PALETTES[self.config.palette as usize]
    }

    /// The palette's tints, the learned theme ramp for the theme palette (the
    /// C's `tints` pointer, which for the theme points at `theme_tints`).
    pub fn palette_tints(&self) -> Option<&[Rgb; 5]> {
        let palette = self.palette();
        match palette.tints {
            Some(tints) => Some(tints),
            None if palette.name == "theme" => Some(&self.theme.tints),
            None => None,
        }
    }

    /// `palette_follows_the_theme`.
    pub fn palette_follows_the_theme(&self) -> bool {
        self.palette().name == "theme"
    }

    /// `palette_shades`: one untinted set for a custom sprite.
    pub fn palette_shades(&self) -> i32 {
        if self.custom_sprite {
            return 1;
        }
        self.palette().shades
    }

    /// `hawk_colour`.
    pub fn hawk_colour(&self) -> Rgb {
        crate::palette::hawk_colour(self.palette(), self.palette_tints(), self.custom_sprite)
    }

    /// `hawk_sprite_size`: twice a bird, capped at twice the largest bird.
    pub fn hawk_sprite_size(&self) -> i32 {
        let size = self.config.bird_size * 2;
        if size > 2 * MAX_BIRD_SIZE { 2 * MAX_BIRD_SIZE } else { size }
    }

    /// `hawk_draw_offset`: a hawk is drawn from its middle.
    pub fn hawk_draw_offset(&self) -> i32 {
        self.hawk_sprite_size() / 2
    }

    /// `drawing_with_text`.
    pub fn drawing_with_text(&self) -> bool {
        matches!(self.render_mode, RenderMode::Braille | RenderMode::Sextants | RenderMode::Blocks)
    }

    /// `live_render_mode`: braille unless something else was asked for.
    pub fn live_render_mode(&self) -> RenderMode {
        if self.render_mode == RenderMode::Unset { RenderMode::Braille } else { self.render_mode }
    }

    /// `settle_the_palette_without_a_terminal`: headless, the theme has no
    /// answers, so the shipped ramp.
    pub fn settle_the_palette_without_a_terminal(&mut self) {
        if self.palette_follows_the_theme() {
            self.config.palette = crate::palette::fallback_palette();
        }
    }

    // ----- the sprite catalogue ---------------------------------------------

    /// `flock_set`.
    #[inline]
    pub fn flock_set(&self, shade: i32, wing: i32, layer: i32) -> i32 {
        if layer > 0 {
            return self.palette_shades() * WING_PHASES + shade;
        }
        shade * WING_PHASES + wing
    }

    /// `hawk_set`.
    #[inline]
    pub fn hawk_set(&self, wing: i32) -> i32 {
        self.palette_shades() * (WING_PHASES + 1) + wing
    }

    /// `trail_set`.
    #[inline]
    pub fn trail_set(&self, step: i32) -> i32 {
        self.palette_shades() * (WING_PHASES + 1) + WING_PHASES + step
    }

    /// `sprite_set_count`.
    pub fn sprite_set_count(&self) -> i32 {
        self.trail_set(TRAIL_LENGTH)
    }

    /// `sprite_image_id`.
    pub fn sprite_image_id(&self, bird: &Bird) -> u32 {
        set_image_id(
            self.flock_set(
                bird.shade,
                WING_SEQUENCE[(bird.wing % WING_CYCLE) as usize],
                bird.layer,
            ),
            bird.frame,
        )
    }

    /// `hawk_image_id`.
    pub fn hawk_image_id(&self, hawk: &Hawk) -> u32 {
        set_image_id(self.hawk_set(WING_SEQUENCE[(hawk.wing % WING_CYCLE) as usize]), hawk.frame)
    }

    // ----- one frame ----------------------------------------------------------

    /// `fly`: one frame of flight for the hawks and the flock, from the
    /// snapshot and grid the caller has just built. Above pace one the frame
    /// is flown in as many equal steps as the pace needs.
    pub fn fly(&mut self, birds: &mut [Bird], snapshot: &mut [Bird], grid: &mut SpatialGrid) {
        let count = self.config.birds as usize;
        let mut steps = (self.config.pace - 1e-9).ceil() as i32;
        if steps < 1 {
            steps = 1;
        }
        let whole = self.frame_seconds;
        if steps > 1 {
            self.set_frame_seconds(whole / f64::from(steps));
        }
        for step in 0..steps {
            if step > 0 {
                snapshot[..count].copy_from_slice(&birds[..count]);
                // Prepared for this many birds by the caller, so it cannot fail.
                let _ = grid.build(self.config.birds, |i| (snapshot[i].x, snapshot[i].y));
            }
            self.hunt(snapshot);
            self.update_birds(birds, snapshot, grid);
        }
        if steps > 1 {
            self.set_frame_seconds(whole);
        }
        for bird in &mut birds[..count] {
            bird.frame = direction_frame(bird.direction);
        }
    }

    /// `fly_away`: straight up, every one of them, at the shipped pace.
    pub fn fly_away(&self, birds: &mut [Bird]) {
        for bird in &mut birds[..self.config.birds as usize] {
            bird.direction = 3.0 * PI / 2.0;
            bird.y -= self.config.base_speed;
            bird.frame = direction_frame(bird.direction);
        }
    }

    /// The simulation half of `render_frame`: paused holds still, a step
    /// grants one frame of motion.
    pub fn advance(&mut self, birds: &mut [Bird], snapshot: &mut [Bird], grid: &mut SpatialGrid) {
        if !self.paused || self.step_once {
            self.step_once = false;
            self.fly(birds, snapshot, grid);
        }
    }

    /// The copy and rebuild every mode performs before flying a frame.
    pub fn snapshot_and_build(
        &self,
        birds: &[Bird],
        snapshot: &mut [Bird],
        grid: &mut SpatialGrid,
    ) -> Result<(), crate::spatial_grid::GridError> {
        let count = self.config.birds as usize;
        snapshot[..count].copy_from_slice(&birds[..count]);
        grid.build(self.config.birds, |i| (snapshot[i].x, snapshot[i].y))
    }

    /// Whether the intro's hold is up (`formation.until` reached).
    pub fn release_the_formation_if_due(&mut self) {
        if self.formation.writing
            && self.formation.until >= 0.0
            && self.clock.seconds >= self.formation.until
        {
            self.formation.clear();
        }
    }
}

/// C's `a < b ? a : b` on ints, spelled out where the C spells it out.
trait MinC {
    fn min_c(self, other: Self) -> Self;
}

impl MinC for i32 {
    #[inline]
    fn min_c(self, other: i32) -> i32 {
        if self < other { self } else { other }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_state_is_the_c_static_initializer() {
        let sim = Sim::new();
        assert_eq!(sim.config.speed, 40.0);
        assert_eq!(sim.frame_seconds, 1.0 / 60.0);
        assert_eq!(sim.requested_preset, -1);
        assert_eq!(sim.render_mode, RenderMode::Unset);
    }

    #[test]
    fn frames_follow_the_heading() {
        assert_eq!(direction_frame(0.0), 0);
        assert_eq!(direction_frame(-0.1), 59);
        assert_eq!(direction_frame(2.0 * PI - 1e-12), 59);
        assert_eq!(set_image_id(0, 0), 1);
    }
}
