//! Differential tests: `src/options.rs` against the pinned cbirds `options.c`.
//!
//! `tools/oracle/options_oracle.c` runs the unmodified C parser, help printer
//! and completion generator over copies of the option table in `boids.c`, the
//! table in `tests/options_test.c`, a synthetic table for the generic corners
//! and a one row table with a chosen range. This file mirrors each table in
//! Rust, feeds both the same corpus, and compares every status, every byte of
//! the error buffer, every stored value and every byte printed.
//!
//! The corpus goes to one oracle process as a batch; options.c keeps no state
//! and the oracle resets every target before each case. A sample also runs in
//! fresh processes with the raw argument bytes the OS delivers, and must agree.

mod support;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use rbirds::options::{self, Example, Kind, OptionSpec};

// ---------------------------------------------------------------------------
// boids.c: OPTIONS, EXAMPLES and the tagline, over plain targets at boids.c's
// defaults.

const MAX_FLOCKS: f64 = 3.0;
const MAX_HAWKS: f64 = 4.0;
const MIN_BIRD_SIZE: f64 = 4.0;
const MAX_BIRD_SIZE: f64 = 64.0;
const MAX_BIRDS: f64 = 4096.0;
const MAX_CAST_FPS: f64 = 120.0;
const MIN_VISION_RADIUS: f64 = 12.0;
const MAX_VISION_RADIUS: f64 = 60.0;
const LEGEND_BAR_CELLS: f64 = 12.0;
const SLIDERS: &str = "Sliders   0 to 12, as the panel shows them";

#[derive(Debug)]
struct Boids {
    birds: i32,
    bird_size: i32,
    flocks: i32,
    hawks: i32,
    preset: i32,
    seed: i32,
    boundary: i32,
    separation: i32,
    alignment: i32,
    turning: i32,
    perception: i32,
    speed: i32,
    avoidance: i32,
    color: i32,
    shape: i32,
    sprite: Option<OsString>,
    trails: bool,
    depth: bool,
    panel: bool,
    render: i32,
    matrix: bool,
    bench: i32,
    frames: i32,
    snapshot: Option<OsString>,
    record: Option<OsString>,
    record_fps: i32,
    record_seconds: i32,
    record_size: Option<OsString>,
    unlock_fps: bool,
}

impl Default for Boids {
    fn default() -> Boids {
        Boids {
            birds: 800,
            bird_size: 0,
            flocks: 1,
            hawks: 0,
            preset: -1,
            seed: -1,
            boundary: 4,
            separation: 4,
            alignment: 4,
            turning: 8,
            perception: 36,
            speed: 1,
            avoidance: 4,
            color: 0,
            shape: 0,
            sprite: None,
            trails: false,
            depth: false,
            panel: false,
            render: -1,
            matrix: false,
            bench: 0,
            frames: 0,
            snapshot: None,
            record: None,
            record_fps: 25,
            record_seconds: 6,
            record_size: None,
            unlock_fps: false,
        }
    }
}

static PRESET_NAMES: &[&str] = &["murmuration", "swarm", "storm"];
static PALETTE_NAMES: &[&str] =
    &["theme", "ember", "ice", "acid", "matrix", "aurora", "prism", "potion", "dusk", "ash"];
static SHAPE_NAMES: &[&str] = &["bird", "arrow", "plane", "dot"];
static RENDER_NAMES: &[&str] = &["kitty", "braille", "sextants", "blocks"];

/// A row in the order of the C initializer, so the two tables read alike.
#[allow(clippy::too_many_arguments)]
const fn row<T>(
    shorthand: Option<u8>,
    name: &'static str,
    alias: Option<&'static str>,
    kind: Kind<T>,
    metavar: Option<&'static str>,
    help: &'static str,
    group: &'static str,
    essential: bool,
) -> OptionSpec<T> {
    OptionSpec { shorthand, name, alias, kind, metavar, help, group, essential }
}

type B = Boids;

static BOIDS: &[OptionSpec<Boids>] = &[
    row(
        Some(b'n'),
        "birds",
        None,
        Kind::Int { target: |s: &mut B| &mut s.birds, min: 1.0, max: MAX_BIRDS },
        Some("COUNT"),
        "how many birds (default 800)",
        "Flock",
        true,
    ),
    row(
        Some(b's'),
        "size",
        None,
        Kind::Int { target: |s: &mut B| &mut s.bird_size, min: MIN_BIRD_SIZE, max: MAX_BIRD_SIZE },
        Some("PIXELS"),
        "sprite size in pixels (default 30)",
        "Flock",
        true,
    ),
    row(
        Some(b'g'),
        "flocks",
        Some("groups"),
        Kind::Int { target: |s: &mut B| &mut s.flocks, min: 1.0, max: MAX_FLOCKS },
        Some("COUNT"),
        "flocks that keep to their own kind (default 1)",
        "Flock",
        true,
    ),
    row(
        Some(b'k'),
        "hawks",
        None,
        Kind::Int { target: |s: &mut B| &mut s.hawks, min: 0.0, max: MAX_HAWKS },
        Some("COUNT"),
        "predators hunting the flock (default 0)",
        "Flock",
        true,
    ),
    row(
        None,
        "preset",
        None,
        Kind::Enum { target: |s: &mut B| &mut s.preset, names: PRESET_NAMES },
        Some("NAME"),
        "murmuration, swarm, storm",
        "Flock",
        true,
    ),
    row(
        None,
        "seed",
        None,
        Kind::Int { target: |s: &mut B| &mut s.seed, min: 0.0, max: 2147483647.0 },
        Some("N"),
        "the same seed gives the same flock",
        "Flock",
        false,
    ),
    row(
        None,
        "boundary",
        None,
        Kind::Int { target: |s: &mut B| &mut s.boundary, min: 0.0, max: LEGEND_BAR_CELLS },
        Some("NOTCH"),
        "how hard the edges push back (default 4)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "separation",
        None,
        Kind::Int { target: |s: &mut B| &mut s.separation, min: 0.0, max: LEGEND_BAR_CELLS },
        Some("NOTCH"),
        "how much a bird keeps its distance (default 4)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "alignment",
        None,
        Kind::Int { target: |s: &mut B| &mut s.alignment, min: 0.0, max: LEGEND_BAR_CELLS },
        Some("NOTCH"),
        "how much a bird matches its neighbours (default 4)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "turning",
        None,
        Kind::Int { target: |s: &mut B| &mut s.turning, min: 0.0, max: LEGEND_BAR_CELLS },
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
            target: |s: &mut B| &mut s.perception,
            min: MIN_VISION_RADIUS,
            max: MAX_VISION_RADIUS,
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
        Kind::Int { target: |s: &mut B| &mut s.speed, min: 0.0, max: LEGEND_BAR_CELLS },
        Some("NOTCH"),
        "how fast the flock flies, 0.2x to 2.6x (default 1, 0.4x)",
        SLIDERS,
        false,
    ),
    row(
        None,
        "avoidance",
        None,
        Kind::Int { target: |s: &mut B| &mut s.avoidance, min: 0.0, max: LEGEND_BAR_CELLS },
        Some("NOTCH"),
        "how much flocks keep out of each other's way (default 4)",
        SLIDERS,
        false,
    ),
    row(
        Some(b'c'),
        "color",
        Some("palette"),
        Kind::Enum { target: |s: &mut B| &mut s.color, names: PALETTE_NAMES },
        Some("RAMP"),
        "theme, ember, ice, acid, matrix, aurora, prism, potion, dusk, ash",
        "Look",
        true,
    ),
    row(
        None,
        "shape",
        None,
        Kind::Enum { target: |s: &mut B| &mut s.shape, names: SHAPE_NAMES },
        Some("NAME"),
        "bird, arrow, plane, dot",
        "Look",
        true,
    ),
    row(
        None,
        "sprite",
        None,
        Kind::Str(|s: &mut B| &mut s.sprite),
        Some("FILE"),
        "a PNG you supply, kept in its own colours",
        "Look",
        false,
    ),
    row(
        Some(b'e'),
        "trails",
        None,
        Kind::Flag(|s: &mut B| &mut s.trails),
        None,
        "faint tails behind the flock",
        "Look",
        false,
    ),
    row(
        None,
        "depth",
        None,
        Kind::Flag(|s: &mut B| &mut s.depth),
        None,
        "a second sky further off: smaller, slower, dimmer birds",
        "Look",
        true,
    ),
    row(
        Some(b'l'),
        "panel",
        None,
        Kind::Flag(|s: &mut B| &mut s.panel),
        None,
        "the sliders in the corner from the start; h toggles them",
        "Look",
        true,
    ),
    row(
        None,
        "render",
        None,
        Kind::Enum { target: |s: &mut B| &mut s.render, names: RENDER_NAMES },
        Some("HOW"),
        "braille by default; sextants, blocks, or kitty in Kitty and Ghostty",
        "Look",
        true,
    ),
    row(
        None,
        "matrix",
        None,
        Kind::Flag(|s: &mut B| &mut s.matrix),
        None,
        "it is raining birds",
        "Oddities",
        false,
    ),
    row(
        None,
        "bench",
        None,
        Kind::Int { target: |s: &mut B| &mut s.bench, min: 0.0, max: 1000000.0 },
        Some("N"),
        "run N frames with no terminal, print the numbers, quit",
        "Output",
        false,
    ),
    row(
        None,
        "frames",
        None,
        Kind::Int { target: |s: &mut B| &mut s.frames, min: 0.0, max: 1000000.0 },
        Some("N"),
        "quit after N frames, for recording",
        "Output",
        false,
    ),
    row(
        None,
        "snapshot",
        None,
        Kind::Str(|s: &mut B| &mut s.snapshot),
        Some("FILE"),
        "write the last frame as a PNG",
        "Output",
        false,
    ),
    row(
        None,
        "record",
        None,
        Kind::Str(|s: &mut B| &mut s.record),
        Some("FILE"),
        "record a GIF, or a .cast for asciinema, with no terminal, and quit",
        "Output",
        false,
    ),
    row(
        None,
        "record-fps",
        None,
        Kind::Int { target: |s: &mut B| &mut s.record_fps, min: 2.0, max: MAX_CAST_FPS },
        Some("RATE"),
        "frames a second; a GIF can carry up to 50 (default 25)",
        "Output",
        false,
    ),
    row(
        None,
        "record-seconds",
        None,
        Kind::Int { target: |s: &mut B| &mut s.record_seconds, min: 1.0, max: 120.0 },
        Some("SECONDS"),
        "how long the recording runs (default 6)",
        "Output",
        false,
    ),
    row(
        None,
        "record-size",
        None,
        Kind::Str(|s: &mut B| &mut s.record_size),
        Some("COLSxROWS"),
        "the size to record at, in cells (default 96x26)",
        "Output",
        false,
    ),
    row(
        None,
        "unlock-fps",
        None,
        Kind::Flag(|s: &mut B| &mut s.unlock_fps),
        None,
        "render as fast as the terminal allows",
        "General",
        false,
    ),
];

