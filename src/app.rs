//! The program: its option table, the order in which options take effect,
//! and the choice of mode. Translated from cbirds `boids.c` (`OPTIONS`,
//! `EXAMPLES`, `KEYS_HELP`, `usage`, `read_options`, `main`).
//!
//! Where the C calls `exit`, these functions return the exit code instead,
//! having written exactly what the C writes first; the caller unwinds the
//! terminal and flushes `stdout` before the process ends.

#![forbid(unsafe_code)]

use crate::platform::OsStrExt;
use std::ffi::OsString;

use crate::config::*;
use crate::image::png_status_string;
use crate::options::{self, Example, Kind, OptionSpec, Status};
use crate::palette::{PALETTE_NAMES, palette_named};
use crate::render::Renderer;
use crate::simulation::{RENDER_NAMES, RenderMode, Sim};
use crate::sprites::{SHAPE_NAMES, SpriteError, load_sprite};
use crate::stdio::{CStdout, cat, eprint};

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;
/// A mistyped command is not a run that went wrong.
pub const EXIT_USAGE: i32 = 2;

/// The product identity: the one intentional difference from cbirds.
pub const PRODUCT: &str = "rbirds";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The option targets that are not simulation state (the C's file-scope
/// `frame_limit`, `record_path`, `requested_seed` and the rest).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub frame_limit: i32,
    pub bench_frames: i32,
    pub snapshot_path: Option<OsString>,
    pub record_path: Option<OsString>,
    pub record_fps: i32,
    pub record_seconds: i32,
    pub record_columns: i32,
    pub record_rows: i32,
    pub requested_record_size: Option<OsString>,
    pub matrix_mode: bool,
    pub unlock_fps: bool,
    pub requested_perception: i32,
    pub requested_seed: i32,
    /// --big-flock's count; zero when not asked for.
    pub big_flock_birds: i32,
    pub sprite_path: Option<OsString>,
    /// `render_mode` as the option table stores it: an index, -1 unset.
    pub render_request: i32,
    /// As invoked, for every message.
    pub program_name: Vec<u8>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            frame_limit: 0,
            bench_frames: 0,
            snapshot_path: None,
            record_path: None,
            record_fps: 25,
            record_seconds: 6,
            record_columns: 96,
            record_rows: 26,
            requested_record_size: None,
            matrix_mode: false,
            unlock_fps: false,
            requested_perception: DEFAULT_VISION_RADIUS,
            requested_seed: -1,
            big_flock_birds: 0,
            sprite_path: None,
            render_request: RenderMode::Unset as i32,
            program_name: PRODUCT.as_bytes().to_vec(),
        }
    }
}

/// Everything the C keeps in globals, owned: the option table's target.
#[derive(Debug, Default)]
pub struct Program {
    pub sim: Sim,
    pub settings: Settings,
    pub renderer: Renderer,
}

const FLOCK: &str = "Flock";
const SLIDERS: &str = "Sliders   0 to 12, as the panel shows them";
const LOOK: &str = "Look";
const ODDITIES: &str = "Oddities";
const OUTPUT: &str = "Output";
const GENERAL: &str = "General";

/// One `option_t` row, in the C initializer's field order.
#[allow(clippy::too_many_arguments)]
const fn row(
    shorthand: Option<u8>,
    name: &'static str,
    alias: Option<&'static str>,
    kind: Kind<Program>,
    metavar: Option<&'static str>,
    help: &'static str,
    group: &'static str,
    essential: bool,
) -> OptionSpec<Program> {
    OptionSpec { shorthand, name, alias, kind, metavar, help, group, essential }
}

