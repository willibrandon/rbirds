#![cfg(unix)]

//! Cross-process CLI comparison (docs/COMPATIBILITY.md C01, C16): the same
//! argument vectors given to the canonical C build and to rbirds, comparing
//! exit status and both output streams.
//!
//! Both run with the same `argv[0]` and an empty environment, standard input
//! from /dev/null (so a live run fails at raw mode, deterministically, after
//! everything read_options does), in a scratch directory holding the files
//! the corpus names. Normalization covers product identity in the
//! places docs/COMPATIBILITY.md §1 allows: the tagline, examples and
//! completion program name print `cbirds` where rbirds prints `rbirds`, and
//! the version line names each product's own version. The explicit Sixel CLI
//! extension (D-004) is applied to the C's expected help and choice listings.

mod support;

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};

use support::oracle::{self, Scratch};

#[derive(Debug, PartialEq, Eq)]
struct Run {
    status: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(exe: &Path, args: &[OsString], dir: &Path) -> Run {
    let output = Command::new(exe)
        .arg0("birds")
        .args(args)
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("run program");
    let status = match (output.status.code(), output.status.signal()) {
        (Some(code), _) => format!("exit {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        _ => "unknown".into(),
    };
    Run { status, stdout: output.stdout, stderr: output.stderr }
}

/// The identity normalization, and nothing else: whole-word `cbirds` in the
/// C's help, completion and tagline text becomes `rbirds`, and the version
/// line becomes the Rust product's.
fn normalize_identity(stdout: &[u8]) -> Vec<u8> {
    let text = stdout;
    if text == b"cbirds 1.4.0\n" {
        return format!("rbirds {}\n", env!("CARGO_PKG_VERSION")).into_bytes();
    }
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let word_start = i == 0 || !text[i - 1].is_ascii_alphanumeric();
        let rest = &text[i..];
        if word_start && rest.starts_with(b"cbirds") {
            let end = i + 6;
            let word_end = end >= text.len() || !text[end].is_ascii_alphanumeric();
            if word_end {
                out.extend_from_slice(b"rbirds");
                i = end;
                continue;
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

#[test]
fn the_identity_normalization_changes_nothing_else() {
    assert_eq!(normalize_identity(b"cbirds --x"), b"rbirds --x");
    assert_eq!(normalize_identity(b"_cbirds() {"), b"_rbirds() {");
    // Neither a longer word nor any other difference is absorbed.
    assert_eq!(normalize_identity(b"cbirdsx xcbirds"), b"cbirdsx xcbirds");
    assert_eq!(normalize_identity(b"cbirds 1.3.0\n"), b"rbirds 1.3.0\n");
    assert_ne!(normalize_identity(b"cbirds --birds 800"), b"rbirds --birds 801");
}

fn args(list: &[&[u8]]) -> Vec<OsString> {
    list.iter().map(|a| OsString::from_vec(a.to_vec())).collect()
}

/// Only the approved CLI additions; byte replacement preserves invalid UTF-8
/// diagnostics and does not weaken comparison of the rest of either stream.
fn extend_sixel_cli(bytes: &[u8]) -> Vec<u8> {
    let mut result = bytes.to_vec();
    for (old, new) in [
        (
            "braille by default; sextants, blocks, or kitty in Kitty and Ghostty",
            "braille (default), sextants, blocks, kitty, sixel",
        ),
        ("kitty braille sextants blocks", "kitty braille sextants blocks sixel"),
        (
            "--render must be one of kitty, braille, sextants, blocks\n",
            "--render must be one of kitty, braille, sextants, blocks, sixel\n",
        ),
        (
            "sprites, in Kitty or Ghostty\n",
            "sprites, in Kitty or Ghostty\n  rbirds --render sixel            pixels, in Windows Terminal 1.22+\n",
        ),
    ] {
        let mut output = Vec::new();
        let mut rest = result.as_slice();
        while let Some(at) = rest.windows(old.len()).position(|part| part == old.as_bytes()) {
            output.extend_from_slice(&rest[..at]);
            output.extend_from_slice(new.as_bytes());
            rest = &rest[at + old.len()..];
        }
        output.extend_from_slice(rest);
        result = output;
    }
    result
}

#[test]
fn sixel_normalization_preserves_other_choices_errors_and_raw_bytes() {
    assert_eq!(
        extend_sixel_cli(b"\xff kitty braille sextants blocks"),
        b"\xff kitty braille sextants blocks sixel"
    );
    assert_eq!(
        extend_sixel_cli(b"--shape must be one of bird, plane\n"),
        b"--shape must be one of bird, plane\n"
    );
    assert_eq!(
        extend_sixel_cli(b"kitty braille sextants missing"),
        b"kitty braille sextants missing"
    );
}

fn corpus(files: &Path) -> Vec<Vec<OsString>> {
    let file = |name: &str| files.join(name).into_os_string().into_vec();
    let owned: Vec<Vec<Vec<u8>>> = vec![
        vec![b"--sprite".to_vec(), file("missing.png")],
        vec![b"--sprite".to_vec(), file("a-directory")],
        vec![b"--sprite".to_vec(), file("huge.png")],
        vec![b"--sprite".to_vec(), file("exactly-4mb.bin")],
        vec![b"--sprite".to_vec(), file("text.png")],
        vec![b"--sprite".to_vec(), file("truncated.png")],
        vec![b"--sprite".to_vec(), file("ok.png"), b"--flocks".to_vec(), b"2".to_vec()],
        vec![b"--sprite".to_vec(), file("ok.png"), b"--help".to_vec()],
        vec![b"--sprite=".to_vec()],
        vec![b"--record".to_vec(), b"x.gif".to_vec(), b"--record-size".to_vec(), b"10x10".to_vec()],
    ];
    let mut all: Vec<Vec<OsString>> =
        owned.into_iter().map(|list| list.into_iter().map(OsString::from_vec).collect()).collect();
    let fixed: &[&[&[u8]]] = &[
        &[],
        &[b"-h"],
        &[b"--help"],
        &[b"-V"],
        &[b"--version"],
        &[b"--completion", b"bash"],
        &[b"--completion", b"zsh"],
        &[b"--completion", b"fish"],
        &[b"--completion", b"tcsh"],
        &[b"--completion"],
        &[b"--completion="],
        &[b"--help", b"--birds", b"x"],
        &[b"--birds", b"x", b"--help"],
        &[b"-V", b"--bogus"],
        &[b"--bogus", b"-V"],
        &[b"--bogus"],
        &[b"--bird"],
        &[b"--brids", b"3"],
        &[b"-x"],
        &[b"-"],
        &[b"--"],
        &[b"--", b"x"],
        &[b"x"],
        &[b"--birds"],
        &[b"--birds="],
        &[b"--birds=0"],
        &[b"--birds=4097"],
        &[b"-n800"],
        &[b"-n", b"1e3"],
        &[b"-n", b"0x10"],
        &[b"-n", b" 12"],
        &[b"-n", b"12.5"],
        &[b"-n", b"inf"],
        &[b"-n", b"nan"],
        &[b"-n", b"1e400"],
        &[b"--seed", b"-1"],
        &[b"--seed", b"2147483648"],
        &[b"--preset", b"storm", b"--speed", b"9"],
        &[b"--preset", b"nope"],
        &[b"--preset"],
        &[b"--avoidance", b"3"],
        &[b"--flocks", b"3", b"--avoidance", b"10"],
        &[b"--flocks", b"4"],
        &[b"--groups", b"2"],
        &[b"--palette", b"ice"],
        &[b"--color", b"THEME"],
        &[b"-c"],
        &[b"--shape", b"dot"],
        &[b"--render", b"auto"],
        &[b"--render", b"kitty", b"--render", b"blocks"],
        &[b"--record-size", b"100x50"],
        &[b"--record-size", b"39x14"],
        &[b"--record-size", b"401x14"],
        &[b"--record-size", b"40x13"],
        &[b"--record-size", b"100x"],
        &[b"--record-size", b"x50"],
        &[b"--record-size", b"100x50 "],
        &[b"--record-size", b"+100x50"],
        &[b"--record-size", b"99999999999x50"],
        &[b"--record-fps", b"1"],
        &[b"--record-fps", b"121"],
        &[b"--record-seconds", b"0"],
        &[b"--matrix"],
        &[b"--matrix", b"--color", b"ice"],
        &[b"--no-trails"],
        &[b"--no-birds"],
        &[b"--no-panel", b"--no-depth"],
        &[b"-le"],
        &[b"-elk2"],
        &[b"-el", b"-k", b"9"],
        &[b"-ek"],
        &[b"--trails=1"],
        &[b"--bench", b"-1"],
        &[b"--frames", b"1000001"],
        &[b"--unlock-fps", b"--frames", b"3"],
        &[b"--birds\xff"],
        &[b"--\xff\xfe"],
        &[b"-\xff"],
        &[b"--preset", b"\xffstorm"],
        &[b"--record-size", b"\xff"],
        &[
            b"--aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
    ];
    all.extend(fixed.iter().map(|list| args(list)));
    all
}

fn write_fixtures(dir: &Path) {
    std::fs::create_dir_all(dir.join("a-directory")).unwrap();
    std::fs::write(dir.join("huge.png"), vec![0_u8; (1 << 22) + 1]).unwrap();
    std::fs::write(dir.join("exactly-4mb.bin"), vec![0_u8; 1 << 22]).unwrap();
    std::fs::write(dir.join("text.png"), b"not a png at all\n").unwrap();
    let sprite = std::fs::read(oracle::repository().join("assets/sprite.png")).unwrap();
    std::fs::write(dir.join("ok.png"), &sprite).unwrap();
    std::fs::write(dir.join("truncated.png"), &sprite[..sprite.len() / 2]).unwrap();
}

#[test]
fn every_argument_vector_behaves_as_the_reference() {
    let Some(reference) = oracle::reference_binary() else { return };
    let rust = oracle::rust_binary();
    let scratch = Scratch::new("cli");
    write_fixtures(&scratch.path);
    let mut failures = Vec::new();
    for case in corpus(&scratch.path) {
        let c = run(&reference, &case, &scratch.path);
        let r = run(&rust, &case, &scratch.path);
        let expected = Run {
            status: c.status.clone(),
            stdout: extend_sixel_cli(&normalize_identity(&c.stdout)),
            stderr: extend_sixel_cli(&c.stderr),
        };
        if expected != r {
            let shown: Vec<String> =
                case.iter().map(|a| String::from_utf8_lossy(a.as_bytes()).into_owned()).collect();
            failures.push(format!(
                "{shown:?}\n  C:    {} stdout={:?} stderr={:?}\n  Rust: {} stdout={:?} stderr={:?}",
                expected.status,
                String::from_utf8_lossy(&expected.stdout),
                String::from_utf8_lossy(&expected.stderr),
                r.status,
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of the CLI corpus differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// argv[0] is every message's prefix, as invoked, bytes and all.
#[test]
fn the_program_name_is_taken_as_invoked() {
    let Some(reference) = oracle::reference_binary() else { return };
    let rust = oracle::rust_binary();
    let scratch = Scratch::new("cli-name");
    for name in [&b"cbirds"[..], b"/usr/local/bin/x", b"\xffname", b""] {
        let with_name = |exe: &Path| {
            Command::new(exe)
                .arg0(OsString::from_vec(name.to_vec()))
                .args(["--bogus"])
                .env_clear()
                .current_dir(&scratch.path)
                .stdin(Stdio::null())
                .output()
                .expect("run")
        };
        let c = with_name(&reference);
        let r = with_name(&rust);
        assert_eq!(c.status.code(), r.status.code());
        assert_eq!(c.stderr, r.stderr, "argv[0] {:?}", String::from_utf8_lossy(name));
    }
}