static BOIDS_EXAMPLES: &[Example] = &[
    Example { command: "cbirds", what: "a flock in braille, and nothing to read" },
    Example { command: "cbirds --preset murmuration", what: "the starling look" },
    Example { command: "cbirds --hawks 2 --color ice", what: "something to watch" },
    Example {
        command: "cbirds --flocks 3 --color ember",
        what: "three of them, keeping to their own",
    },
    Example { command: "cbirds --depth --trails", what: "a second sky behind the first" },
    Example { command: "cbirds --render kitty", what: "sprites, in Kitty or Ghostty" },
    Example { command: "cbirds --record flock.gif", what: "a GIF, with no terminal in the way" },
];
const BOIDS_TAGLINE: &str = "cbirds \u{2014} a flock of birds in your terminal.";

impl Observed for Boids {
    fn dump(&self, out: &mut String) {
        int(out, "birds", self.birds);
        int(out, "size", self.bird_size);
        int(out, "flocks", self.flocks);
        int(out, "hawks", self.hawks);
        int(out, "preset", self.preset);
        int(out, "seed", self.seed);
        int(out, "boundary", self.boundary);
        int(out, "separation", self.separation);
        int(out, "alignment", self.alignment);
        int(out, "turning", self.turning);
        int(out, "perception", self.perception);
        int(out, "speed", self.speed);
        int(out, "avoidance", self.avoidance);
        int(out, "color", self.color);
        int(out, "shape", self.shape);
        string(out, "sprite", &self.sprite);
        flag(out, "trails", self.trails);
        flag(out, "depth", self.depth);
        flag(out, "panel", self.panel);
        int(out, "render", self.render);
        flag(out, "matrix", self.matrix);
        int(out, "bench", self.bench);
        int(out, "frames", self.frames);
        string(out, "snapshot", &self.snapshot);
        string(out, "record", &self.record);
        int(out, "record-fps", self.record_fps);
        int(out, "record-seconds", self.record_seconds);
        string(out, "record-size", &self.record_size);
        flag(out, "unlock-fps", self.unlock_fps);
    }
}

// ---------------------------------------------------------------------------
// tests/options_test.c: TABLE, reset() and the usage EXAMPLES; and QUOTED.

#[derive(Debug)]
struct TestTable {
    birds: i32,
    weight: f64,
    quiet: bool,
    mono: bool,
    palette: i32,
    label: Option<OsString>,
}

impl Default for TestTable {
    fn default() -> TestTable {
        TestTable { birds: 800, weight: 0.5, quiet: false, mono: false, palette: 0, label: None }
    }
}

impl Observed for TestTable {
    fn dump(&self, out: &mut String) {
        int(out, "birds", self.birds);
        double(out, "weight", self.weight);
        flag(out, "quiet", self.quiet);
        flag(out, "mono", self.mono);
        int(out, "palette", self.palette);
        string(out, "label", &self.label);
    }
}

type Tt = TestTable;

static TEST_TABLE: &[OptionSpec<TestTable>] = &[
    row(
        Some(b'n'),
        "birds",
        Some("boids"),
        Kind::Int { target: |s: &mut Tt| &mut s.birds, min: 1.0, max: 4096.0 },
        Some("COUNT"),
        "how many boids",
        "Flock",
        true,
    ),
    row(
        Some(b'w'),
        "weight",
        None,
        Kind::Double { target: |s: &mut Tt| &mut s.weight, min: 0.0, max: 1.0 },
        Some("VALUE"),
        "a weight",
        "Flock",
        false,
    ),
    row(
        Some(b'q'),
        "quiet",
        None,
        Kind::Flag(|s: &mut Tt| &mut s.quiet),
        None,
        "say less",
        "Output",
        true,
    ),
    row(
        Some(b'm'),
        "mono",
        None,
        Kind::Flag(|s: &mut Tt| &mut s.mono),
        None,
        "one colour",
        "Output",
        false,
    ),
    row(
        Some(b'P'),
        "palette",
        None,
        Kind::Enum { target: |s: &mut Tt| &mut s.palette, names: &["mono", "flame", "ice"] },
        Some("NAME"),
        "colour scheme",
        "Output",
        false,
    ),
    row(
        None,
        "label",
        None,
        Kind::Str(|s: &mut Tt| &mut s.label),
        Some("TEXT"),
        "a caption",
        "Output",
        false,
    ),
];

static TEST_EXAMPLES: &[Example] = &[
    Example { command: "cbirds -n 1500", what: "a bigger flock" },
    Example { command: "cbirds", what: "the default" },
];
const TEST_TAGLINE: &str = "A flock in your terminal.";

#[derive(Debug, Default)]
struct Quoted {
    shy: bool,
}

impl Observed for Quoted {
    fn dump(&self, out: &mut String) {
        flag(out, "shy", self.shy);
    }
}

static QUOTED: &[OptionSpec<Quoted>] = &[row(
    None,
    "shy",
    None,
    Kind::Flag(|s: &mut Quoted| &mut s.shy),
    None,
    "keeps out of each other's [way] \"$HOME\"",
    "Flock",
    true,
)];

// ---------------------------------------------------------------------------
// The synthetic table: aliases on flags, an off switch, names the special
// switches shadow, shared and odd shorthands, empty and repeated enumeration
// names, repeated groups, bytes outside ASCII, and names either side of the
// lengths the C measures.

#[derive(Debug)]
struct Synthetic {
    fast: bool,
    panel: bool,
    x: bool,
    no_x: bool,
    int: i32,
    double: f64,
    help: i32,
    version: i32,
    aitch: bool,
    vee: bool,
    dup_one: bool,
    dup_two: bool,
    empty: i32,
    choice: i32,
    file: Option<OsString>,
    int_file: i32,
    dash: bool,
    accent: bool,
    utf: i32,
    sixty_three: Option<OsString>,
    sixty_four: bool,
    eighty: Option<OsString>,
    eighty_eight: Option<OsString>,
    hundred: bool,
    blank: bool,
}

impl Default for Synthetic {
    fn default() -> Synthetic {
        Synthetic {
            fast: false,
            panel: true,
            x: true,
            no_x: true,
            int: 2,
            double: 0.5,
            help: 0,
            version: 0,
            aitch: false,
            vee: false,
            dup_one: false,
            dup_two: false,
            empty: -1,
            choice: -1,
            file: None,
            int_file: 0,
            dash: false,
            accent: false,
            utf: 0,
            sixty_three: None,
            sixty_four: false,
            eighty: None,
            eighty_eight: None,
            hundred: false,
            blank: true,
        }
    }
}

impl Observed for Synthetic {
    fn dump(&self, out: &mut String) {
        flag(out, "fast", self.fast);
        flag(out, "panel", self.panel);
        flag(out, "x", self.x);
        flag(out, "no-x", self.no_x);
        int(out, "int", self.int);
        double(out, "double", self.double);
        int(out, "help", self.help);
        int(out, "version", self.version);
        flag(out, "aitch", self.aitch);
        flag(out, "vee", self.vee);
        flag(out, "dup-one", self.dup_one);
        flag(out, "dup-two", self.dup_two);
        int(out, "empty", self.empty);
        int(out, "choice", self.choice);
        string(out, "file", &self.file);
        int(out, "int-file", self.int_file);
        flag(out, "dash", self.dash);
        flag(out, "accent", self.accent);
        int(out, "utf", self.utf);
        string(out, "sixty-three", &self.sixty_three);
        flag(out, "sixty-four", self.sixty_four);
        string(out, "eighty", &self.eighty);
        string(out, "eighty-eight", &self.eighty_eight);
        flag(out, "one-hundred", self.hundred);
        flag(out, "blank", self.blank);
    }
}

type Sy = Synthetic;

const NAME_63: &str = "sixty-three-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm";
const NAME_64: &str = "sixty-four-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-n";
const NAME_80: &str =
    "eighty-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-nnn-ooo-ppp-qqq-rrr-s";
const NAME_88: &str =
    "eighty-eight-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-nnn-ooo-ppp-qqq-rrr-sss";