/// The option table: the parser, the help and the completions all come off
/// this, in this order.
pub static OPTIONS: [OptionSpec<Program>; 30] = [
    row(
        Some(b'n'),
        "birds",
        None,
        Kind::Int { target: |p| &mut p.sim.config.birds, min: 1.0, max: MAX_BIRDS as f64 },
        Some("COUNT"),
        "how many birds (default 800)",
        FLOCK,
        true,
    ),
    row(
        Some(b's'),
        "size",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.bird_size,
            min: MIN_BIRD_SIZE as f64,
            max: MAX_BIRD_SIZE as f64,
        },
        Some("PIXELS"),
        "sprite size in pixels (default 30)",
        FLOCK,
        true,
    ),
    row(
        Some(b'g'),
        "flocks",
        Some("groups"),
        Kind::Int { target: |p| &mut p.sim.config.flocks, min: 1.0, max: MAX_FLOCKS as f64 },
        Some("COUNT"),
        "flocks that keep to their own kind (default 1)",
        FLOCK,
        true,
    ),
    row(
        Some(b'k'),
        "hawks",
        None,
        Kind::Int { target: |p| &mut p.sim.config.hawks, min: 0.0, max: MAX_HAWKS as f64 },
        Some("COUNT"),
        "predators hunting the flock (default 0)",
        FLOCK,
        true,
    ),
    row(
        None,
        "preset",
        None,
        Kind::Enum { target: |p| &mut p.sim.requested_preset, names: &PRESET_NAMES },
        Some("NAME"),
        "murmuration, swarm, storm",
        FLOCK,
        true,
    ),
    row(
        None,
        "seed",
        None,
        Kind::Int { target: |p| &mut p.settings.requested_seed, min: 0.0, max: 2147483647.0 },
        Some("N"),
        "the same seed gives the same flock",
        FLOCK,
        false,
    ),
    row(
        None,
        "big-flock",
        None,
        Kind::Int {
            target: |p| &mut p.settings.big_flock_birds,
            min: 1.0,
            max: BIG_FLOCK_BIRDS as f64,
        },
        Some("COUNT"),
        "up to 65536 birds on several cores, instead of --birds",
        FLOCK,
        false,
    ),
    row(
        None,
        "boundary",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.boundary_notch,
            min: 0.0,
            max: LEGEND_BAR_CELLS as f64,
        },
        Some("NOTCH"),
        "how hard the edges push back (default 4)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "separation",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.separation_notch,
            min: 0.0,
            max: LEGEND_BAR_CELLS as f64,
        },
        Some("NOTCH"),
        "how much a bird keeps its distance (default 4)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "alignment",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.alignment_notch,
            min: 0.0,
            max: LEGEND_BAR_CELLS as f64,
        },
        Some("NOTCH"),
        "how much a bird matches its neighbours (default 4)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "turning",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.turning_notch,
            min: 0.0,
            max: LEGEND_BAR_CELLS as f64,
        },
        Some("NOTCH"),
        "sharpest turn a frame, 12 is instant (default 8)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "perception",
        None,
        Kind::Int {
            target: |p| &mut p.settings.requested_perception,
            min: MIN_VISION_RADIUS as f64,
            max: MAX_VISION_RADIUS as f64,
        },
        Some("PIXELS"),
        "how far a bird sees, 12 to 60 (default 36)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "speed",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.pace_notch,
            min: 0.0,
            max: LEGEND_BAR_CELLS as f64,
        },
        Some("NOTCH"),
        "how fast the flock flies, 0.2x to 2.6x (default 1, 0.4x)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "avoidance",
        None,
        Kind::Int {
            target: |p| &mut p.sim.config.avoid_notch,
            min: 0.0,
            max: LEGEND_BAR_CELLS as f64,
        },
        Some("NOTCH"),
        "how much flocks keep out of each other's way (default 4)",
        SLIDERS,
        false,
    ),
    row(
        Some(b'c'),
        "color",
        Some("palette"),
        Kind::Enum { target: |p| &mut p.sim.config.palette, names: &PALETTE_NAMES },
        Some("RAMP"),
        "theme, ember, ice, acid, matrix, aurora, prism, potion, dusk, ash",
        LOOK,
        true,
    ),
    row(
        None,
        "shape",
        None,
        Kind::Enum { target: |p| &mut p.sim.config.shape, names: &SHAPE_NAMES },
        Some("NAME"),
        "bird, arrow, plane, dot",
        LOOK,
        true,
    ),
    row(
        None,
        "sprite",
        None,
        Kind::Str(|p| &mut p.settings.sprite_path),
        Some("FILE"),
        "a PNG you supply, kept in its own colours",
        LOOK,
        false,
    ),
    row(
        Some(b'e'),
        "trails",
        None,
        Kind::Flag(|p| &mut p.sim.config.trails),
        None,
        "faint tails behind the flock",
        LOOK,
        false,
    ),
    row(
        None,
        "depth",
        None,
        Kind::Flag(|p| &mut p.sim.deep_look),
        None,
        "a second sky further off: smaller, slower, dimmer birds",
        LOOK,
        true,
    ),
    row(
        Some(b'l'),
        "panel",
        None,
        Kind::Flag(|p| &mut p.sim.legend_enabled),
        None,
        "the sliders in the corner from the start; h toggles them",
        LOOK,
        true,
    ),
    row(
        None,
        "render",
        None,
        Kind::Enum { target: |p| &mut p.settings.render_request, names: &RENDER_NAMES },
        Some("HOW"),
        "braille (default), sextants, blocks, kitty, sixel",
        LOOK,
        true,
    ),
    row(
        None,
        "matrix",
        None,
        Kind::Flag(|p| &mut p.settings.matrix_mode),
        None,
        "it is raining birds",
        ODDITIES,
        false,
    ),
    row(
        None,
        "bench",
        None,
        Kind::Int { target: |p| &mut p.settings.bench_frames, min: 0.0, max: 1000000.0 },
        Some("N"),
        "run N frames with no terminal, print the numbers, quit",
        OUTPUT,
        false,
    ),
    row(
        None,
        "frames",
        None,
        Kind::Int { target: |p| &mut p.settings.frame_limit, min: 0.0, max: 1000000.0 },
        Some("N"),
        "quit after N frames, for recording",
        OUTPUT,
        false,
    ),
    row(
        None,
        "snapshot",
        None,
        Kind::Str(|p| &mut p.settings.snapshot_path),
        Some("FILE"),
        "write the last frame as a PNG",
        OUTPUT,
        false,
    ),
    row(
        None,
        "record",
        None,
        Kind::Str(|p| &mut p.settings.record_path),
        Some("FILE"),
        "record a GIF, or a .cast for asciinema, with no terminal, and quit",
        OUTPUT,
        false,
    ),
    row(
        None,
        "record-fps",
        None,
        Kind::Int { target: |p| &mut p.settings.record_fps, min: 2.0, max: MAX_CAST_FPS as f64 },
        Some("RATE"),
        "frames a second; a GIF can carry up to 50 (default 25)",
        OUTPUT,
        false,
    ),
    row(
        None,
        "record-seconds",
        None,
        Kind::Int { target: |p| &mut p.settings.record_seconds, min: 1.0, max: 120.0 },
        Some("SECONDS"),
        "how long the recording runs (default 6)",
        OUTPUT,
        false,
    ),
    row(
        None,
        "record-size",
        None,
        Kind::Str(|p| &mut p.settings.requested_record_size),
        Some("COLSxROWS"),
        "the size to record at, in cells (default 96x26)",
        OUTPUT,
        false,
    ),
    row(
        None,
        "unlock-fps",
        None,
        Kind::Flag(|p| &mut p.settings.unlock_fps),
        None,
        "render as fast as the terminal allows",
        GENERAL,
        false,
    ),
];

