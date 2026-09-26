//! Every place the canonical C build fuses a multiply-add is a tagged
//! `mul_add` in the Rust port, and no other place is (see src/fp.rs).
//!
//! docs/evidence/fma-sites.txt is generated from the pinned reference's debug
//! info by tools/oracle/fma-sites.sh. This test holds the `fma: FILE:LINE:COL`
//! tags in src/ to that list exactly, counts `mul_add(` calls against the
//! tags, and, where a compiler is available, regenerates the list to show the
//! checked-in copy has not drifted from the reference.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use support::oracle;

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push((path.display().to_string(), fs::read_to_string(&path).expect("source")));
        }
    }
}

fn listed_sites() -> BTreeMap<String, usize> {
    let text = fs::read_to_string(oracle::repository().join("docs/evidence/fma-sites.txt"))
        .expect("fma site list");
    let mut sites = BTreeMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        *sites.entry(line.trim().to_owned()).or_insert(0) += 1;
    }
    sites
}

#[test]
fn every_contraction_site_is_tagged_once_and_nothing_else_is() {
    let mut files = Vec::new();
    sources(&oracle::repository().join("src"), &mut files);
    let mut tagged: BTreeMap<String, usize> = BTreeMap::new();
    let mut calls = 0;
    let mut tags = 0;
    for (path, text) in &files {
        if path.ends_with("fp.rs") {
            continue; // The definition and its own tests.
        }
        for line in text.lines() {
            if let Some(at) = line.find("fma: ") {
                let site = line[at + 5..].trim().trim_end_matches("*/").trim().to_owned();
                *tagged.entry(site).or_insert(0) += 1;
                tags += 1;
            }
            calls += line.matches("mul_add(").count();
        }
    }
    let listed = listed_sites();
    let missing: Vec<_> = listed.keys().filter(|s| !tagged.contains_key(*s)).collect();
    let invented: Vec<_> = tagged.keys().filter(|s| !listed.contains_key(*s)).collect();
    let repeated: Vec<_> = tagged.iter().filter(|(_, n)| **n > 1).collect();
    assert!(missing.is_empty(), "C contraction sites with no Rust tag: {missing:?}");
    assert!(invented.is_empty(), "Rust tags naming no C site: {invented:?}");
    assert!(repeated.is_empty(), "sites tagged more than once: {repeated:?}");
    // One call per tag: `use` lines and doc mentions are excluded by the
    // `mul_add(` spelling, and imports never carry a parenthesis.
    assert_eq!(calls, tags, "mul_add calls ({calls}) and site tags ({tags}) must pair up");
}

#[test]
fn the_site_list_is_what_the_reference_produces() {
    if !cfg!(all(target_arch = "aarch64", target_vendor = "apple")) {
        // The list describes the Apple clang control; elsewhere the canonical
        // control does not contract at all and the list is inert.
        return;
    }
    let reference = oracle::reference_dir();
    if !reference.join("boids.c").is_file() {
        if oracle::skipping_allowed() {
            eprintln!("SKIPPED: no reference to regenerate the site list from");
            return;
        }
        panic!("no reference at {}", reference.display());
    }
    let script = oracle::repository().join("tools/oracle/fma-sites.sh");
    let output = Command::new("sh").arg(&script).arg(&reference).output().expect("run fma-sites.sh");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let generated: BTreeMap<String, usize> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .fold(BTreeMap::new(), |mut sites, line| {
            *sites.entry(line.trim().to_owned()).or_insert(0) += 1;
            sites
        });
    assert_eq!(generated, listed_sites(), "docs/evidence/fma-sites.txt is stale");
}

/// Every sine and cosine goes through `fp::sin_cos`, because the canonical
/// build fuses every one of them (docs/evidence/trig-sites.txt).
#[test]
fn no_sine_or_cosine_bypasses_the_fused_pair() {
    let mut files = Vec::new();
    sources(&oracle::repository().join("src"), &mut files);
    let mut plain = Vec::new();
    for (path, text) in &files {
        if path.ends_with("platform/trig.rs") {
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if code.contains(".sin()")
                || code.contains(".cos()")
                || code.contains(".sin_cos()")
                || code.contains("f64::sin")
                || code.contains("f64::cos")
            {
                plain.push(format!("{path}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(plain.is_empty(), "plain sine or cosine calls: {plain:#?}");
}

#[test]
fn the_trig_site_list_says_every_call_is_fused() {
    let text = fs::read_to_string(oracle::repository().join("docs/evidence/trig-sites.txt"))
        .expect("trig site list");
    let calls: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert!(!calls.is_empty());
    assert!(calls.iter().all(|l| l.ends_with(" __sincos_stret")), "{calls:?}");
}