const NAME_100: &str = "one-hundred-aaa-bbb-ccc-ddd-eee-fff-ggg-hhh-iii-jjj-kkk-lll-mmm-nnn-ooo-ppp-qqq-rrr-sss-ttt-uuu-vvvw";

static SYNTHETIC: &[OptionSpec<Synthetic>] = &[
    row(
        Some(b'f'),
        "fast",
        Some("quick"),
        Kind::Flag(|s: &mut Sy| &mut s.fast),
        Some("IGNORED"),
        "go fast",
        "Alpha",
        true,
    ),
    row(
        Some(b'o'),
        "no-panel",
        Some("hide"),
        Kind::Off(|s: &mut Sy| &mut s.panel),
        Some("IGNORED"),
        "hide the panel",
        "Alpha",
        false,
    ),
    row(None, "x", None, Kind::Flag(|s: &mut Sy| &mut s.x), None, "ex", "Beta", true),
    row(None, "no-x", None, Kind::Flag(|s: &mut Sy| &mut s.no_x), None, "no ex", "Beta", false),
    row(
        Some(b'i'),
        "int",
        Some("integer"),
        Kind::Int { target: |s: &mut Sy| &mut s.int, min: -5.0, max: 5.0 },
        None,
        "an int",
        "Alpha",
        true,
    ),
    row(
        Some(b'd'),
        "double",
        None,
        Kind::Double { target: |s: &mut Sy| &mut s.double, min: -2.5, max: 1e6 },
        Some("REAL"),
        "a double",
        "Gamma",
        false,
    ),
    row(
        None,
        "help",
        None,
        Kind::Int { target: |s: &mut Sy| &mut s.help, min: 0.0, max: 9.0 },
        Some("N"),
        "shadowed by --help",
        "Gamma",
        true,
    ),
    row(
        None,
        "version",
        None,
        Kind::Int { target: |s: &mut Sy| &mut s.version, min: 0.0, max: 9.0 },
        Some("N"),
        "shadowed by --version",
        "General",
        true,
    ),
    row(
        Some(b'h'),
        "aitch",
        None,
        Kind::Flag(|s: &mut Sy| &mut s.aitch),
        None,
        "never reached as -h",
        "General",
        false,
    ),
    row(
        Some(b'V'),
        "vee",
        None,
        Kind::Flag(|s: &mut Sy| &mut s.vee),
        None,
        "never reached as -V",
        "Gamma",
        true,
    ),
    row(
        Some(b'D'),
        "dup-one",
        None,
        Kind::Flag(|s: &mut Sy| &mut s.dup_one),
        None,
        "first D",
        "Gamma",
        true,
    ),
    row(
        Some(b'D'),
        "dup-two",
        None,
        Kind::Flag(|s: &mut Sy| &mut s.dup_two),
        None,
        "second D",
        "Gamma",
        true,
    ),
    row(
        Some(b'e'),
        "empty",
        None,
        Kind::Enum { target: |s: &mut Sy| &mut s.empty, names: &[] },
        Some("NONE"),
        "no choices",
        "Gamma",
        true,
    ),
    row(
        Some(b'c'),
        "choice",
        Some("pick"),
        Kind::Enum {
            target: |s: &mut Sy| &mut s.choice,
            names: &["alpha", "beta", "", "alpha", "Gamma"],
        },
        Some("FILE"),
        "choose [one] 'of' \"them\" $x \\ back",
        "D\u{e9}lta",
        true,
    ),
    row(
        Some(b'F'),
        "file",
        None,
        Kind::Str(|s: &mut Sy| &mut s.file),
        Some("FILE"),
        "a file",
        "D\u{e9}lta",
        false,
    ),
    row(
        None,
        "int-file",
        None,
        Kind::Int { target: |s: &mut Sy| &mut s.int_file, min: 0.0, max: 100.0 },
        Some("FILE"),
        "a number called FILE",
        "D\u{e9}lta",
        false,
    ),
    row(
        Some(b'-'),
        "dash",
        None,
        Kind::Flag(|s: &mut Sy| &mut s.dash),
        None,
        "a dash",
        "Epsilon",
        true,
    ),
    row(
        Some(0xe9),
        "accent",
        None,
        Kind::Flag(|s: &mut Sy| &mut s.accent),
        None,
        "an \u{e9}",
        "Epsilon",
        true,
    ),
    row(
        Some(b'u'),
        "utf",
        None,
        Kind::Int { target: |s: &mut Sy| &mut s.utf, min: 0.0, max: 9.0 },
        Some("\u{c9}CHELLE"),
        "help \u{2014} with \u{fc}n\u{ef}code",
        "Epsilon",
        true,
    ),
    row(
        None,
        NAME_63,
        None,
        Kind::Str(|s: &mut Sy| &mut s.sixty_three),
        Some("M\u{c9}TA"),
        "sixty-three bytes",
        "Long",
        false,
    ),
    row(
        None,
        NAME_64,
        None,
        Kind::Flag(|s: &mut Sy| &mut s.sixty_four),
        None,
        "sixty-four bytes",
        "Long",
        true,
    ),
    row(
        None,
        NAME_80,
        None,
        Kind::Str(|s: &mut Sy| &mut s.eighty),
        Some("LONGMETAVAR"),
        "eighty bytes",
        "Long",
        false,
    ),
    row(
        None,
        NAME_88,
        None,
        Kind::Str(|s: &mut Sy| &mut s.eighty_eight),
        Some("CUT"),
        "eighty-eight bytes",
        "Long",
        true,
    ),
    row(
        None,
        NAME_100,
        None,
        Kind::Flag(|s: &mut Sy| &mut s.hundred),
        None,
        "a hundred bytes",
        "Long",
        false,
    ),
    row(
        None,
        "blank",
        Some(""),
        Kind::Flag(|s: &mut Sy| &mut s.blank),
        None,
        "an empty alias",
        "Long",
        false,
    ),
];

static SYNTHETIC_EXAMPLES: &[Example] = &[
    Example { command: "prog --fast", what: "quickly" },
    Example { command: "prog --choice \u{e9}", what: "an accent, padded by bytes" },
    Example { command: "", what: "nothing at all" },
];
const SYNTHETIC_TAGLINE: &str = "Synthetic \u{2014} corners.";

// ---------------------------------------------------------------------------
// The one row table "x" with a chosen range.

#[derive(Debug)]
struct Ranged {
    int: i32,
    double: f64,
}

impl Default for Ranged {
    fn default() -> Ranged {
        Ranged { int: 7, double: 0.5 }
    }
}

fn ranged_row(integer: bool, min: f64, max: f64) -> OptionSpec<Ranged> {
    let kind = if integer {
        Kind::Int { target: |s: &mut Ranged| &mut s.int, min, max }
    } else {
        Kind::Double { target: |s: &mut Ranged| &mut s.double, min, max }
    };
    row(None, "x", None, kind, Some("N"), "a number", "G", true)
}

// ---------------------------------------------------------------------------
// What both sides print.

trait Observed: Default {
    fn dump(&self, out: &mut String);
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").unwrap();
    }
    text
}

fn x(bytes: &[u8]) -> String {
    format!("x{}", hex(bytes))
}

fn int(out: &mut String, name: &str, value: i32) {
    writeln!(out, "{name} {value}").unwrap();
}

fn flag(out: &mut String, name: &str, value: bool) {
    int(out, name, i32::from(value));
}

fn double(out: &mut String, name: &str, value: f64) {
    writeln!(out, "{name} {:016x}", value.to_bits()).unwrap();
}

fn string(out: &mut String, name: &str, value: &Option<OsString>) {
    match value {
        None => writeln!(out, "{name} null").unwrap(),
        Some(text) => writeln!(out, "{name} s:{}", hex(text.as_bytes())).unwrap(),
    }
}

/// A readable rendering of an argument list for failure reports.
fn show(args: &[Vec<u8>]) -> String {
    let shown: Vec<String> =
        args.iter().map(|a| a.escape_ascii().to_string()).map(|a| format!("\"{a}\"")).collect();
    format!("[{}]", shown.join(", "))
}

fn argv(args: &[Vec<u8>]) -> Vec<OsString> {
    let mut argv = vec![OsString::from("cbirds")];
    argv.extend(args.iter().map(|a| OsString::from_vec(a.clone())));
    argv
}

fn rust_parse<T: Observed>(table: &[OptionSpec<T>], size: usize, args: &[Vec<u8>]) -> String {
    let mut settings = T::default();
    let parsed = options::parse(table, &mut settings, &argv(args), size);
    let mut out = String::new();
    let status = parsed.status;
    writeln!(out, "status {} {}", status as i32, options::options_status_string(status)).unwrap();
    writeln!(out, "message {}", hex(&parsed.message)).unwrap();
    settings.dump(&mut out);
    out.push_str("end\n");
    out
}

fn rust_usage<T>(
    table: &[OptionSpec<T>],
    everything: bool,
    program: &[u8],
    tagline: Option<&str>,
    examples: Option<&[Example]>,
) -> String {
    let mut text = Vec::new();
    options::usage(&mut text, program, tagline, examples, table, everything).unwrap();
    format!("usage {}\nend\n", hex(&text))
}

fn rust_completion<T>(table: &[OptionSpec<T>], shell: &[u8], program: &str) -> String {
    let mut text = Vec::new();
    let known = options::completion(&mut text, shell, program, table).unwrap();
    format!("return {}\noutput {}\nend\n", i32::from(known), hex(&text))
}