/// The panel teaches the slider keys, so this only has to list the rest.
pub const KEYS_HELP: &str = concat!(
    "\nKeys   b/B s/S a/A t/T p/P v/V   one notch down / up\n",
    "       space pause   . step   0 reset   +/- birds   Tab preset\n",
    "       h panel   e trails   k/K hawks   g/G flocks avoid, with two or more\n",
    "       q quit\n",
);

pub const TAGLINE: &str = "rbirds - a flock of birds in your terminal.";

pub const EXAMPLES: [Example; 8] = [
    Example { command: "rbirds", what: "a flock in braille, and nothing to read" },
    Example { command: "rbirds --preset murmuration", what: "the starling look" },
    Example { command: "rbirds --hawks 2 --color ice", what: "something to watch" },
    Example {
        command: "rbirds --flocks 3 --color ember",
        what: "three of them, keeping to their own",
    },
    Example { command: "rbirds --depth --trails", what: "a second sky behind the first" },
    Example { command: "rbirds --render kitty", what: "sprites, in Kitty or Ghostty" },
    Example { command: "rbirds --render sixel", what: "pixels, in Windows Terminal 1.22+" },
    Example { command: "rbirds --record flock.gif", what: "a GIF, with no terminal in the way" },
];

/// `usage`.
pub fn usage(out: &mut CStdout, program: &[u8], everything: bool) {
    let mut text = Vec::new();
    let _ =
        options::usage(&mut text, program, Some(TAGLINE), Some(&EXAMPLES), &OPTIONS, everything);
    if everything {
        text.extend_from_slice(KEYS_HELP.as_bytes());
    }
    out.print(&text);
}

