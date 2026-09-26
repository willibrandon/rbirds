#![cfg(unix)]

//! The recordings cbirds publishes in its README (`docs/*.gif` and
//! `docs/demo.cast`), made again by rbirds with the commands listed in cbirds'
//! own `docs/README.md`.
//!
//! cbirds' author made them with GCC on Linux, and on Linux rbirds makes them
//! exactly as published (the cast apart from its header line, which holds a
//! timestamp and the title). On macOS the canonical C build fuses
//! multiply-adds and uses Apple's libm, so it doesn't make the published files
//! either; there each recording is compared with the pinned C build's.

mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use support::oracle::{self, Scratch};

/// Whether the canonical C build on this target makes the published files.
const MAKES_THE_PUBLISHED_FILES: bool = cfg!(all(target_os = "linux", target_env = "gnu"));

/// The `./cbirds --record docs/NAME ...` commands in cbirds' docs/README.md,
/// as the file name and the arguments after it.
fn documented_commands() -> Vec<(String, Vec<String>)> {
    let readme = fs::read_to_string(oracle::reference_dir().join("docs/README.md"))
        .expect("read cbirds docs/README.md");
    readme
        .replace("\\\n", " ")
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("./cbirds --record docs/")?;
            let mut words = rest.split_whitespace().map(str::to_owned);
            Some((words.next()?, words.collect()))
        })
        .collect()
}

fn record(program: &Path, output: &Path, args: &[String]) {
    let status = Command::new(program)
        .arg("--record")
        .arg(output)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run the recorder");
    assert!(
        status.success(),
        "{} --record {} {args:?}: {status}",
        program.display(),
        output.display()
    );
}

/// A cast's events: everything after the header line.
fn events(cast: &[u8]) -> &[u8] {
    let start = cast.iter().position(|&b| b == b'\n').map_or(cast.len(), |i| i + 1);
    &cast[start..]
}

fn check(name: &str) {
    let Some(cbirds) = oracle::reference_binary() else { return };
    let commands = documented_commands();
    assert_eq!(commands.len(), 8, "cbirds docs/README.md lists eight recordings");
    let (_, args) = commands.iter().find(|(n, _)| n == name).expect("a documented recording");
    let scratch = Scratch::new(&format!("published-{name}"));
    let ours = scratch.file(name);
    record(&oracle::rust_binary(), &ours, args);
    let (expected_from, expected) = if MAKES_THE_PUBLISHED_FILES {
        let published = oracle::reference_dir().join("docs").join(name);
        (published.clone(), fs::read(published).expect("read the published file"))
    } else {
        let theirs = scratch.file(&format!("c-{name}"));
        record(&cbirds, &theirs, args);
        (theirs.clone(), fs::read(theirs).expect("read the C recording"))
    };
    let actual = fs::read(&ours).expect("read the rbirds recording");
    let (expected, actual) = if name.ends_with(".cast") {
        (events(&expected), events(&actual))
    } else {
        (&expected[..], &actual[..])
    };
    if expected != actual {
        let at = expected.iter().zip(actual).position(|(a, b)| a != b);
        panic!(
            "{name} differs from {}: {} bytes against {}, first difference at {at:?}",
            expected_from.display(),
            actual.len(),
            expected.len()
        );
    }
}

#[test]
fn demo_gif() {
    check("demo.gif");
}

#[test]
fn hawks_gif() {
    check("hawks.gif");
}

#[test]
fn flocks_gif() {
    check("flocks.gif");
}

#[test]
fn matrix_gif() {
    check("matrix.gif");
}

#[test]
fn depth_gif() {
    check("depth.gif");
}

#[test]
fn braille_gif() {
    check("braille.gif");
}

#[test]
fn sextants_gif() {
    check("sextants.gif");
}

#[test]
fn demo_cast() {
    check("demo.cast");
}