/// A table as both sides know it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Table {
    Boids,
    Test,
    Synthetic,
    Quoted,
}

impl Table {
    fn letter(self) -> &'static str {
        match self {
            Table::Boids => "B",
            Table::Test => "T",
            Table::Synthetic => "X",
            Table::Quoted => "Q",
        }
    }

    fn parse(self, size: usize, args: &[Vec<u8>]) -> String {
        match self {
            Table::Boids => rust_parse(BOIDS, size, args),
            Table::Test => rust_parse(TEST_TABLE, size, args),
            Table::Synthetic => rust_parse(SYNTHETIC, size, args),
            Table::Quoted => rust_parse(QUOTED, size, args),
        }
    }

    fn usage(
        self,
        everything: bool,
        program: &[u8],
        tagline: Option<&str>,
        examples: Option<&[Example]>,
    ) -> String {
        match self {
            Table::Boids => rust_usage(BOIDS, everything, program, tagline, examples),
            Table::Test => rust_usage(TEST_TABLE, everything, program, tagline, examples),
            Table::Synthetic => rust_usage(SYNTHETIC, everything, program, tagline, examples),
            Table::Quoted => rust_usage(QUOTED, everything, program, tagline, examples),
        }
    }

    fn completion(self, shell: &[u8], program: &str) -> String {
        match self {
            Table::Boids => rust_completion(BOIDS, shell, program),
            Table::Test => rust_completion(TEST_TABLE, shell, program),
            Table::Synthetic => rust_completion(SYNTHETIC, shell, program),
            Table::Quoted => rust_completion(QUOTED, shell, program),
        }
    }

    fn examples(self) -> Option<&'static [Example]> {
        match self {
            Table::Boids => Some(BOIDS_EXAMPLES),
            Table::Test => Some(TEST_EXAMPLES),
            Table::Synthetic => Some(SYNTHETIC_EXAMPLES),
            Table::Quoted => None,
        }
    }

    fn tagline(self) -> Option<&'static str> {
        match self {
            Table::Boids => Some(BOIDS_TAGLINE),
            Table::Test => Some(TEST_TAGLINE),
            Table::Synthetic => Some(SYNTHETIC_TAGLINE),
            Table::Quoted => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The batch: one oracle command and the Rust answer for each case.

struct Case {
    label: String,
    command: String,
    expected: String,
}

#[derive(Default)]
struct Batch {
    cases: Vec<Case>,
}

impl Batch {
    fn parse(&mut self, table: Table, size: usize, args: &[Vec<u8>]) {
        for arg in args {
            assert!(!arg.contains(&0), "an argv string cannot hold NUL: {}", show(args));
        }
        let mut command = format!("p {} {size}", table.letter());
        for arg in args {
            command.push(' ');
            command.push_str(&x(arg));
        }
        self.cases.push(Case {
            label: format!("parse {table:?} size {size} {}", show(args)),
            command,
            expected: table.parse(size, args),
        });
    }

    /// A case whose Rust arguments hold a NUL, which no C `argv` can: the C is
    /// given what it would see, each argument up to its first NUL.
    fn parse_with_nul(&mut self, table: Table, size: usize, rust_args: &[Vec<u8>]) {
        let c_args: Vec<Vec<u8>> = rust_args
            .iter()
            .map(|a| a.split(|&b| b == 0).next().unwrap_or_default().to_vec())
            .collect();
        let mut command = format!("p {} {size}", table.letter());
        for arg in &c_args {
            command.push(' ');
            command.push_str(&x(arg));
        }
        self.cases.push(Case {
            label: format!("parse {table:?} size {size} {} as {}", show(rust_args), show(&c_args)),
            command,
            expected: table.parse(size, rust_args),
        });
    }

    fn ranged(&mut self, integer: bool, min: f64, max: f64, size: usize, args: &[Vec<u8>]) {
        let mut command = format!(
            "r {} {:016x} {:016x} {size}",
            if integer { "i" } else { "d" },
            min.to_bits(),
            max.to_bits()
        );
        for arg in args {
            command.push(' ');
            command.push_str(&x(arg));
        }
        let table = [ranged_row(integer, min, max)];
        let mut settings = Ranged::default();
        let parsed = options::parse(&table, &mut settings, &argv(args), size);
        let mut expected = String::new();
        let status = parsed.status;
        writeln!(expected, "status {} {}", status as i32, options::options_status_string(status))
            .unwrap();
        writeln!(expected, "message {}", hex(&parsed.message)).unwrap();
        if integer {
            int(&mut expected, "x", settings.int);
        } else {
            double(&mut expected, "x", settings.double);
        }
        expected.push_str("end\n");
        self.cases.push(Case {
            label: format!(
                "range {} [{min:e}, {max:e}] size {size} {}",
                if integer { "int" } else { "double" },
                show(args)
            ),
            command,
            expected,
        });
    }

    /// `tagline`: `None` is NULL; `Some(None)` the table's; `Some(Some(t))` t.
    /// `examples`: 0 NULL, 1 the table's, 2 an empty list.
    fn usage(
        &mut self,
        table: Table,
        everything: bool,
        program: &[u8],
        tagline: Option<Option<&str>>,
        examples: u8,
    ) {
        let (tag_token, tag) = match tagline {
            None => ("-".to_owned(), None),
            Some(None) => ("=".to_owned(), table.tagline()),
            Some(Some(text)) => (x(text.as_bytes()), Some(text)),
        };
        let list = match examples {
            0 => None,
            1 => table.examples(),
            _ => Some(&[][..]),
        };
        self.cases.push(Case {
            label: format!(
                "usage {table:?} everything {everything} program {} tagline {tag:?} examples {examples}",
                program.escape_ascii()
            ),
            command: format!(
                "u {} {} {} {tag_token} {examples}",
                table.letter(),
                i32::from(everything),
                x(program)
            ),
            expected: table.usage(everything, program, tag, list),
        });
    }

    fn completion(&mut self, table: Table, shell: &[u8], program: &str) {
        self.cases.push(Case {
            label: format!(
                "completion {table:?} shell {} program {program:?}",
                shell.escape_ascii()
            ),
            command: format!("c {} {} {}", table.letter(), x(shell), x(program.as_bytes())),
            expected: table.completion(shell, program),
        });
    }

    /// Runs every case through the oracle and fails, listing the first few
    /// divergences and their first differing line, unless all agree.
    fn check(self, exe: &Path) -> usize {
        let mut input = String::new();
        for case in &self.cases {
            input.push_str(&case.command);
            input.push('\n');
        }
        let output = support::oracle::run(exe, &[], input.as_bytes());
        let stdout = String::from_utf8(output.stdout).expect("the oracle prints ASCII");
        let answers: Vec<&str> = stdout.split_inclusive("end\n").collect();
        assert_eq!(answers.len(), self.cases.len(), "one oracle answer per case");
        let mut failures = Vec::new();
        for (case, answer) in self.cases.iter().zip(&answers) {
            if case.expected != *answer {
                let first = case
                    .expected
                    .lines()
                    .zip(answer.lines())
                    .find(|(rust, c)| rust != c)
                    .map(|(rust, c)| format!("  rust: {rust}\n     c: {c}"))
                    .unwrap_or_else(|| "  (one side has extra lines)".to_owned());
                failures.push(format!("{}\n{first}", case.label));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} cases differ from the C; the first:\n{}",
            failures.len(),
            self.cases.len(),
            failures.iter().take(12).cloned().collect::<Vec<_>>().join("\n")
        );
        self.cases.len()
    }
}

fn oracle() -> Option<PathBuf> {
    support::oracle::build("options_oracle", "options_oracle.c", &["options.c"])
}

// ---------------------------------------------------------------------------
// The corpus.

fn args(list: &[&[u8]]) -> Vec<Vec<u8>> {
    list.iter().map(|a| a.to_vec()).collect()
}

fn joined(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// A number as an argument: integers without a point, others exactly.
fn number_text(value: f64) -> Vec<u8> {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64).into_bytes()
    } else {
        format!("{value:e}").into_bytes()
    }
}

