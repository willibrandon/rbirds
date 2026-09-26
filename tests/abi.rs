//! The ABI gate for `rbirds::platform` (docs/DESIGN.md §6, PORTING.md §4.D):
//! `tools/oracle/abi_probe.c`, compiled against this machine's native headers
//! for the same target as this test binary, must print exactly the report the
//! Rust bindings print. Also checks the strerror wrapper against C's own
//! `strerror` for every errno value the probe lists.
//!
//! The probe needs no cbirds reference, only a C compiler (`CC`, else `cc`).
//! A missing compiler fails the test unless `RBIRDS_NO_ORACLE=1`, which turns
//! it into a printed skip that does not count as ABI evidence.

// tests/support/oracle.rs (not this file's to change) trips clippy::collapsible_if
// under Rust 1.96 now that an integration test compiles it.
#[allow(clippy::collapsible_if)]
mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use rbirds::platform;
use support::oracle;

/// The compiler flags that select this test binary's own target, so an
/// x86_64 test run under Rosetta checks an x86_64 probe.
fn target_flags() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "x86_64") { &["-arch", "x86_64"] } else { &["-arch", "arm64"] }
    } else {
        // GNU/Linux: the native compiler of the machine running the tests.
        &[]
    }
}

/// Builds the probe once per test process into target/oracle/.
fn probe() -> Option<PathBuf> {
    static PROBE: OnceLock<Option<PathBuf>> = OnceLock::new();
    PROBE
        .get_or_init(|| {
            let source = oracle::repository().join("tools/oracle/abi_probe.c");
            let out_dir = oracle::repository().join("target/oracle");
            std::fs::create_dir_all(&out_dir).expect("create target/oracle");
            let arch = if cfg!(target_arch = "x86_64") { "x86_64" } else { "aarch64" };
            let os = std::env::consts::OS;
            let exe = out_dir.join(format!("abi_probe-{os}-{arch}"));
            let partial =
                out_dir.join(format!("abi_probe-{os}-{arch}.partial.{}", std::process::id()));
            let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
            let compiled = Command::new(&cc)
                .args(target_flags())
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O0"])
                .arg(&source)
                .arg("-o")
                .arg(&partial)
                .output();
            match compiled {
                Ok(out) if out.status.success() => {
                    std::fs::rename(&partial, &exe).expect("install the ABI probe");
                    Some(exe)
                }
                Ok(out) => panic!(
                    "the ABI probe failed to compile:\n{}",
                    String::from_utf8_lossy(&out.stderr)
                ),
                Err(e) if oracle::skipping_allowed() => {
                    eprintln!("SKIPPED: no C compiler for the ABI probe ({cc:?}: {e})");
                    None
                }
                Err(e) => panic!(
                    "cannot run {cc:?} for the ABI probe: {e} \
                     (set RBIRDS_NO_ORACLE=1 to skip explicitly)"
                ),
            }
        })
        .clone()
}

fn run_probe(args: &[&str]) -> Option<String> {
    let exe = probe()?;
    let out = Command::new(&exe).args(args).output().expect("run the ABI probe");
    assert!(out.status.success(), "the ABI probe failed: {}", out.status);
    Some(String::from_utf8(out.stdout).expect("the probe prints ASCII"))
}

#[test]
fn rust_bindings_match_the_native_headers() {
    let Some(native) = run_probe(&[]) else { return };
    let rust = platform::abi::rust_layout_report();
    let native_lines: Vec<&str> = native.lines().collect();
    let rust_lines: Vec<&str> = rust.lines().collect();
    let mut problems = Vec::new();
    for (i, (c, r)) in native_lines.iter().zip(&rust_lines).enumerate() {
        if c != r {
            problems.push(format!("line {}: C {c:?} != Rust {r:?}", i + 1));
        }
    }
    if native_lines.len() != rust_lines.len() {
        problems.push(format!("C printed {} lines, Rust {}", native_lines.len(), rust_lines.len()));
    }
    assert!(problems.is_empty(), "ABI mismatch on this target:\n{}", problems.join("\n"));
    assert!(!native.contains("MISMATCH"), "a header prototype differs:\n{native}");
    eprintln!("ABI report: {} identical lines", native_lines.len());
}

#[test]
fn strerror_matches_c_strerror() {
    let Some(table) = run_probe(&["strerror"]) else { return };
    let mut checked = 0;
    for line in table.lines() {
        let rest = line.strip_prefix("strerror ").expect("strerror line");
        let (number, text) = rest.split_once(' ').expect("number and text");
        let errnum: i32 = number.parse().expect("errno value");
        let ours = platform::strerror(errnum);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            text,
            "strerror({errnum}) differs from the C library's"
        );
        checked += 1;
    }
    assert_eq!(checked, 142, "errno -1..=140");
}

#[test]
fn perror_message_matches_c_perror_text() {
    // perror(s) is "s: " + strerror(errno) + "\n" in both libraries; the text
    // comes from the table above, so this pins only the framing.
    let Some(table) = run_probe(&["strerror"]) else { return };
    let enotty = table
        .lines()
        .find_map(|l| l.strip_prefix(&format!("strerror {} ", platform::ENOTTY)))
        .expect("ENOTTY line")
        .to_owned();
    assert_eq!(
        platform::perror_message(b"Can't enable raw mode", platform::ENOTTY),
        format!("Can't enable raw mode: {enotty}\n").into_bytes()
    );
}
