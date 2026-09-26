//! The global state each cbirds boids test is entered with, captured by
//! running the unmodified C suite (`tools/oracle/boids_suite.c`) once per
//! test process, so every translated test in tests/c_boids.rs starts exactly
//! where the C one did, including random state, panel switch and hawks.

use std::collections::HashMap;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[cfg(unix)]
use super::oracle;
use super::sim::World;

#[cfg(unix)]
const SOURCES: &[&str] =
    &["cells.c", "font.c", "gif.c", "kitty_graphics.c", "options.c", "png.c", "spatial_grid.c"];

static ENTRIES: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();

#[cfg(windows)]
fn capture() -> Option<HashMap<String, String>> {
    // The C suite is POSIX-only. These are its recorded INPUT states, not
    // expected Windows results; the translated assertions still execute.
    Some(parse_entries(include_str!("../fixtures/boids-suite-linux-x64.txt")))
}

#[cfg(unix)]
fn capture() -> Option<HashMap<String, String>> {
    let exe = oracle::build("boids_suite", "boids_suite.c", SOURCES)?;
    let scratch = oracle::repository().join("target/scratch/boids-suite");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("scratch");
    let output = Command::new(&exe)
        .current_dir(&scratch)
        .env("TMPDIR", &scratch)
        .env_remove("COLORTERM")
        .stdin(Stdio::null())
        .output()
        .expect("run the C suite");
    assert!(
        output.status.success(),
        "the C suite itself failed under observation: {}",
        output.status
    );
    let text = String::from_utf8(output.stderr).expect("dumps are ASCII");
    let entries = parse_entries(&text);
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    assert_eq!(
        entries,
        parse_entries(include_str!("../fixtures/boids-suite-linux-x64.txt")),
        "the recorded Windows input fixtures must match the pinned C suite"
    );
    Some(entries)
}

fn parse_entries(text: &str) -> HashMap<String, String> {
    let mut entries = HashMap::new();
    let mut name = None;
    let mut body = String::new();
    for line in text.lines() {
        if let Some(test) = line.strip_prefix("entry ") {
            name = Some(test.to_owned());
            body.clear();
        } else if line == "end" {
            entries.insert(name.take().expect("entry before end"), std::mem::take(&mut body));
        } else if name.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    assert_eq!(entries.len(), 64, "the suite enters 64 tests");
    entries
}

/// The dump of the state `test` was entered with, or `None` where oracle
/// tests may be skipped and the reference is unavailable.
pub fn entry_dump(test: &str) -> Option<String> {
    let entries = ENTRIES.get_or_init(capture).as_ref()?;
    Some(entries.get(test).unwrap_or_else(|| panic!("no C suite entry named {test}")).clone())
}

/// The C suite's state as `test` begins, loaded into the port.
pub fn entry(test: &str) -> Option<World> {
    let dump = entry_dump(test)?;
    let mut world = World::load(&dump);
    // The load must be complete: dumping it again reproduces every line the
    // C printed, apart from the settings line only the suite adds.
    let again = world.dump();
    let expected: String =
        dump.lines().filter(|l| !l.starts_with("settings ")).map(|l| format!("{l}\n")).collect();
    assert_eq!(again, expected, "{test}: the entry state did not load completely");
    Some(world)
}