/// Every row in every spelling: long, `=`, alias, short, attached, negated,
/// with values at and past its bounds, and missing.
fn every_form<T>(table: &[OptionSpec<T>]) -> Vec<Vec<Vec<u8>>> {
    let mut corpus = Vec::new();
    for option in table {
        let mut longs = vec![option.name.as_bytes().to_vec()];
        if let Some(alias) = option.alias {
            longs.push(alias.as_bytes().to_vec());
        }
        let values: Vec<Vec<u8>> = match &option.kind {
            Kind::Flag(_) | Kind::Off(_) => Vec::new(),
            Kind::Int { min, max, .. } => {
                let (min, max) = (*min, *max);
                let mut values: Vec<Vec<u8>> = [min - 1.0, min, max, max + 1.0, (min + max) / 2.0]
                    .iter()
                    .map(|&v| number_text(v.trunc()))
                    .collect();
                values.push(format!("{}.0", min as i64).into_bytes());
                values.push(format!("{}.5", min as i64).into_bytes());
                values.push(format!("{:e}", max).into_bytes());
                values.push(format!("{:e}", min.next_down()).into_bytes());
                values.push(format!("{:e}", max.next_up()).into_bytes());
                values
            }
            Kind::Double { min, max, .. } => {
                let (min, max) = (*min, *max);
                [min - 1.0, min, max, max + 1.0, (min + max) / 2.0, min.next_down(), max.next_up()]
                    .iter()
                    .map(|v| format!("{v:e}").into_bytes())
                    .chain([format!("{min}").into_bytes(), format!("{max}").into_bytes()])
                    .collect()
            }
            Kind::Enum { names, .. } => {
                let mut values: Vec<Vec<u8>> =
                    names.iter().map(|n| n.as_bytes().to_vec()).collect();
                if let Some(first) = names.first() {
                    values.push(first.to_uppercase().into_bytes());
                    values.push(first.as_bytes()[..first.len().min(3)].to_vec());
                    values.push(joined(&[first.as_bytes(), b" "]));
                    values.push(joined(&[b" ", first.as_bytes()]));
                    values.push(joined(&[first.as_bytes(), b"\xff"]));
                }
                values.extend([b"bogus".to_vec(), Vec::new(), b"\xff".to_vec(), b"-".to_vec()]);
                values
            }
            Kind::Str(_) => {
                [&b"flock.png"[..], b"", b"-", b"--help", b"-h", b"\xff\xfe", b"a b", b"=x", b"--"]
                    .iter()
                    .map(|v| v.to_vec())
                    .collect()
            }
        };
        let short = option.shorthand.filter(|&c| c != 0);
        match &option.kind {
            Kind::Flag(_) | Kind::Off(_) => {
                for long in &longs {
                    let dashed = joined(&[b"--", long]);
                    let negated = joined(&[b"--no-", long]);
                    corpus.push(vec![dashed.clone()]);
                    corpus.push(vec![negated.clone()]);
                    corpus.push(vec![joined(&[&dashed, b"=1"])]);
                    corpus.push(vec![joined(&[&dashed, b"="])]);
                    corpus.push(vec![joined(&[&negated, b"="])]);
                    corpus.push(vec![joined(&[&negated, b"=0"])]);
                    corpus.push(vec![dashed.clone(), negated.clone()]);
                    corpus.push(vec![negated.clone(), dashed.clone()]);
                    corpus.push(vec![dashed.clone(), b"1".to_vec()]);
                    corpus.push(vec![joined(&[b"--no-no-", long])]);
                }
                if let Some(letter) = short {
                    corpus.push(vec![vec![b'-', letter]]);
                    corpus.push(vec![vec![b'-', letter, letter]]);
                    corpus.push(vec![vec![b'-', letter, b'=']]);
                    corpus.push(vec![
                        vec![b'-', letter],
                        b"--no-".iter().chain(longs[0].iter()).copied().collect(),
                    ]);
                }
            }
            _ => {
                for long in &longs {
                    let dashed = joined(&[b"--", long]);
                    for value in &values {
                        corpus.push(vec![dashed.clone(), value.clone()]);
                        corpus.push(vec![joined(&[&dashed, b"=", value])]);
                    }
                    corpus.push(vec![dashed.clone()]);
                    corpus.push(vec![b"-e".to_vec(), dashed.clone()]);
                    corpus.push(vec![joined(&[&dashed, b"="])]);
                    corpus.push(vec![joined(&[b"--no-", long])]);
                    corpus.push(vec![joined(&[b"--no-", long, b"=1"])]);
                    corpus.push(vec![dashed.clone(), b"--help".to_vec()]);
                    corpus.push(vec![dashed.clone(), b"--".to_vec()]);
                }
                if let Some(letter) = short {
                    for value in &values {
                        corpus.push(vec![vec![b'-', letter], value.clone()]);
                        if !value.is_empty() {
                            corpus.push(vec![joined(&[&[b'-', letter], value])]);
                        }
                    }
                    corpus.push(vec![vec![b'-', letter]]);
                    corpus.push(vec![vec![b'-', letter], b"-h".to_vec()]);
                    corpus.push(vec![vec![b'-', letter, b'=', b'1']]);
                }
            }
        }
    }
    corpus
}

/// Numbers as `strtod` sees them: whitespace, signs, hexadecimal, exponents,
/// overflow and underflow, NaN and infinity, and the trailing bytes it stops at.
const NUMERIC: &[&[u8]] = &[
    b" 12",
    b"+12",
    b"-0",
    b"0",
    b"12.0",
    b"1.2e1",
    b"0x10",
    b"0x1p4",
    b"0X1P4",
    b"1e400",
    b"-1e400",
    b"1e-400",
    b"1e-310",
    b"inf",
    b"-inf",
    b"INF",
    b"nan",
    b"-nan",
    b"NaN(1)",
    b"nan(0x7)",
    b"infinity",
    b"12abc",
    b"",
    b"12 ",
    b"\t12",
    b"\n12",
    b"\x0b12",
    b"\x0c12",
    b"\r12",
    b"  +0x1.8p3",
    b".5e1",
    b"5.",
    b".",
    b"e5",
    b"0x",
    b"0x.p1",
    b"0xp1",
    b"1_000",
    "\u{ff11}\u{ff12}".as_bytes(),
    b"12\xff",
    b"\xff12",
    b"2147483647",
    b"2147483648",
    b"-1",
    b"-2147483648",
    b"4096.0000000000001",
    b"4095.9999999999995",
    b"4096.000000000001",
    b"4095.99999999999",
    b"1e3",
    b"0.0e0",
    b"00012",
    b"0b101",
    b"1,5",
    b"0x7fffffff",
    b"2147483647.0",
    b"2147483647.5",
    b"2147483646.5",
    b"-0.0",
    b"+-1",
    b"--1",
    b"4e3",
    b"1E3",
    b"0x1000",
    b"0x1001",
    b"12e",
    b"12e+",
    b"12e+1",
    b"1.5",
    b"2.5",
    b"0.9999999999999999",
    b"1.0000000000000002",
    b"3.0000000000000004",
    b"0x1.fffffffffffffp1",
    b"1e-5",
    b"12\n",
    b" ",
    b"+",
    b"-",
    b"0x-1",
    b"-0x10",
    b"012",
    b"1e+0",
    b"1.e1",
    b"100000e-5",
    b"0.01e2",
    b"1e1000000000000",
    b"0e1000000000000",
    b"1e-1000000000000",
    b"0x1p-1074",
    b"0x1p-1080",
    b"0x1p1024",
    b"0x1p1023",
    b"1000001",
    b"999999.99999999999",
    b"5e-324",
    b"2.2250738585072011e-308",
    b"1.7976931348623157e308",
    b"1.7976931348623159e308",
];

/// Clusters of short flags, ending in values attached, detached and missing.
const CLUSTERS: &[&[&[u8]]] = &[
    &[b"-elk2"],
    &[b"-le"],
    &[b"-lec", b"ice"],
    &[b"-lecice"],
    &[b"-elhk"],
    &[b"-ekV"],
    &[b"-en"],
    &[b"-e-"],
    &[b"-ee"],
    &[b"-lz"],
    &[b"-n800e"],
    &[b"-k2e"],
    &[b"-elk"],
    &[b"-ek", b"-1"],
    &[b"-lce"],
    &[b"-lcbogus"],
    &[b"-eln0"],
    &[b"-eln4097"],
    &[b"-elg3"],
    &[b"-elg4"],
    &[b"-els"],
    &[b"-els64"],
    &[b"-gkn"],
    &[b"-lV"],
    &[b"-Vl"],
    &[b"-hl"],
    &[b"-ln-5"],
    &[b"-n-5"],
    &[b"-n+5"],
    &[b"-n", b"5", b"-e"],
    &[b"-e5"],
    &[b"-5"],
    &[b"-l="],
    &[b"-e=1"],
    &[b"-g2k3"],
    &[b"-sn"],
    &[b"-s-n"],
    &[b"-nsize"],
    &[b"-e", b"-l", b"-k", b"3"],
    &[b"-elc", b"--help"],
    &[b"-elc"],
    &[b"-cemerald"],
    &[b"-cice", b"-e"],
    &[b"-n", b"-e"],
    &[b"-k", b"-h"],
    &[b"-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeel"],
    &[b"-l\xff"],
    &[b"-e\xc3\xa9"],
];

const UNKNOWN: &[&[&[u8]]] = &[
    &[b"--birdz"],
    &[b"--bird"],
    &[b"--birdss"],
    &[b"--brids"],
    &[b"--sed"],
    &[b"--seeed"],
    &[b"--sped"],
    &[b"--prest"],
    &[b"--colour"],
    &[b"--colr"],
    &[b"--pallete"],
    &[b"--palete"],
    &[b"--x"],
    &[b"--xyzzy"],
    &[b"--flcks"],
    &[b"--flock"],
    &[b"--recordfps"],
    &[b"--record_fps"],
    &[b"--recrod-fps"],
    &[b"--record-fp"],
    &[b"--recordsize"],
    &[b"--record-sizes"],
    &[b"--record-second"],
    &[b"--no-bird"],
    &[b"--no-birds"],
    &[b"--no-"],
    &[b"--no"],
    &[b"--no-trail"],
    &[b"--trail"],
    &[b"--trailz"],
    &[b"--no-depth"],
    &[b"--no-panel"],
    &[b"--no-matrix"],
    &[b"--no-unlock-fps"],
    &[b"--no-unlock"],
    &[b"--unlock"],
    &[b"--frame"],
    &[b"--hawk"],
    &[b"--shap"],
    &[b"--matrx"],
    &[b"--deth"],
    &[b"--pane"],
    &[b"--panels"],
    &[b"--rendr"],
    &[b"--snap"],
    &[b"--snapshots"],
    &[b"--seconds"],
    &[b"--fps"],
    &[b"--sizes"],
    &[b"--sie"],
    &[b"--siz"],
    &[b"--a"],
    &[b"--ab"],
    &[b"--abc"],
    &[b"--help=1"],
    &[b"--help="],
    &[b"--version=1"],
    &[b"--completion=bash"],
    &[b"--Help"],
    &[b"--HELP"],
    &[b"--VERSION"],
    &[b"--Birds"],
    &[b"---birds"],
    &[b"----"],
    &[b"---"],
    &[b"--=5"],
    &[b"--="],
    &[b"--no-="],
    &[b"--no-birds=5"],
    &[b"--group"],
    &[b"--palettes"],
    &[b"--no-groups"],
    &[b"--no-palette"],
    &[b"--no-color"],
    &[b"--bird", b"10"],
    &[b"--birdz=10"],
    &[b"--h"],
    &[b"--V"],
    &[b"--e"],
    &[b"--n"],
    &[b"-z"],
    &[b"-x"],
    &[b"-N"],
    &[b"-H"],
    &[b"-v"],
    &[b"-S"],
    &[b"-E"],
    &[b"-L"],
    &[b"-K"],
    &[b"-G"],
    &[b"-C"],
    &[b"-?"],
    &[b"-="],
    &[b"-version"],
    &[b"-completion"],
    &[b"-birds"],
];

