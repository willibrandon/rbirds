//! The C test inventory is complete and every row maps to real Rust tests
//! (docs/PORTING.md §4.A).
//!
//! The pinned suites' `main()` calls are rediscovered from the reference
//! sources and compared with docs/c-test-inventory.csv: no missing, duplicate
//! or extra C tests. Every row must name at least one Rust test as
//! `tests/FILE.rs::NAME`, which must exist as a `#[test]` without `#[ignore]`.
//! Whether each named test actually ran is checked by tools/verify.sh against
//! `cargo test -- --list`.

mod support;

use std::collections::BTreeMap;
#[cfg(unix)]
use std::collections::BTreeSet;
use std::fs;

use support::oracle;

#[cfg(unix)]
const SUITES: [&str; 7] = [
    "tests/boids_test.c",
    "tests/cells_test.c",
    "tests/gif_test.c",
    "tests/kitty_graphics_test.c",
    "tests/options_test.c",
    "tests/png_test.c",
    "tests/spatial_grid_test.c",
];

/// The `test_*();` statements inside a suite's `int main(void)`.
#[cfg(unix)]
fn main_calls(source: &str) -> Vec<String> {
    let Some(start) = source.find("int main(void)") else { return Vec::new() };
    let body = &source[start..];
    let end = body.find("\n}").unwrap_or(body.len());
    body[..end]
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let name = line.strip_suffix("();")?;
            name.starts_with("test_").then(|| name.to_owned())
        })
        .collect()
}

struct Row {
    file: String,
    test: String,
    rust_tests: Vec<String>,
    status: String,
}

fn inventory() -> Vec<Row> {
    let text = fs::read_to_string(oracle::repository().join("docs/c-test-inventory.csv"))
        .expect("inventory");
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            assert_eq!(fields.len(), 7, "malformed inventory row: {line}");
            Row {
                file: fields[0].to_owned(),
                test: fields[1].to_owned(),
                rust_tests: fields[4]
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                status: fields[6].to_owned(),
            }
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn the_inventory_is_exactly_the_reference_suites() {
    let reference = oracle::reference_dir();
    if !reference.join("boids.c").is_file() {
        assert!(oracle::skipping_allowed(), "no reference at {}", reference.display());
        eprintln!("SKIPPED: no reference to rediscover the suites from");
        return;
    }
    let mut discovered = BTreeSet::new();
    for suite in SUITES {
        let source = fs::read_to_string(reference.join(suite)).expect("suite source");
        for call in main_calls(&source) {
            assert!(
                discovered.insert((suite.to_owned(), call.clone())),
                "{suite} calls {call} twice"
            );
        }
    }
    let mut listed = BTreeSet::new();
    for row in inventory() {
        assert!(
            listed.insert((row.file.clone(), row.test.clone())),
            "inventory lists {} {} twice",
            row.file,
            row.test
        );
    }
    let missing: Vec<_> = discovered.difference(&listed).collect();
    let extra: Vec<_> = listed.difference(&discovered).collect();
    assert!(missing.is_empty(), "C tests missing from the inventory: {missing:?}");
    assert!(extra.is_empty(), "inventory rows with no C test: {extra:?}");
    assert_eq!(discovered.len(), 112, "the pinned suites call 112 tests");
}

/// Every `#[test]` function of a Rust test file, and whether it is ignored.
fn rust_tests_in(file: &str) -> BTreeMap<String, bool> {
    let text = fs::read_to_string(oracle::repository().join(file))
        .unwrap_or_else(|_| panic!("{file} named by the inventory does not exist"));
    let mut tests = BTreeMap::new();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("fn ") else { continue };
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        // The attributes directly above the function.
        let mut j = i;
        let (mut is_test, mut ignored) = (false, false);
        while j > 0 {
            j -= 1;
            let attribute = lines[j].trim();
            if !attribute.starts_with("#[") {
                break;
            }
            is_test |= attribute == "#[test]";
            ignored |= attribute.starts_with("#[ignore");
        }
        if is_test {
            tests.insert(name, ignored);
        }
    }
    tests
}

#[test]
fn every_inventory_row_maps_to_a_live_rust_test() {
    let mut unmapped = Vec::new();
    let mut broken = Vec::new();
    let mut files: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for row in inventory() {
        if row.rust_tests.is_empty() {
            unmapped.push(format!("{} {}", row.file, row.test));
            continue;
        }
        assert_ne!(row.status, "pending", "{} {} is mapped but still pending", row.file, row.test);
        for target in &row.rust_tests {
            let Some((file, name)) = target.split_once("::") else {
                broken.push(format!("{target}: not FILE::NAME"));
                continue;
            };
            let tests = files.entry(file.to_owned()).or_insert_with(|| rust_tests_in(file));
            match tests.get(name) {
                None => broken.push(format!("{target}: no such #[test]")),
                Some(true) => broken.push(format!("{target}: ignored")),
                Some(false) => {}
            }
        }
    }
    assert!(broken.is_empty(), "inventory mappings that do not resolve: {broken:#?}");
    assert!(unmapped.is_empty(), "{} C tests have no Rust mapping: {unmapped:#?}", unmapped.len());
}
