//! Headless modes end to end against the canonical C build
//! (docs/COMPATIBILITY.md C08–C11, C16, C17). GIF recordings must match
//! exactly, asciinema casts exactly apart from the header's wall-clock
//! timestamp and title, and benchmark reports apart from the two measured
//! timing lines. A seeded recording exercises options, simulation, sprites,
//! rotation and resampling, composition, cells, palette quantization and LZW
//! all at once, so one differing byte anywhere fails it.

mod support;

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use support::oracle::{self, Scratch};

struct Outcome {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(exe: &Path, args: &[&str], dir: &Path) -> Outcome {
    let output = Command::new(exe)
        .arg0("birds")
        .args(args)
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("run program");
    Outcome { code: output.status.code(), stdout: output.stdout, stderr: output.stderr }
}

/// The first differing byte of two files, as a report.
fn byte_difference(expected: &[u8], actual: &[u8]) -> String {
    let at = expected
        .iter()
        .zip(actual)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    format!(
        "lengths C {} Rust {}; first difference at byte {at}: C {:02x?} Rust {:02x?}",
        expected.len(),
        actual.len(),
        &expected[at.min(expected.len())..(at + 16).min(expected.len())],
        &actual[at.min(actual.len())..(at + 16).min(actual.len())],
    )
}

fn record_both(name: &str, extra: &[&str]) {
    let Some(reference) = oracle::reference_binary() else { return };
    let rust = oracle::rust_binary();
    let scratch = Scratch::new(&format!("record-{name}"));
    let mut c_args = vec!["--record", "c.out"];
    let mut r_args = vec!["--record", "r.out"];
    c_args.extend_from_slice(extra);
    r_args.extend_from_slice(extra);
    let c = run(&reference, &c_args, &scratch.path);
    let r = run(&rust, &r_args, &scratch.path);
    assert_eq!(
        c.code,
        r.code,
        "{name}: exit status; C stderr {}",
        String::from_utf8_lossy(&c.stderr)
    );
    // The summary names the file; the names differ only by c/r.
    let summary = String::from_utf8_lossy(&c.stdout).replacen("c.out", "r.out", 1);
    assert_eq!(summary.as_bytes(), r.stdout.as_slice(), "{name}: summary");
    assert_eq!(
        String::from_utf8_lossy(&c.stderr).replace("c.out", "r.out"),
        String::from_utf8_lossy(&r.stderr),
        "{name}: diagnostics"
    );
    let expected = std::fs::read(scratch.file("c.out")).ok();
    let actual = std::fs::read(scratch.file("r.out")).ok();
    match (expected, actual) {
        (Some(e), Some(a)) => assert!(e == a, "{name}: files differ: {}", byte_difference(&e, &a)),
        (None, None) => {}
        (e, a) => panic!("{name}: C wrote {} and Rust wrote {}", e.is_some(), a.is_some()),
    }
}

fn gif(name: &str, extra: &[&str]) {
    record_both(name, extra);
}

#[test]
fn gif_recordings_are_the_references_bytes() {
    let short = if cfg!(debug_assertions) { "1" } else { "2" };
    gif("default", &["--record-seconds", short]);
    gif("seed-0", &["--seed", "0", "--record-seconds", "1", "-n", "200"]);
    gif("seed-42-hawks", &["--seed", "42", "--hawks", "3", "--record-seconds", short, "-n", "300"]);
    gif(
        "flocks",
        &["--flocks", "3", "--avoidance", "10", "--color", "prism", "--record-seconds", "1"],
    );
    gif(
        "depth-trails",
        &["--depth", "--trails", "--hawks", "2", "--record-seconds", "1", "--color", "ice"],
    );
    gif("matrix", &["--matrix", "--record-seconds", "1", "--record-size", "60x20"]);
    gif("shapes", &["--shape", "arrow", "--record-seconds", "1", "-n", "150", "--size", "18"]);
    gif("dot-plane", &["--shape", "plane", "--record-seconds", "1", "-n", "100", "--size", "64"]);
    gif("dot", &["--shape", "dot", "--record-seconds", "1", "-n", "100", "--size", "4"]);
    gif("fps-7", &["--record-fps", "7", "--record-seconds", "2", "-n", "100"]);
    gif("fps-60", &["--record-fps", "60", "--record-seconds", "1", "-n", "100"]);
    gif("small", &["--record-size", "40x14", "--record-seconds", "1", "-n", "80", "--hawks", "4"]);
    gif(
        "preset",
        &["--preset", "murmuration", "--speed", "12", "--record-seconds", "1", "-n", "200"],
    );
}

#[test]
fn gif_recordings_of_text_renderers_are_the_references_bytes() {
    for render in ["braille", "sextants", "blocks"] {
        gif(
            &format!("text-{render}"),
            &["--render", render, "--record-seconds", "1", "-n", "200", "--color", "aurora"],
        );
    }
}

#[test]
fn a_custom_sprite_records_as_the_reference_does() {
    let scratch_sprite = oracle::repository().join("assets/sprite.png");
    let path = scratch_sprite.to_str().unwrap().to_owned();
    gif(
        "custom-sprite",
        &["--sprite", &path, "--record-seconds", "1", "-n", "100", "--depth", "--hawks", "1"],
    );
    gif(
        "custom-sprite-text",
        &["--sprite", &path, "--render", "blocks", "--record-seconds", "1", "-n", "100"],
    );
}

/// A cast's header carries the wall clock; that and the product title are
/// the only fields normalized.
fn normalize_cast(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let Some((header, rest)) = text.split_once('\n') else { return bytes.to_vec() };
    let mut header = header.replace("\"title\": \"cbirds\"", "\"title\": \"rbirds\"");
    if let Some(at) = header.find("\"timestamp\": ") {
        let start = at + "\"timestamp\": ".len();
        let end = header[start..].find(',').map_or(header.len(), |e| start + e);
        header.replace_range(start..end, "T");
    }
    format!("{header}\n{rest}").into_bytes()
}

#[test]
fn the_cast_normalization_keeps_everything_else() {
    let c = b"{\"version\": 2, \"width\": 60, \"timestamp\": 1700000000, \"title\": \"cbirds\"}\n[0, \"o\", \"x\"]\n";
    let r = b"{\"version\": 2, \"width\": 60, \"timestamp\": 1800000000, \"title\": \"rbirds\"}\n[0, \"o\", \"x\"]\n";
    assert_eq!(normalize_cast(c), normalize_cast(r));
    let other = b"{\"version\": 2, \"width\": 61, \"timestamp\": 1800000000, \"title\": \"rbirds\"}\n[0, \"o\", \"x\"]\n";
    assert_ne!(normalize_cast(c), normalize_cast(other));
}

fn cast(name: &str, extra: &[&str]) {
    let Some(reference) = oracle::reference_binary() else { return };
    let rust = oracle::rust_binary();
    let scratch = Scratch::new(&format!("cast-{name}"));
    let mut c_args = vec!["--record", "c.cast"];
    let mut r_args = vec!["--record", "r.cast"];
    c_args.extend_from_slice(extra);
    r_args.extend_from_slice(extra);
    let c = run(&reference, &c_args, &scratch.path);
    let r = run(&rust, &r_args, &scratch.path);
    assert_eq!(c.code, r.code, "{name}: exit status");
    assert_eq!(
        String::from_utf8_lossy(&c.stdout).replacen("c.cast", "r.cast", 1).as_bytes(),
        r.stdout.as_slice()
    );
    assert_eq!(c.stderr, r.stderr);
    let e = normalize_cast(&std::fs::read(scratch.file("c.cast")).expect("C cast"));
    let a = normalize_cast(&std::fs::read(scratch.file("r.cast")).expect("Rust cast"));
    assert!(e == a, "{name}: casts differ: {}", byte_difference(&e, &a));
}

#[test]
fn casts_are_the_references_bytes() {
    cast("default", &["--record-seconds", "2"]);
    cast(
        "hawks-flocks",
        &["--hawks", "2", "--flocks", "2", "--record-seconds", "1", "--record-fps", "120"],
    );
    cast(
        "kitty-requested",
        &["--render", "kitty", "--panel", "--record-seconds", "1", "-n", "300"],
    );
    cast(
        "theme",
        &["--color", "theme", "--record-seconds", "1", "--record-size", "400x120", "-n", "100"],
    );
}

/// `.cast` selects a cast only as a suffix of a name longer than itself.
#[test]
fn the_cast_suffix_rule_is_the_references() {
    for name in [".cast", "a.cast", "x.CAST", "x.cast.gif"] {
        let Some(reference) = oracle::reference_binary() else { return };
        let rust = oracle::rust_binary();
        let scratch = Scratch::new("cast-names");
        let c = run(
            &reference,
            &["--record", name, "--record-seconds", "1", "-n", "20"],
            &scratch.path,
        );
        let c_file = std::fs::read(scratch.file(name)).expect("C file");
        let r = run(&rust, &["--record", name, "--record-seconds", "1", "-n", "20"], &scratch.path);
        let r_file = std::fs::read(scratch.file(name)).expect("Rust file");
        assert_eq!(c.code, r.code);
        assert_eq!(c.stdout, r.stdout, "{name}");
        assert_eq!(normalize_cast(&c_file), normalize_cast(&r_file), "{name}");
    }
}

#[test]
fn recording_failures_are_reported_as_the_reference_reports_them() {
    let Some(reference) = oracle::reference_binary() else { return };
    let rust = oracle::rust_binary();
    let scratch = Scratch::new("record-failures");
    std::fs::create_dir_all(scratch.file("dir.gif")).unwrap();
    std::fs::create_dir_all(scratch.file("dir.cast")).unwrap();
    let mut cases: Vec<Vec<&str>> = vec![
        vec!["--record", "no/such/dir/x.gif"],
        vec!["--record", "no/such/dir/x.cast"],
        vec!["--record", "dir.gif"],
        vec!["--record", "dir.cast"],
    ];
    if Path::new("/dev/full").exists() {
        cases.push(vec!["--record", "/dev/full", "-n", "10", "--record-seconds", "1"]);
    }
    for case in cases {
        let c = run(&reference, &case, &scratch.path);
        let r = run(&rust, &case, &scratch.path);
        assert_eq!(c.code, r.code, "{case:?}");
        assert_eq!(c.stdout, r.stdout, "{case:?}");
        assert_eq!(
            String::from_utf8_lossy(&c.stderr),
            String::from_utf8_lossy(&r.stderr),
            "{case:?}"
        );
    }
}

/// The benchmark's report, with the two lines that hold wall-clock
/// measurements replaced by their labels; everything else, byte counts
/// included, must be identical.
fn normalize_bench(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| {
            if line.starts_with("frame time") || line.starts_with("ceiling") {
                // Only the numbers are measurements; the labels and units,
                // and the spacing between them, are compared.
                line.split(' ')
                    .map(|word| if word.parse::<f64>().is_ok() { "<measured>" } else { word })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn benchmarks_report_what_the_reference_reports() {
    let Some(reference) = oracle::reference_binary() else { return };
    let rust = oracle::rust_binary();
    let scratch = Scratch::new("bench");
    for case in [
        vec!["--bench", "30"],
        vec![
            "--bench", "20", "--hawks", "4", "--flocks", "3", "--depth", "--trails", "--speed",
            "12",
        ],
        vec!["--bench", "20", "--render", "braille", "--panel"],
        vec!["--bench", "10", "--render", "sextants", "-n", "4096"],
        vec!["--bench", "10", "--render", "blocks", "--matrix"],
        vec!["--bench", "0", "--record", "x.gif", "--record-seconds", "1", "-n", "10"],
        vec!["--bench", "5", "--record", "never-written.gif"],
    ] {
        let c = run(&reference, &case, &scratch.path);
        let r = run(&rust, &case, &scratch.path);
        assert_eq!(c.code, r.code, "{case:?}");
        assert_eq!(normalize_bench(&c.stdout), normalize_bench(&r.stdout), "{case:?}");
        assert_eq!(c.stderr, r.stderr, "{case:?}");
    }
}

/// Every rate --record-fps accepts, as a GIF and as a cast.
#[test]
fn every_recording_rate_matches_the_reference() {
    for fps in 2..=120 {
        let fps = fps.to_string();
        let small = [
            "--record-fps",
            fps.as_str(),
            "--record-seconds",
            "1",
            "-n",
            "5",
            "--record-size",
            "40x14",
        ];
        gif(&format!("rate-{fps}"), &small);
        cast(&format!("rate-{fps}"), &small);
    }
}

/// Which mode wins, and which options a headless mode ignores.
#[test]
fn mode_precedence_is_the_references() {
    gif(
        "record-ignores-snapshot-and-frames",
        &["--record-seconds", "1", "-n", "20", "--snapshot", "s.png", "--frames", "3"],
    );
    gif("record-with-panel", &["--record-seconds", "1", "-n", "20", "--panel"]);
    cast(
        "cast-ignores-render-and-panel",
        &["--render", "sextants", "--panel", "--record-seconds", "1", "-n", "20"],
    );
}