const SPECIALS: &[&[&[u8]]] = &[
    &[b"-h"],
    &[b"--help"],
    &[b"-V"],
    &[b"--version"],
    &[b"--completion", b"bash"],
    &[b"--completion", b"zsh"],
    &[b"--completion", b"fish"],
    &[b"--completion"],
    &[b"--completion", b""],
    &[b"--completion", b"tcsh"],
    &[b"--completion", b"bash zsh"],
    &[b"-help"],
    &[b"-hV"],
    &[b"-Vh"],
    &[b"-eh"],
    &[b"-nh"],
    &[b"-hh"],
    &[b"-h="],
    &[b"--birds", b"--help"],
    &[b"--help", b"--birds", b"x"],
    &[b"--birds", b"x", b"--help"],
    &[b"--birds", b"10", b"--help"],
    &[b"--", b"--help"],
    &[b"--help", b"--"],
    &[b"--completion", b"--help"],
    &[b"--completion", b"bash", b"--help"],
    &[b"--birds=10", b"--completion", b"zsh"],
    &[b"-h", b"-z"],
    &[b"-z", b"-h"],
    &[b"--sprite", b"--help"],
    &[b"--sprite", b"-h"],
    &[b"--sprite", b"--completion"],
    &[b"-V", b"--completion"],
    &[b"--completion", b"-V"],
    &[b"--version", b"--help"],
    &[b"--help", b"--version"],
    &[b"-n", b"5", b"-V"],
    &[b"-n", b"5", b"--version", b"x"],
    &[b"--help", b"x", b"y", b"z"],
    &[b"x", b"--help"],
    &[b"--sprite=x", b"-h"],
    &[b"-e", b"-h", b"-V"],
    &[b"-e", b"--no-trails", b"--completion", b"fish"],
    &[b"--color", b"--version"],
    &[b"--completion", b"--completion"],
    &[b"--completion", b"bash", b"--bogus"],
    &[b"--bogus", b"--completion", b"bash"],
    &[b"--", b"--completion", b"bash"],
    &[b"-cV"],
    &[b"-ch"],
    &[b"-nV"],
];

const POSITIONAL: &[&[&[u8]]] = &[
    &[b"flock.png"],
    &[b""],
    &[b"-"],
    &[b"--"],
    &[b"--", b"x"],
    &[b"--", b"--"],
    &[b"--", b""],
    &[b"--birds", b"10", b"--"],
    &[b"--birds", b"10", b"--", b"x"],
    &[b"x", b"y"],
    &[b" "],
    &[b"-", b"-"],
    &[b"--birds", b"10", b"extra"],
    &[b"--sprite", b"a", b"b"],
    &[b"\xff"],
    &[b"+n"],
    &[b"n800"],
    &[b" -n"],
    &[b"--", b"--", b"--"],
    &[b"-e", b""],
];

const REPEATS: &[&[&[u8]]] = &[
    &[b"-n", b"5", b"-n", b"6"],
    &[b"--color", b"ice", b"--palette", b"ember"],
    &[b"-e", b"--no-trails", b"-e"],
    &[b"-e", b"--no-trails"],
    &[b"--sprite", b"a", b"--sprite", b"b"],
    &[b"--preset", b"storm", b"--preset", b"swarm"],
    &[b"-n", b"5", b"--bogus"],
    &[b"-e", b"-l", b"--birds", b"x"],
    &[b"--color", b"ice", b"-k", b"9"],
    &[b"--seed", b"1", b"--seed", b"2", b"--seed", b"x"],
    &[b"--record", b"a.gif", b"--record-fps", b"30", b"--record-seconds", b"0"],
    &[b"--flocks", b"2", b"--groups", b"3"],
    &[b"--groups=0"],
    &[b"--sprite", b"a", b"--sprite="],
    &[b"--render", b"kitty", b"--render", b"blocks", b"--render", b"sixel"],
    &[
        b"-n5",
        b"-s10",
        b"-g2",
        b"-k3",
        b"--preset",
        b"swarm",
        b"--seed",
        b"42",
        b"--boundary",
        b"7",
        b"--separation=8",
        b"--alignment",
        b"9",
        b"--turning",
        b"10",
        b"--perception=40",
        b"--speed",
        b"11",
        b"--avoidance",
        b"12",
        b"-cdusk",
        b"--shape",
        b"dot",
        b"--sprite",
        b"s.png",
        b"-e",
        b"--depth",
        b"-l",
        b"--render",
        b"sextants",
        b"--matrix",
        b"--bench",
        b"100",
        b"--frames=200",
        b"--snapshot",
        b"snap.png",
        b"--record",
        b"r.cast",
        b"--record-fps",
        b"60",
        b"--record-seconds",
        b"30",
        b"--record-size",
        b"80x24",
        b"--unlock-fps",
    ],
    &[
        b"-n5",
        b"-s10",
        b"-g2",
        b"-k3",
        b"--preset",
        b"swarm",
        b"--seed",
        b"42",
        b"-cdusk",
        b"--sprite",
        b"s.png",
        b"-e",
        b"--record-size",
        b"80x24",
        b"--unlock-fps",
        b"--frames",
        b"1000001",
    ],
    &[b"--depth", b"--no-depth", b"--depth", b"--no-depth"],
    &[b"-l", b"--no-panel", b"-l"],
];

const RAW_BYTES: &[&[&[u8]]] = &[
    &[b"--birds\xff"],
    &[b"--sprite=\xff\xfe"],
    &[b"-\xff"],
    &[b"--\xff\xfe"],
    &[b"--birds=\xff"],
    &[b"--color=\xc3\xa9"],
    &[b"\xff"],
    &[b"--no-\xff"],
    &[b"--sprite", b"\xff\xfe"],
    &[b"--record", b"\x80"],
    &[b"-e\xff"],
    &[b"--birds", b"\xff"],
    &[b"--completion", b"\xff\xfe"],
    &[b"--\xc3\xa9"],
    &[b"--bird\xc3\xa9"],
    &[b"--s\xffze"],
    &[b"-n\xff"],
    &[b"-c\xff"],
    &[b"--trails=\xff"],
    &[b"--no-trails=\xff"],
    &[b"--\xff=\xff"],
    &[b"--snapshot=\xed\xa0\x80"],
    &[b"--record-size", b"\xc0\x80"],
    &[b"\x1b[31m"],
    &[b"--birds", b"1\x1b"],
    &[b"--sprite", b"\x01\x02\x7f"],
];

/// Arguments long enough to be cut at the buffer's last byte, with messages
/// of 158 to 162 bytes around the cut.
fn long_arguments() -> Vec<Vec<Vec<u8>>> {
    let mut corpus = vec![
        vec![joined(&[b"--sprite=", &[b'a'; 1000]])],
        vec![joined(&[b"--birds=", &[b'9'; 300]])],
        vec![joined(&[b"--birds=", &[b'x'; 300]])],
        vec![joined(&[b"--", &[b'b'; 300]])],
        vec![vec![b'q'; 300]],
        vec![b"--completion".to_vec(), vec![b'z'; 300]],
        vec![joined(&[b"--color=", &[b'z'; 300]])],
        vec![joined(&[b"-", &[b'e'; 300]])],
        vec![joined(&[b"-n", &[b'1'; 300]])],
        vec![joined(&[b"--birds=", &[b'0'; 300], b"5"])],
        vec![joined(&[b"--birds=", &[b'0'; 300], b"5x"])],
        vec![joined(&[b"--no-", &[b'b'; 300]])],
        vec![joined(&[b"--", &[b'b'; 62]])],
        vec![joined(&[b"--", &[b'b'; 63]])],
        vec![joined(&[b"--", &[b'b'; 64]])],
        vec![joined(&[b"--birds", &[b's'; 57]])],
        vec![joined(&[b"--sprite", &[b'\xff'; 200]])],
        vec![b"--sprite".to_vec(), vec![b'\xfe'; 5000]],
    ];
    for length in 130..=142 {
        corpus.push(vec![vec![b'p'; length]]);
        corpus.push(vec![joined(&[b"--birds=", &vec![b'y'; length - 10]])]);
        corpus.push(vec![b"--completion".to_vec(), vec![b's'; length + 20]]);
        corpus.push(vec![b"--".to_vec(), vec![b'u'; length]]);
    }
    corpus
}

