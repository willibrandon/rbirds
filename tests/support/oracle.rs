//! Builds and runs C observation programs against the pinned cbirds reference.
//!
//! An oracle is a small C program under `tools/oracle/` compiled together with
//! the unmodified reference sources, with the canonical flags (`-std=c99 -O3
//! -g`, the platform's own `cc`), so that what it prints is what the reference
//! computes on this target, floating-point contraction included. Oracles only
//! observe: they call the reference's functions and print the results.
//!
//! The reference is `.reference/cbirds` in the repository, or `RBIRDS_REFERENCE`.
//! A missing reference or compiler fails the test. `RBIRDS_NO_ORACLE=1`
//! downgrades that to a printed skip, for machines that cannot host the
//! reference; a report from such a run does not count as oracle evidence.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;

/// The pinned reference commit; an oracle refuses any other checkout.
pub const REFERENCE_COMMIT: &str = "cc446fc3cb80733371c62676533adcac2fc10002";

pub fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn reference_dir() -> PathBuf {
    match std::env::var_os("RBIRDS_REFERENCE") {
        Some(dir) => PathBuf::from(dir),
        None => repository().join(".reference/cbirds"),
    }
}

/// Whether oracle tests may be skipped on this machine.
pub fn skipping_allowed() -> bool {
    std::env::var_os("RBIRDS_NO_ORACLE").is_some_and(|v| v == "1")
}

fn unavailable(why: &str) -> Option<PathBuf> {
    if skipping_allowed() {
        eprintln!("SKIPPED: oracle unavailable: {why}");
        None
    } else {
        panic!("C oracle unavailable: {why} (set RBIRDS_NO_ORACLE=1 to skip explicitly)");
    }
}

fn check_reference(reference: &Path) -> Result<(), String> {
    if !reference.join("boids.c").is_file() {
        return Err(format!("no reference sources at {}", reference.display()));
    }
    let head = Command::new("git").arg("-C").arg(reference).args(["rev-parse", "HEAD"]).output();
    match head {
        Ok(out) if out.status.success() => {
            let head = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if head != REFERENCE_COMMIT {
                return Err(format!("reference is at {head}, not {REFERENCE_COMMIT}"));
            }
        }
        _ => return Err(format!("cannot read the commit of {}", reference.display())),
    }
    let dirty = Command::new("git")
        .arg("-C")
        .arg(reference)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .map_err(|e| e.to_string())?;
    if !dirty.stdout.is_empty() {
        return Err(format!("reference checkout {} has tracked changes", reference.display()));
    }
    Ok(())
}

static BUILD_LOCK: Mutex<()> = Mutex::new(());

/// Compiles `tools/oracle/<program>.c` with the named reference sources into
/// `target/oracle/<name>` and returns the executable, rebuilding when any input
/// is newer. `None` only when skipping is explicitly allowed.
pub fn build(name: &str, program: &str, reference_sources: &[&str]) -> Option<PathBuf> {
    build_with(name, program, reference_sources, &[])
}

/// As [`build`], with extra compiler arguments such as `-DNAME=VALUE`.
pub fn build_with(
    name: &str,
    program: &str,
    reference_sources: &[&str],
    extra: &[&str],
) -> Option<PathBuf> {
    let _guard = BUILD_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    let reference = reference_dir();
    if let Err(why) = check_reference(&reference) {
        return unavailable(&why);
    }
    let out_dir = repository().join("target/oracle");
    if let Err(e) = fs::create_dir_all(&out_dir) {
        return unavailable(&format!("cannot create {}: {e}", out_dir.display()));
    }
    let exe = out_dir.join(name);
    let program_path = repository().join("tools/oracle").join(program);
    let mut inputs = vec![program_path.clone()];
    inputs.extend(reference_sources.iter().map(|s| reference.join(s)));
    let newest_input = inputs.iter().filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok()).max();
    let built = fs::metadata(&exe).and_then(|m| m.modified()).ok();
    if let (Some(input), Some(built)) = (newest_input, built) {
        if built >= input && extra.is_empty() {
            return Some(exe);
        }
    }
    let partial = out_dir.join(format!("{name}.partial.{}", std::process::id()));
    let mut cc = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()));
    cc.args(["-std=c99", "-Wall", "-Wextra", "-O3", "-g"])
        .arg(format!("-I{}", reference.display()))
        .args(extra)
        .arg(&program_path);
    for source in reference_sources {
        cc.arg(reference.join(source));
    }
    cc.arg("-o").arg(&partial).arg("-lm");
    match cc.output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => panic!(
            "oracle {name} failed to compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => return unavailable(&format!("cannot run cc: {e}")),
    }
    fs::rename(&partial, &exe).expect("install oracle");
    Some(exe)
}

/// Runs an oracle with `args`, feeding `stdin`, and insists it succeeded.
pub fn run(exe: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let output = run_status(exe, args, stdin);
    assert!(
        output.status.success(),
        "oracle {} {:?} failed: {}\n{}",
        exe.display(),
        args,
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Runs an oracle and returns whatever happened.
pub fn run_status(exe: &Path, args: &[&str], stdin: &[u8]) -> Output {
    use std::io::Write;
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot run oracle {}: {e}", exe.display()));
    let mut input = child.stdin.take().expect("oracle stdin");
    let data = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let output = child.wait_with_output().expect("oracle output");
    let _ = writer.join();
    output
}

/// A scratch directory unique to this process and label, removed on drop
/// unless the test panicked, so failed inputs stay available for diagnosis.
pub struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Scratch {
        let base = repository().join("target/scratch");
        let path = base.join(format!("{label}.{}.{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch directory");
        Scratch { path }
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            eprintln!("kept scratch files for diagnosis: {}", self.path.display());
        }
    }
}