impl Program {
    pub fn new() -> Program {
        Program::default()
    }

    /// The C's `sprite_path` as the sprite loader takes it.
    pub fn sprite_path(&self) -> Option<&std::ffi::OsStr> {
        self.settings.sprite_path.as_deref()
    }
}

/// `read_decimal` over a whole string, for `--record-size`.
fn record_size(text: &[u8]) -> Option<(i32, i32)> {
    let mut at = 0;
    let columns = crate::input::read_decimal(text, &mut at)?;
    if text.get(at) != Some(&b'x') {
        return None;
    }
    at += 1;
    let rows = crate::input::read_decimal(text, &mut at)?;
    if at != text.len() {
        return None;
    }
    if !(40..=400).contains(&columns) || !(14..=120).contains(&rows) {
        return None;
    }
    Some((columns, rows))
}

/// `read_options`: `Err(code)` where the C exits, having written what it
/// writes. `argv[0]` is the program name as invoked.
pub fn read_options(
    program: &mut Program,
    argv: &[OsString],
    stdout: &mut CStdout,
) -> Result<(), i32> {
    if let Some(first) = argv.first() {
        // A C string ends at its first NUL; argv cannot hold one.
        program.settings.program_name = first.as_bytes().to_vec();
    }
    let name = program.settings.program_name.clone();
    let parsed = options::parse(&OPTIONS, program, argv, 160);
    program.sim.render_mode = RenderMode::from_index(program.settings.render_request);
    program.sim.custom_sprite = program.settings.sprite_path.is_some();

    match parsed.status {
        Status::Help | Status::HelpFull => {
            usage(stdout, &name, parsed.status == Status::HelpFull);
            return Err(EXIT_SUCCESS);
        }
        Status::Completion => {
            let mut text = Vec::new();
            let known =
                options::completion(&mut text, &parsed.message, PRODUCT, &OPTIONS).unwrap_or(false);
            stdout.print(&text);
            if !known {
                eprint(&cat(&[&name, b": --completion wants bash, zsh or fish\n"]));
                return Err(EXIT_USAGE);
            }
            return Err(EXIT_SUCCESS);
        }
        Status::Version => {
            stdout.print(format!("{PRODUCT} {VERSION}\n").as_bytes());
            return Err(EXIT_SUCCESS);
        }
        Status::Error => {
            eprint(&cat(&[&name, b": ", &parsed.message, b"\n"]));
            eprint(&cat(&[b"Try '", &name, b" --help'.\n"]));
            return Err(EXIT_USAGE);
        }
        Status::Ok => {}
    }

    let sim = &mut program.sim;
    let settings = &mut program.settings;
    if settings.big_flock_birds > 0 {
        sim.big_flock = true;
        sim.config.birds = settings.big_flock_birds;
        sim.threads = crate::parallel::big_flock_threads();
    }
    // A preset is expanded first so that a slider given after it still wins,
    // as far as the table can tell: only values off the shipped default are
    // put back.
    if sim.requested_preset >= 0 {
        let boundary = sim.config.boundary_notch;
        let separation = sim.config.separation_notch;
        let alignment = sim.config.alignment_notch;
        sim.apply_preset(sim.requested_preset);
        if boundary != DEFAULT_NOTCH {
            sim.config.boundary_notch = boundary;
        }
        if separation != DEFAULT_NOTCH {
            sim.config.separation_notch = separation;
        }
        if alignment != DEFAULT_NOTCH {
            sim.config.alignment_notch = alignment;
        }
        if settings.requested_perception != DEFAULT_VISION_RADIUS {
            sim.config.vision_notch = notch_for_integer(
                settings.requested_perception,
                MIN_VISION_RADIUS,
                MAX_VISION_RADIUS,
            );
        }
    } else {
        // Snap what was asked for to the nearest notch.
        sim.config.vision_notch =
            notch_for_integer(settings.requested_perception, MIN_VISION_RADIUS, MAX_VISION_RADIUS);
    }
    sim.apply_notches();
    // Checked here, so every mode reports a bad sprite the same way.
    if let Some(path) = settings.sprite_path.as_deref() {
        match load_sprite(Some(path), sim.config.shape, &name) {
            Ok(_) => {}
            Err(SpriteError::Fatal(message)) => {
                eprint(&message);
                return Err(EXIT_FAILURE);
            }
            Err(SpriteError::Png(error)) => {
                eprint(&cat(&[
                    &name,
                    b": ",
                    path.as_bytes(),
                    b": ",
                    png_status_string(Err(error)).as_bytes(),
                    b"\n",
                ]));
                return Err(EXIT_FAILURE);
            }
        }
    }
    if let Some(requested) = settings.requested_record_size.as_deref() {
        match record_size(requested.as_bytes()) {
            Some((columns, rows)) => {
                settings.record_columns = columns;
                settings.record_rows = rows;
            }
            None => {
                eprint(&cat(&[
                    &name,
                    b": --record-size wants COLUMNSxROWS, 40x14 to 400x120, not '",
                    requested.as_bytes(),
                    b"'\n",
                ]));
                return Err(EXIT_USAGE);
            }
        }
    }
    if sim.config.flocks == 1 && sim.config.avoid_notch != DEFAULT_NOTCH {
        eprint(&cat(&[&name, b": --avoidance is how flocks avoid each other, and there is one\n"]));
    }
    if sim.config.flocks > 1 && sim.palette_shades() <= 1 {
        eprint(&cat(&[
            &name,
            format!(
                ": {} has one colour, so the {} flocks will look like one\n",
                sim.palette().name,
                sim.config.flocks
            )
            .as_bytes(),
        ]));
    }
    // It is raining birds: every part of it a switch that already existed.
    if settings.matrix_mode {
        sim.config.palette = palette_named("matrix");
        sim.config.trails = true;
        sim.config.alignment_notch = LEGEND_BAR_CELLS;
        sim.rain = true;
        sim.apply_notches();
    }
    Ok(())
}

/// `frame_delay_after`: microseconds left of a sixtieth, or none unlocked.
pub fn frame_delay_after(unlock_fps: bool, elapsed: i64) -> i64 {
    if unlock_fps {
        return 0;
    }
    let remaining = 1_000_000 / i64::from(FRAME_RATE) - elapsed;
    if remaining > 0 { remaining } else { 0 }
}

/// `main`, minus the process exit: the exit code, with `stdout` flushed.
pub fn main(argv: Vec<OsString>) -> i32 {
    crate::platform::default_sigpipe();
    let mut stdout = CStdout::new();
    let mut program = Program::new();
    let code = match read_options(&mut program, &argv, &mut stdout) {
        Err(code) => code,
        Ok(()) => {
            let Program { sim, settings, renderer } = &mut program;
            if settings.bench_frames > 0 {
                crate::bench::run_benchmark(sim, renderer, settings, &mut stdout)
            } else if settings.record_path.is_some() {
                crate::record::run_recording(sim, renderer, settings, &mut stdout)
            } else {
                crate::live::run(&mut program, &mut stdout)
            }
        }
    };
    stdout.flush();
    code
}