fn boids_corpus() -> Vec<Vec<Vec<u8>>> {
    let mut corpus = vec![Vec::new()];
    corpus.extend(every_form(BOIDS));
    // Each number spelled as `--name=N` or `-nN` (attached) or `--name N`.
    let numeric_forms: &[(&[u8], bool)] = &[
        (b"--birds=", true),
        (b"--birds", false),
        (b"-n", false),
        (b"-n", true),
        (b"--seed=", true),
        (b"--speed", false),
        (b"--bench=", true),
        (b"--record-fps=", true),
        (b"-k", true),
        (b"--perception=", true),
        (b"--size=", true),
    ];
    for value in NUMERIC {
        for &(spelling, attached) in numeric_forms {
            if attached {
                corpus.push(vec![joined(&[spelling, value])]);
            } else {
                corpus.push(vec![spelling.to_vec(), value.to_vec()]);
            }
        }
    }
    for list in [CLUSTERS, UNKNOWN, SPECIALS, POSITIONAL, REPEATS, RAW_BYTES] {
        corpus.extend(list.iter().map(|case| args(case)));
    }
    corpus.extend(long_arguments());
    corpus
}

// ---------------------------------------------------------------------------
// The tests.

#[test]
fn boids_table_parses_as_c_does() {
    let Some(exe) = oracle() else { return };
    let corpus = boids_corpus();
    let mut batch = Batch::default();
    for case in &corpus {
        batch.parse(Table::Boids, 160, case);
    }
    // The buffer the application passes is 160 bytes; other sizes move the cut.
    for size in [0, 1, 2, 3, 22, 40, 128, 159, 161, 4096] {
        for case in &corpus {
            batch.parse(Table::Boids, size, case);
        }
    }
    let checked = batch.check(&exe);
    eprintln!("boids table: {} argument lists, {checked} cases agree", corpus.len());
}

#[test]
fn test_table_parses_as_c_does() {
    let Some(exe) = oracle() else { return };
    let mut corpus = vec![Vec::new()];
    corpus.extend(every_form(TEST_TABLE));
    for value in NUMERIC {
        corpus.push(vec![joined(&[b"--weight=", value])]);
        corpus.push(vec![b"-w".to_vec(), value.to_vec()]);
        corpus.push(vec![joined(&[b"--boids=", value])]);
    }
    let extra: &[&[&[u8]]] = &[
        &[b"-qm"],
        &[b"-qn200"],
        &[b"-qn", b"200"],
        &[b"-mqw0.5"],
        &[b"-wq"],
        &[b"-Pice"],
        &[b"-Pflam"],
        &[b"-qPmono"],
        &[b"-mP"],
        &[b"-lq"],
        &[b"--quiet", b"--no-quiet"],
        &[b"--no-mono", b"-m"],
        &[b"--palette", b"chartreuse"],
        &[b"--boids", b"1200"],
        &[b"--boids=1200"],
        &[b"--no-boids"],
        &[b"--weight=0.25"],
        &[b"--weight=1.0000000000000002"],
        &[b"--weight=-0"],
        &[b"--weight=-1e-320"],
        &[b"--weight=0x1p-1"],
        &[b"--label", b"hello world"],
        &[b"--label"],
        &[b"--quiett"],
        &[b"--weigth"],
        &[b"--wieght"],
        &[b"--lable"],
        &[b"--mon"],
        &[b"--pallete"],
        &[b"--boid"],
    ];
    corpus.extend(extra.iter().map(|case| args(case)));
    for list in [SPECIALS, POSITIONAL, RAW_BYTES] {
        corpus.extend(list.iter().map(|case| args(case)));
    }
    corpus.extend(long_arguments());
    let mut batch = Batch::default();
    for size in [160, 128, 0, 1, 17] {
        for case in &corpus {
            batch.parse(Table::Test, size, case);
        }
    }
    let checked = batch.check(&exe);
    eprintln!("options_test table: {} argument lists, {checked} cases agree", corpus.len());
}

#[test]
fn synthetic_table_parses_as_c_does() {
    let Some(exe) = oracle() else { return };
    let mut corpus = vec![Vec::new()];
    corpus.extend(every_form(SYNTHETIC));
    let long_63 = NAME_63.as_bytes();
    let long_64 = NAME_64.as_bytes();
    let long_100 = NAME_100.as_bytes();
    let mut typo_63 = long_63.to_vec();
    typo_63[5] = b'X';
    let mut typo_64 = long_64.to_vec();
    typo_64[5] = b'X';
    let extra: Vec<Vec<Vec<u8>>> = vec![
        args(&[b"--no-fast"]),
        args(&[b"--no-quick"]),
        args(&[b"--quick"]),
        args(&[b"--no-quick=1"]),
        args(&[b"--no-panel"]),
        args(&[b"--hide"]),
        args(&[b"--no-hide"]),
        args(&[b"--no-no-panel"]),
        args(&[b"--no-panel=1"]),
        args(&[b"--panel"]),
        args(&[b"-o"]),
        args(&[b"-fo"]),
        args(&[b"--x"]),
        args(&[b"--no-x"]),
        args(&[b"--no-no-x"]),
        args(&[b"--no-x=1"]),
        args(&[b"--no-no-x="]),
        args(&[b"--no-no-no-x"]),
        args(&[b"--help=3"]),
        args(&[b"--help="]),
        args(&[b"--help", b"3"]),
        args(&[b"--help=10"]),
        args(&[b"--version=4"]),
        args(&[b"--version"]),
        args(&[b"--no-help"]),
        args(&[b"-h"]),
        args(&[b"-fh"]),
        args(&[b"-V"]),
        args(&[b"-fV"]),
        args(&[b"--aitch"]),
        args(&[b"--vee"]),
        args(&[b"-D"]),
        args(&[b"-DD"]),
        args(&[b"--dup-two"]),
        args(&[b"-e", b"x"]),
        args(&[b"-e", b""]),
        args(&[b"--empty="]),
        args(&[b"--empty", b"alpha"]),
        args(&[b"-c"]),
        args(&[b"-calpha"]),
        args(&[b"-cbeta"]),
        args(&[b"-c", b""]),
        args(&[b"--choice="]),
        args(&[b"--choice=Gamma"]),
        args(&[b"--choice=gamma"]),
        args(&[b"--pick=beta"]),
        args(&[b"--pick", b""]),
        args(&[b"-F"]),
        args(&[b"-Ffile"]),
        args(&[b"-F", b"-"]),
        args(&[b"--int-file", b"5"]),
        args(&[b"-f-"]),
        args(&[b"--", b"-"]),
        args(&[b"-\xe9"]),
        args(&[b"-f\xe9"]),
        args(&[b"-\xe9f"]),
        args(&[b"--accent"]),
        args(&[b"--no-accent"]),
        args(&[b"-u9"]),
        args(&[b"-u10"]),
        args(&[b"-i-5"]),
        args(&[b"-i", b"-6"]),
        args(&[b"-i5.0"]),
        args(&[b"--int=-0"]),
        args(&[b"--integer=3"]),
        args(&[b"--integer", b"-5.5"]),
        args(&[b"--int=-4.5"]),
        args(&[b"-d-2.5"]),
        args(&[b"-d-2.6"]),
        args(&[b"-d1e6"]),
        args(&[b"-d1000000.1"]),
        args(&[b"-d0x1p-3"]),
        args(&[b"--double=1e400"]),
        args(&[b"--doubel"]),
        args(&[b"--dup-on"]),
        args(&[b"--dup"]),
        args(&[b"--in"]),
        args(&[b"--nt"]),
        args(&[b"--xx"]),
        args(&[b"--y"]),
        args(&[b"--"]),
        args(&[b"--no-y"]),
        vec![joined(&[b"--", long_63, b"=v"])],
        vec![joined(&[b"--", long_63]), b"\xff".to_vec()],
        vec![joined(&[b"--", long_64])],
        vec![joined(&[b"--no-", long_64])],
        vec![joined(&[b"--no-", long_100])],
        vec![joined(&[b"--", long_100])],
        vec![joined(&[b"--", long_100, b"=1"])],
        vec![joined(&[b"--", &typo_63])],
        vec![joined(&[b"--", &typo_64])],
        vec![joined(&[b"--", &long_63[..62]])],
        vec![joined(&[b"--", &long_64[..63]])],
        vec![joined(&[b"--", NAME_80.as_bytes(), b"=eighty"])],
        vec![joined(&[b"--", NAME_88.as_bytes()]), b"eighty-eight".to_vec()],
        vec![joined(&[b"--", &NAME_88.as_bytes()[..87]])],
        // A typo is measured only while it fits in 63 bytes.
        vec![joined(&[b"--", long_63, b"x"])],
        vec![joined(&[b"--", long_63, b"xy"])],
        vec![joined(&[b"--", &long_63[..62], b"x"])],
        vec![joined(&[b"--", &long_63[..62], b"xy"])],
        vec![joined(&[b"--", &long_63[..61], b"xy"])],
        // The empty alias, found by an empty name but not by a bare --no-.
        args(&[b"--="]),
        args(&[b"--=1"]),
        args(&[b"--no-"]),
        args(&[b"--no-="]),
        args(&[b"--no-blank"]),
        args(&[b"--blank"]),
    ];
    corpus.extend(extra);
    for list in [SPECIALS, POSITIONAL, RAW_BYTES, UNKNOWN] {
        corpus.extend(list.iter().map(|case| args(case)));
    }
    let mut batch = Batch::default();
    for size in [160, 0, 1, 64] {
        for case in &corpus {
            batch.parse(Table::Synthetic, size, case);
        }
    }
    let quoted: &[&[&[u8]]] =
        &[&[], &[b"--shy"], &[b"--no-shy"], &[b"--shy=1"], &[b"--sh"], &[b"-s"]];
    for case in quoted {
        batch.parse(Table::Quoted, 160, &args(case));
    }
    let checked = batch.check(&exe);
    eprintln!("synthetic table: {} argument lists, {checked} cases agree", corpus.len());
}

