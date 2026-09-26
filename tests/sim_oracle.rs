#![cfg(unix)]

//! Controlled numerical comparison of the simulation against the pinned C
//! reference (docs/PORTING.md §4.B, docs/COMPATIBILITY.md C02–C08, C11, C12,
//! C14).
//!
//! Each scenario is a script run once through `tools/oracle/sim_oracle.c`
//! (the unmodified boids.c) and once through the Rust port, with identical
//! injected time, input and window sizes. Every double is compared as its
//! IEEE bits and every queued frame as its bytes: there is no tolerance. On a
//! mismatch the scenario is re-run with full dumps around the first differing
//! digest and the first differing field is reported.

mod support;

use std::fmt::Write as _;
use std::process::{Command, Stdio};

use support::oracle;
use support::sim::run_rust;

const SOURCES: &[&str] =
    &["cells.c", "font.c", "gif.c", "kitty_graphics.c", "options.c", "png.c", "spatial_grid.c"];

fn run_c(exe: &std::path::Path, script: &str) -> String {
    use std::io::Write;
    let mut child = Command::new(exe)
        .env_remove("COLORTERM")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run sim oracle");
    let mut input = child.stdin.take().expect("stdin");
    let data = script.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let output = child.wait_with_output().expect("sim oracle output");
    let _ = writer.join();
    assert!(
        output.status.success(),
        "sim oracle failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("oracle output is ASCII")
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// The first line on which two transcripts differ, and the first differing
/// space-separated field in it.
fn first_difference(expected: &str, actual: &str) -> Option<String> {
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    for number in 1.. {
        match (expected_lines.next(), actual_lines.next()) {
            (None, None) => return None,
            (e, a) if e == a => continue,
            (e, a) => {
                let e = e.unwrap_or("<end>");
                let a = a.unwrap_or("<end>");
                let field = e
                    .split(' ')
                    .zip(a.split(' '))
                    .find(|(x, y)| x != y)
                    .map(|(x, y)| {
                        let short = |s: &str| s.chars().take(160).collect::<String>();
                        format!("first differing field: C `{}` Rust `{}`", short(x), short(y))
                    })
                    .unwrap_or_default();
                let short = |s: &str| s.chars().take(300).collect::<String>();
                return Some(format!(
                    "line {number}\n  C:    {}\n  Rust: {}\n  {field}",
                    short(e),
                    short(a)
                ));
            }
        }
    }
    unreachable!()
}

fn compare(name: &str, script: &str) {
    let Some(exe) = oracle::build("sim_oracle", "sim_oracle.c", SOURCES) else { return };
    let expected = run_c(&exe, script);
    let actual = run_rust(script);
    if expected == actual {
        return;
    }
    // Locate the first differing digest and replace it, and the one before
    // it, with full dumps so the report names a field.
    let digest_lines: Vec<usize> =
        script.lines().enumerate().filter(|(_, l)| l.trim() == "digest").map(|(i, _)| i).collect();
    let expected_digests: Vec<&str> =
        expected.lines().filter(|l| l.starts_with("digest ")).collect();
    let actual_digests: Vec<&str> = actual.lines().filter(|l| l.starts_with("digest ")).collect();
    let mut detail = first_difference(&expected, &actual).unwrap_or_default();
    if let Some(k) = expected_digests.iter().zip(&actual_digests).position(|(e, a)| e != a) {
        let mut lines: Vec<String> = script.lines().map(str::to_owned).collect();
        if k > 0 {
            lines[digest_lines[k - 1]] = "dump".into();
        }
        lines[digest_lines[k]] = "dump".into();
        lines.truncate(digest_lines[k] + 1);
        let focused = lines.join("\n") + "\n";
        let e = run_c(&exe, &focused);
        let a = run_rust(&focused);
        detail = format!(
            "first differing digest is number {k} (script line {})\n{}",
            digest_lines[k] + 1,
            first_difference(&e, &a).unwrap_or_else(|| "dumps agree?".into())
        );
    }
    let kept = oracle::repository().join("target/scratch").join(format!("sim-{name}.script"));
    let _ = std::fs::create_dir_all(kept.parent().unwrap());
    let _ = std::fs::write(&kept, script);
    panic!(
        "scenario {name}: Rust diverges from the C reference\n{detail}\nscript kept at {}",
        kept.display()
    );
}

/// Recording parameters, as run_recording derives them.
struct Recording {
    seed: u32,
    birds: i32,
    flocks: i32,
    hawks: i32,
    pace_notch: i32,
    avoid_notch: i32,
    palette: i32,
    depth: bool,
    trails: bool,
    matrix: bool,
    columns: i32,
    rows: i32,
    fps: i32,
    frames: i32,
    turning: i32,
    size: i32,
}

impl Default for Recording {
    fn default() -> Recording {
        Recording {
            seed: 1,
            birds: 800,
            flocks: 1,
            hawks: 0,
            pace_notch: 1,
            avoid_notch: 4,
            palette: 1,
            depth: false,
            trails: false,
            matrix: false,
            columns: 96,
            rows: 26,
            fps: 25,
            frames: 150,
            turning: 8,
            size: 30,
        }
    }
}

/// `record_delay_for`, as the Rust port computes it (itself compared with
/// the C in the recording tests).
fn actual_fps(fps: i32) -> f64 {
    100.0 / f64::from(rbirds::record::record_delay_for(fps))
}

fn digest_every(frames: i32) -> i32 {
    if frames > 400 { 10 } else { 1 }
}

impl Recording {
    fn script(&self) -> String {
        let mut s = String::new();
        let fps = actual_fps(self.fps);
        let _ = writeln!(
            s,
            "set birds {}\nset flocks {}\nset hawks {}",
            self.birds, self.flocks, self.hawks
        );
        let _ = writeln!(
            s,
            "set pace {}\nset avoid {}\nset palette {}",
            self.pace_notch, self.avoid_notch, self.palette
        );
        let _ = writeln!(
            s,
            "set turning {}\nset deep {}\nset trails {}",
            self.turning,
            u8::from(self.depth),
            u8::from(self.trails)
        );
        s.push_str("notches\n");
        if self.matrix {
            s.push_str("set palette 4\nset trails 1\nset alignment 12\nset rain 1\nnotches\n");
        }
        let _ = writeln!(s, "seconds {}", bits(1.0 / fps));
        s.push_str("set legend 0\n");
        let _ = writeln!(
            s,
            "screen {} {} {} {}",
            self.columns,
            self.rows,
            self.columns * 8,
            self.rows * 16
        );
        s.push_str("grid\n");
        let _ = writeln!(s, "set size {}", self.size);
        let _ = writeln!(s, "seed {}\nalloc {}\ninit\nhawks\nintro\ndigest", self.seed, self.birds);
        let every = digest_every(self.frames);
        for frame in 0..self.frames {
            let _ = writeln!(s, "record {frame} {}", bits(fps));
            if frame % every == 0 || frame + 1 == self.frames {
                s.push_str("digest\n");
            }
        }
        s.push_str("dump\n");
        s
    }
}

fn frames(release: i32, debug: i32) -> i32 {
    if cfg!(debug_assertions) { debug } else { release }
}

#[test]
fn recording_default_flock_matches_the_reference() {
    compare(
        "record-default",
        &Recording { frames: frames(150, 90), ..Default::default() }.script(),
    );
}

#[test]
fn recording_seeds_match_the_reference() {
    for seed in [0, 2, 5, 33, 42, 2147483647] {
        let script =
            Recording { seed, birds: 200, frames: frames(120, 40), ..Default::default() }.script();
        compare(&format!("record-seed-{seed}"), &script);
    }
}

#[test]
fn recording_with_hawks_matches_the_reference() {
    for hawks in [1, 2, 4] {
        let script =
            Recording { hawks, birds: 300, frames: frames(200, 60), ..Default::default() }.script();
        compare(&format!("record-hawks-{hawks}"), &script);
    }
}

#[test]
fn recording_flocks_and_avoidance_match_the_reference() {
    for flocks in [2, 3] {
        for avoid_notch in [0, 2, 4, 8, 12] {
            let script = Recording {
                flocks,
                avoid_notch,
                birds: 240,
                frames: frames(150, 40),
                ..Default::default()
            }
            .script();
            compare(&format!("record-flocks-{flocks}-avoid-{avoid_notch}"), &script);
        }
    }
}

#[test]
fn recording_speed_extremes_match_the_reference() {
    for pace_notch in [0, 4, 9, 12] {
        let script =
            Recording { pace_notch, birds: 250, frames: frames(150, 40), ..Default::default() }
                .script();
        compare(&format!("record-pace-{pace_notch}"), &script);
    }
    for turning in [0, 12] {
        let script =
            Recording { turning, birds: 150, frames: frames(100, 30), ..Default::default() }
                .script();
        compare(&format!("record-turning-{turning}"), &script);
    }
}

#[test]
fn recording_depth_trails_and_matrix_match_the_reference() {
    let script = Recording {
        depth: true,
        trails: true,
        hawks: 2,
        flocks: 3,
        pace_notch: 12,
        birds: 300,
        frames: frames(150, 40),
        ..Default::default()
    }
    .script();
    compare("record-depth-trails", &script);
    let script =
        Recording { matrix: true, birds: 300, frames: frames(150, 40), ..Default::default() }
            .script();
    compare("record-matrix", &script);
}

#[test]
fn recording_extreme_sizes_match_the_reference() {
    compare(
        "record-one-bird",
        &Recording { birds: 1, hawks: 1, frames: frames(300, 100), ..Default::default() }.script(),
    );
    compare(
        "record-tiny-viewport",
        &Recording {
            columns: 40,
            rows: 14,
            birds: 120,
            hawks: 4,
            frames: frames(200, 60),
            ..Default::default()
        }
        .script(),
    );
    compare(
        "record-large-viewport",
        &Recording {
            columns: 400,
            rows: 120,
            birds: 400,
            size: 64,
            frames: frames(60, 10),
            ..Default::default()
        }
        .script(),
    );
    compare(
        "record-4096",
        &Recording {
            birds: 4096,
            flocks: 3,
            hawks: 4,
            frames: frames(30, 3),
            ..Default::default()
        }
        .script(),
    );
    compare(
        "record-small-birds",
        &Recording { size: 4, birds: 300, frames: frames(100, 30), ..Default::default() }.script(),
    );
}

#[test]
fn recording_rates_match_the_reference() {
    for fps in [2, 7, 20, 41, 60, 120] {
        let script =
            Recording { fps, birds: 120, frames: frames(90, 30), ..Default::default() }.script();
        compare(&format!("record-fps-{fps}"), &script);
    }
}

/// Past a minute, the autopilot moves a slider every four seconds.
#[test]
fn a_long_recording_flies_itself_as_the_reference_does() {
    let script =
        Recording { birds: 100, fps: 25, frames: frames(1700, 1600), ..Default::default() }
            .script();
    compare("record-autopilot", &script);
}

/// A live session: monotonic timestamps with jitter, zero-length and late
/// frames, keys, mouse reports, resizes, the panel, and the flight out.
struct Live {
    render: i32,
    birds: i32,
    hawks: i32,
    flocks: i32,
    palette: i32,
    legend: bool,
    trails: bool,
    depth: bool,
    events: Vec<(i32, &'static str)>,
    frames: i32,
    columns: i32,
    rows: i32,
    pace_notch: i32,
}

impl Default for Live {
    fn default() -> Live {
        Live {
            render: 1,
            birds: 120,
            hawks: 0,
            flocks: 1,
            palette: 1,
            legend: false,
            trails: false,
            depth: false,
            events: Vec::new(),
            frames: 120,
            columns: 100,
            rows: 30,
            pace_notch: 1,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Live {
    fn script(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "set birds {}\nset hawks {}\nset flocks {}",
            self.birds, self.hawks, self.flocks
        );
        let _ = writeln!(
            s,
            "set palette {}\nset legend {}\nset trails {}",
            self.palette,
            u8::from(self.legend),
            u8::from(self.trails)
        );
        let _ =
            writeln!(s, "set deep {}\nset pace {}\nnotches", u8::from(self.depth), self.pace_notch);
        // main: render mode, size, theme (not asked here), grid, seed, sizes.
        let _ = writeln!(s, "set render {}\nset size 30", self.render);
        let (w, h) = (self.columns * 8, self.rows * 16);
        let _ = writeln!(s, "seed 7\nscreen {} {} {w} {h}\ngrid", self.columns, self.rows);
        if self.render != 0 {
            s.push_str("sprites\nset truecolor 1\n");
        }
        let _ = writeln!(s, "seconds {}\nset hawk_sets 1", bits(1.0 / 60.0));
        let _ = writeln!(
            s,
            "alloc {}\nscreen {} {} {w} {h}\ninit\nhawks\nintro",
            self.birds, self.columns, self.rows
        );
        let (mut sec, mut nsec) = (1000_i64, 0_i64);
        let _ = writeln!(s, "live_begin {sec} {nsec}");
        let mut columns = self.columns;
        let mut rows = self.rows;
        for frame in 0..self.frames {
            // 60 Hz with jitter, a zero-length frame, and some late ones.
            let step = match frame % 17 {
                3 => 0,
                5 => 50_000_000,
                11 => 16_000_000 + (frame as i64 * 7919) % 900_000,
                _ => 16_666_667,
            };
            nsec += step;
            sec += nsec / 1_000_000_000;
            nsec %= 1_000_000_000;
            let mut keys = String::from("-");
            for (at, bytes) in &self.events {
                if *at == frame {
                    if let Some(size) = bytes.strip_prefix("resize ") {
                        let mut parts = size.split('x');
                        columns = parts.next().unwrap().parse().unwrap();
                        rows = parts.next().unwrap().parse().unwrap();
                    } else if let Some(line) = bytes.strip_prefix("stats ") {
                        let _ = writeln!(s, "stats {line}");
                    } else {
                        keys = hex(bytes.as_bytes());
                    }
                }
            }
            let _ = writeln!(
                s,
                "live {keys} {sec} {nsec} {columns} {rows} {} {}",
                columns * 8,
                rows * 16
            );
            s.push_str("digest\n");
        }
        s.push_str("dump\n");
        s
    }
}

#[test]
fn live_braille_session_matches_the_reference() {
    let script = Live {
        events: vec![
            (5, "B"),
            (9, "\x1b[<35;30;10M"),
            (12, " "),
            (14, "."),
            (16, " "),
            (20, "h"),
            (24, "stats 2125 31000"),
            (30, "+"),
            (34, "e"),
            (40, "resize 60x20"),
            (44, "-"),
            (50, "resize 120x40"),
            (55, "\t"),
            (60, "0"),
            (64, "vVVV"),
            (70, "\x1b[A\x1b[A\x1b[B\x1b[B\x1b[D\x1b[C\x1b[D\x1b[Cba"),
            (80, "kkK"),
            (90, "q"),
        ],
        legend: true,
        frames: frames(140, 110),
        ..Default::default()
    }
    .script();
    compare("live-braille", &script);
}

#[test]
fn live_text_renderers_match_the_reference() {
    for render in [2, 3] {
        let script = Live {
            render,
            hawks: 2,
            flocks: 2,
            trails: true,
            depth: true,
            legend: true,
            events: vec![
                (10, "h"),
                (20, "gGG"),
                (30, "resize 40x14"),
                (40, "resize 90x30"),
                (50, "h"),
            ],
            frames: frames(70, 55),
            ..Default::default()
        }
        .script();
        compare(&format!("live-render-{render}"), &script);
    }
}

#[test]
fn live_kitty_placements_match_the_reference() {
    let script = Live {
        render: 0,
        hawks: 3,
        trails: true,
        depth: true,
        legend: true,
        events: vec![(15, "h"), (25, "resize 50x16"), (35, "resize 100x30"), (45, "q")],
        frames: frames(100, 100),
        ..Default::default()
    }
    .script();
    compare("live-kitty", &script);
}

#[test]
fn live_palettes_and_shades_match_the_reference() {
    for palette in 1..10 {
        let script =
            Live { palette, birds: 60, frames: frames(30, 12), ..Default::default() }.script();
        compare(&format!("live-palette-{palette}"), &script);
    }
}

#[test]
fn benchmark_frames_match_the_reference() {
    let mut s = String::new();
    s.push_str("set birds 400\nset hawks 3\nset flocks 2\nset pace 12\nnotches\n");
    s.push_str("screen 200 50 1600 800\nset size 30\n");
    let _ = writeln!(s, "seconds {}", bits(1.0 / 60.0));
    s.push_str("grid\nseed 1\nalloc 400\ninit\nhawks\ndigest\n");
    for _ in 0..frames(120, 30) {
        s.push_str("bench\ndigest\n");
    }
    s.push_str("dump\n");
    compare("bench-hawks", &s);
}

#[test]
fn keys_presets_and_population_match_the_reference() {
    let mut s = String::new();
    s.push_str("screen 80 24 640 384\nset legend 1\nscreen 80 24 640 384\nnotches\nset size 30\nset hawk_sets 1\n");
    s.push_str("seed 3\nalloc 100\nset birds 100\ninit\ngrid\ndigest\n");
    for keys in [
        "BBBBBBBBBBBBBB",
        "bbbbbbbbbbbbbbbbbbbbbbbb",
        "SsAaTtPpVvGg",
        "\t\t\t\t",
        "0",
        "\x1b[<35;10;5M",
        "\x1b[<0;20;9m",
        "\x1b[<35;3;3MB",
        "\x1b[A\x1b[1;2B\x1bOP",
        "kkkkkK",
        "AABBDCDCba",
        "AAABBDCDCba",
        "qwertyAABBDCDCba",
        "  ..",
        "h",
        "hh",
    ] {
        let _ = writeln!(s, "keys {}\ndigest", hex(keys.as_bytes()));
    }
    s.push_str("resize 160\ndigest\nresize 40\ndigest\ndump\n");
    compare("keys", &s);
}

/// Every notch of every slider, each flown for a few frames from one seeded
/// flock. The derived values and the flight they give must match exactly.
#[test]
fn every_notch_of_every_slider_matches_the_reference() {
    let mut s = String::new();
    s.push_str("set birds 120\nset flocks 2\nset hawks 1\nset size 30\nnotches\n");
    let _ = writeln!(s, "seconds {}", bits(1.0 / 60.0));
    s.push_str("screen 100 30 800 480\ngrid\nseed 9\nalloc 120\ninit\nhawks\ndigest\n");
    for slider in ["boundary", "separation", "alignment", "vision", "pace", "turning", "avoid"] {
        for notch in 0..=12 {
            let _ = writeln!(s, "set {slider} {notch}\nnotches\ndigest");
            for _ in 0..3 {
                s.push_str("fly\ndigest\n");
            }
        }
        let _ = writeln!(s, "defaults\ndigest");
    }
    for preset in 0..3 {
        let _ = writeln!(s, "preset {preset}\ndigest\nfly\ndigest");
    }
    s.push_str("dump\n");
    compare("notch-sweep", &s);
}

/// A live session across the intro's release (3 s) on a faster clock, then
/// late frames, then long enough idle for the autopilot (60 s) to move the
/// sliders twice, then the flight out.
#[test]
fn a_long_live_session_crosses_every_threshold_as_the_reference_does() {
    let mut s = String::new();
    s.push_str("set birds 60\nset hawks 1\nset palette 1\nset legend 1\nnotches\nset render 1\nset size 30\n");
    s.push_str("seed 11\nscreen 100 30 800 480\ngrid\nsprites\nset truecolor 1\n");
    let _ = writeln!(s, "seconds {}\nset hawk_sets 1", bits(1.0 / 60.0));
    s.push_str("alloc 60\nscreen 100 30 800 480\ninit\nhawks\nintro\n");
    let (mut sec, mut nsec) = (500_i64, 0_i64);
    let _ = writeln!(s, "live_begin {sec} {nsec}");
    let mut advance = |s: &mut String, step: i64, keys: &str, digest: bool| {
        nsec += step;
        sec += nsec / 1_000_000_000;
        nsec %= 1_000_000_000;
        let _ = writeln!(s, "live {keys} {sec} {nsec} 100 30 800 480");
        if digest {
            s.push_str("digest\n");
        }
    };
    // 240 Hz through the intro's release at three seconds.
    for _ in 0..800 {
        advance(&mut s, 4_166_667, "-", true);
    }
    // Late frames, a zero-length one, and a key that resets the idle clock.
    for step in [100_000_000, 0, 250_000_000, 33_333_333] {
        advance(&mut s, step, "-", true);
    }
    advance(&mut s, 16_666_667, &hex(b"e"), true);
    // Idle past a minute: the autopilot takes over and drifts every 4 s.
    for frame in 0..3900 {
        advance(&mut s, 16_666_667, "-", frame % 25 == 0);
    }
    s.push_str("digest\n");
    advance(&mut s, 16_666_667, &hex(b"q"), true);
    for _ in 0..50 {
        advance(&mut s, 16_666_667, "-", true);
    }
    s.push_str("dump\n");
    compare("live-long", &s);
}