/// The range message prints its bounds with `%d` or `%g`; these bounds cover
/// every `%g` branch, its rounding, ties, and the extremes.
#[test]
fn numeric_rows_and_their_ranges_match_c() {
    let Some(exe) = oracle() else { return };
    let doubles: &[(f64, f64)] = &[
        (0.0, 1.0),
        (-2.5, 1e6),
        (1e-5, 123456.7),
        (0.1, 0.3),
        (f64::from_bits(1), f64::MAX),
        (f64::NEG_INFINITY, f64::INFINITY),
        (f64::INFINITY, f64::INFINITY),
        (1.0, f64::NAN),
        (1.0, -f64::NAN),
        (f64::NAN, 1.0),
        (1234565.0, 1234575.0),
        (999999.5, 9999995.0),
        (0.000099999999, 0.00001),
        (-0.0, 0.0),
        (1e21, 1e22),
        (123456.0, 1234567.0),
        (0.0001, 0.00012345678),
        (1.0 / 3.0, 2.0 / 3.0),
        (100000.0, 999999.0),
        (0.5, 2.5),
        (12.5, 0.000123456),
        (-1e-300, 1e300),
        (9.9999949999999e5, 9.9999950000001e5),
        (0.00001234, 99999.95),
        (1.5e-7, 8.5),
        (123.456789, 0.001),
        (2.2250738585072014e-308, f64::from_bits(1)),
        (-123456789.0, -0.000001),
        (0.99999949999, 0.9999995),
    ];
    let integers: &[(f64, f64)] = &[
        (1.0, 4096.0),
        (0.0, 2147483647.0),
        (-2147483648.0, 2147483647.0),
        (-5.5, 5.5),
        (0.5, 1.5),
        (1e-300, 1.0),
        (-0.9, 0.9),
        (3.0, 2.0),
        (-0.0, 0.0),
    ];
    let values: &[&[u8]] = &[
        b"-1e308",
        b"1e308",
        b"0",
        b"-0",
        b"0.5",
        b"1",
        b"2",
        b"-1",
        b"5.5",
        b"-5.5",
        b"2147483647",
        b"2147483648",
        b"-2147483648",
        b"-2147483649",
        b"1e10",
        b"1e-320",
        b"1e-400",
        b"nan",
        b"1234570",
        b"0.2",
        b"",
    ];
    let mut batch = Batch::default();
    for &(integer, pairs) in &[(false, doubles), (true, integers)] {
        for &(min, max) in pairs {
            for value in values {
                batch.ranged(integer, min, max, 160, &[b"--x".to_vec(), value.to_vec()]);
                batch.ranged(integer, min, max, 20, &[joined(&[b"--x=", value])]);
            }
            batch.ranged(integer, min, max, 160, &args(&[b"-x", b"1"]));
            batch.ranged(integer, min, max, 160, &args(&[b"--no-x"]));
        }
    }
    for value in NUMERIC {
        batch.ranged(false, -1e300, 1e300, 160, &[b"--x".to_vec(), value.to_vec()]);
        batch.ranged(true, -2147483648.0, 2147483647.0, 160, &[joined(&[b"--x=", value])]);
    }
    let checked = batch.check(&exe);
    eprintln!("one row tables: {checked} cases agree");
}

#[test]
fn usage_prints_as_c_does() {
    let Some(exe) = oracle() else { return };
    let programs: &[&[u8]] =
        &[b"cbirds", b"\xff\xfe-bird", b"", b"/usr/local/bin/cbirds", b"a program with spaces"];
    let mut batch = Batch::default();
    for table in [Table::Boids, Table::Test, Table::Synthetic, Table::Quoted] {
        for everything in [false, true] {
            for program in programs {
                for tagline in [Some(None), None] {
                    for examples in [1, 0, 2] {
                        batch.usage(table, everything, program, tagline, examples);
                    }
                }
            }
            batch.usage(table, everything, b"cbirds", Some(Some("")), 1);
            batch.usage(table, everything, b"cbirds", Some(Some("Tag \u{2014} line\n")), 2);
        }
    }
    let checked = batch.check(&exe);
    eprintln!("usage: {checked} outputs agree");
}

#[test]
fn completions_print_as_c_does() {
    let Some(exe) = oracle() else { return };
    let shells: &[&[u8]] = &[
        b"bash",
        b"zsh",
        b"fish",
        b"tcsh",
        b"",
        b"BASH",
        b"bash\xff",
        b"bas",
        b"bash ",
        b" bash",
        b"fish\n",
        b"zsh5",
        b"ksh",
        b"powershell",
    ];
    let programs = ["cbirds", "rbirds", "my prog", "", "pr\u{f6}g"];
    let mut batch = Batch::default();
    for table in [Table::Boids, Table::Test, Table::Synthetic, Table::Quoted] {
        for shell in shells {
            for program in programs {
                batch.completion(table, shell, program);
            }
        }
    }
    let checked = batch.check(&exe);
    eprintln!("completions: {checked} outputs agree");
}

/// The batch's answers are those of fresh processes given the raw bytes by
/// the OS, not only of a batch reset between cases.
#[test]
fn fresh_processes_with_raw_arguments_agree() {
    let Some(exe) = oracle() else { return };
    let mut samples: Vec<(Table, Vec<Vec<u8>>)> = Vec::new();
    for list in [CLUSTERS, SPECIALS, RAW_BYTES, REPEATS] {
        for case in list.iter().step_by(3) {
            samples.push((Table::Boids, args(case)));
        }
    }
    samples.push((Table::Boids, Vec::new()));
    samples.push((Table::Boids, long_arguments().swap_remove(2)));
    samples.push((Table::Test, args(&[b"--weight", b"0x1p-2", b"-qm"])));
    samples.push((Table::Synthetic, args(&[b"-\xe9", b"--no-quick", b"--help=3"])));
    let mut by_table: BTreeMap<Table, usize> = BTreeMap::new();
    for (table, case) in &samples {
        let os_args: Vec<OsString> = case.iter().map(|a| OsString::from_vec(a.clone())).collect();
        let output = Command::new(&exe)
            .arg("argv")
            .arg(table.letter())
            .arg("160")
            .args(&os_args)
            .output()
            .expect("run the oracle");
        assert!(output.status.success(), "oracle failed on {}", show(case));
        let c = String::from_utf8(output.stdout).expect("ASCII");
        assert_eq!(table.parse(160, case), c, "{table:?} {}", show(case));
        *by_table.entry(*table).or_default() += 1;
    }
    eprintln!("fresh processes: {by_table:?}");
}

/// An `OsString` built in Rust can hold a NUL that a real `argv` cannot; the
/// parser reads it as the C would read that string, up to the NUL.
#[test]
fn arguments_end_at_a_nul_as_c_strings_do() {
    let Some(exe) = oracle() else { return };
    let cases: &[&[&[u8]]] = &[
        &[b"--birds=12\0junk"],
        &[b"--birds", b"12\0junk"],
        &[b"--sprite=a\0b"],
        &[b"--sprite", b"\0b"],
        &[b"--trails\0=1"],
        &[b"-e\0l"],
        &[b"\0x"],
        &[b"--\0x"],
        &[b"--help\0x"],
        &[b"--completion", b"fish\0bash"],
        &[b"--color=ice\0x"],
        &[b"--bogus\0x"],
    ];
    let mut batch = Batch::default();
    for case in cases {
        batch.parse_with_nul(Table::Boids, 160, &args(case));
    }
    // And the program name the help prints, and the shell asked for.
    batch.cases.push(Case {
        label: "usage with a NUL in the program name".to_owned(),
        command: format!("u B 1 {} = 1", x(b"cbirds")),
        expected: Table::Boids.usage(
            true,
            b"cbirds\0tail",
            Some(BOIDS_TAGLINE),
            Some(BOIDS_EXAMPLES),
        ),
    });
    for shell in [&b"zsh\0x"[..], b"\0bash"] {
        let c_shell = shell.split(|&b| b == 0).next().unwrap_or_default();
        batch.cases.push(Case {
            label: format!("completion for {}", shell.escape_ascii()),
            command: format!("c B {} {}", x(c_shell), x(b"cbirds")),
            expected: Table::Boids.completion(shell, "cbirds"),
        });
    }
    batch.check(&exe);
}

/// The application's own handling, as `read_options` in boids.c does it with
/// its 160 byte buffer: the shell is what `--completion` names, and cannot be
/// longer than the buffer allows.
#[test]
fn completion_shell_names_are_cut_like_c() {
    let mut settings = Boids::default();
    let long_shell = OsString::from_vec(vec![b'b'; 400]);
    let argv = [OsString::from("cbirds"), OsString::from("--completion"), long_shell];
    let parsed = options::parse(BOIDS, &mut settings, &argv, 160);
    assert_eq!(parsed.status, options::Status::Completion);
    assert_eq!(parsed.message, vec![b'b'; 159]);
    let mut out = Vec::new();
    assert!(!options::completion(&mut out, &parsed.message, "cbirds", BOIDS).unwrap());
    assert!(out.is_empty());
}
