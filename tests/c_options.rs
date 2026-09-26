//! cbirds `tests/options_test.c`, translated: one test per C `test_*`, with
//! the same table, the same arguments and every assertion. Where the C
//! captured output with `fmemopen` into a fixed buffer, this captures into a
//! `Vec<u8>` and also checks the output would have fit that buffer.

use std::ffi::OsString;

use rbirds::options::{self, Example, Kind, OptionSpec, Status};

/// The C test's statics, as one settings struct.
#[derive(Debug)]
struct Settings {
    birds: i32,
    quiet: bool,
    mono: bool,
    palette: i32,
    weight: f64,
    label: Option<OsString>,
}

/// `reset()`.
fn reset() -> Settings {
    Settings { birds: 800, weight: 0.5, quiet: false, mono: false, palette: 0, label: None }
}

static PALETTES: &[&str] = &["mono", "flame", "ice"];

static TABLE: &[OptionSpec<Settings>] = &[
    OptionSpec {
        shorthand: Some(b'n'),
        name: "birds",
        alias: Some("boids"),
        kind: Kind::Int { target: |s| &mut s.birds, min: 1.0, max: 4096.0 },
        metavar: Some("COUNT"),
        help: "how many boids",
        group: "Flock",
        essential: true,
    },
    OptionSpec {
        shorthand: Some(b'w'),
        name: "weight",
        alias: None,
        kind: Kind::Double { target: |s| &mut s.weight, min: 0.0, max: 1.0 },
        metavar: Some("VALUE"),
        help: "a weight",
        group: "Flock",
        essential: false,
    },
    OptionSpec {
        shorthand: Some(b'q'),
        name: "quiet",
        alias: None,
        kind: Kind::Flag(|s| &mut s.quiet),
        metavar: None,
        help: "say less",
        group: "Output",
        essential: true,
    },
    OptionSpec {
        shorthand: Some(b'm'),
        name: "mono",
        alias: None,
        kind: Kind::Flag(|s| &mut s.mono),
        metavar: None,
        help: "one colour",
        group: "Output",
        essential: false,
    },
    OptionSpec {
        shorthand: Some(b'P'),
        name: "palette",
        alias: None,
        kind: Kind::Enum { target: |s| &mut s.palette, names: PALETTES },
        metavar: Some("NAME"),
        help: "colour scheme",
        group: "Output",
        essential: false,
    },
    OptionSpec {
        shorthand: None,
        name: "label",
        alias: None,
        kind: Kind::Str(|s| &mut s.label),
        metavar: Some("TEXT"),
        help: "a caption",
        group: "Output",
        essential: false,
    },
];

/// The C helper `parse(error, size, ...)`: argv[0] is "cbirds", and the C
/// buffer was `char error[128]`. Returns the status; `error` receives what the
/// C would have left in its buffer.
fn parse(settings: &mut Settings, error: &mut Vec<u8>, args: &[&str]) -> Status {
    let mut argv = vec![OsString::from("cbirds")];
    argv.extend(args.iter().map(OsString::from));
    let parsed = options::parse(TABLE, settings, &argv, 128);
    *error = parsed.message;
    parsed.status
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle.as_bytes())
}

fn find(haystack: &[u8], needle: &str) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle.as_bytes())
}

#[test]
fn test_forms() {
    let mut error = Vec::new();

    // The four ways of giving a value all mean the same thing.
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-n", "1500"]) == Status::Ok && s.birds == 1500);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-n1500"]) == Status::Ok && s.birds == 1500);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds", "1500"]) == Status::Ok && s.birds == 1500);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds=1500"]) == Status::Ok && s.birds == 1500);

    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--weight=0.25"]) == Status::Ok && s.weight == 0.25);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--label", "hello world"]) == Status::Ok);
    assert_eq!(s.label.as_deref(), Some("hello world".as_ref()));
}

#[test]
fn test_flags_cluster() {
    let mut error = Vec::new();

    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-qm"]) == Status::Ok && s.quiet && s.mono);
    // A cluster may end in a value taking option.
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-qn200"]) == Status::Ok && s.quiet && s.birds == 200);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-qn", "200"]) == Status::Ok && s.quiet && s.birds == 200);

    // --no-NAME turns a flag back off, whatever set it.
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--quiet", "--no-quiet"]) == Status::Ok && !s.quiet);
    let mut s = reset();
    s.quiet = true;
    assert!(parse(&mut s, &mut error, &["--no-quiet"]) == Status::Ok && !s.quiet);
}

#[test]
fn test_enumerations() {
    let mut error = Vec::new();

    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--palette", "ice"]) == Status::Ok && s.palette == 2);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-Pflame"]) == Status::Ok && s.palette == 1);

    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--palette", "chartreuse"]) == Status::Error);
    assert!(contains(&error, "mono") && contains(&error, "flame") && contains(&error, "ice"));
}

#[test]
fn test_refusals() {
    let mut error = Vec::new();

    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds", "0"]) == Status::Error);
    assert!(contains(&error, "between 1 and 4096"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds", "5000"]) == Status::Error);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds", "12.5"]) == Status::Error);
    assert!(contains(&error, "whole number"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds", "many"]) == Status::Error);
    assert!(contains(&error, "wants a number"));
    // strtod reads these as numbers; a NaN fails every range comparison and so
    // slipped past them into a cast to int.
    for odd in ["nan", "-nan", "NAN(1)", "inf", "-inf", "infinity"] {
        let mut s = reset();
        assert!(parse(&mut s, &mut error, &["--birds", odd]) == Status::Error);
        assert!(contains(&error, "wants a number") && s.birds == 800);
        let mut s = reset();
        assert!(parse(&mut s, &mut error, &["--weight", odd]) == Status::Error);
        assert!(contains(&error, "wants a number") && s.weight == 0.5);
    }
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birds"]) == Status::Error);
    assert!(contains(&error, "wants a value"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--quiet=1"]) == Status::Error);
    assert!(contains(&error, "takes no value"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--birdz", "10"]) == Status::Error);
    // A near miss is answered with the name they probably meant.
    assert!(contains(&error, "unknown option '--birdz'"));
    assert!(contains(&error, "did you mean '--birds'"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--quiett"]) == Status::Error);
    assert!(contains(&error, "did you mean '--quiet'"));
    let mut s = reset();
    // And something that is not a near miss gets no guess.
    assert!(parse(&mut s, &mut error, &["--xyzzy"]) == Status::Error);
    assert!(!contains(&error, "did you mean"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["-z"]) == Status::Error);
    assert!(contains(&error, "unknown option '-z'"));

    // This program takes no positional arguments, and says so.
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["flock.png"]) == Status::Error);
    assert!(contains(&error, "unexpected argument 'flock.png'"));
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--", "flock.png"]) == Status::Error);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--"]) == Status::Ok);
}

#[test]
fn test_help_and_version() {
    let mut error = Vec::new();
    let mut s = reset();
    // -h is the one screen version, --help everything: two answers, not one.
    assert!(parse(&mut s, &mut error, &["-h"]) == Status::Help);
    assert!(parse(&mut s, &mut error, &["--help"]) == Status::HelpFull);
    assert!(parse(&mut s, &mut error, &["-V"]) == Status::Version);
    assert!(parse(&mut s, &mut error, &["--version"]) == Status::Version);
    // Asked for anywhere, it wins over whatever else is on the line.
    assert!(parse(&mut s, &mut error, &["-n", "10", "--help"]) == Status::HelpFull);

    // And inside a cluster, which is what -help and -hV are: a typo for the
    // help flag should not be answered with "unknown option '-h'".
    assert!(parse(&mut s, &mut error, &["-help"]) == Status::Help);
    assert!(parse(&mut s, &mut error, &["-hV"]) == Status::Help);
    assert!(parse(&mut s, &mut error, &["-Vh"]) == Status::Version);

    // --completion leaves the shell in the buffer for the caller to act on.
    assert!(parse(&mut s, &mut error, &["--completion", "fish"]) == Status::Completion);
    assert_eq!(error, b"fish");
    assert!(parse(&mut s, &mut error, &["--completion"]) == Status::Error);
}

#[test]
fn test_usage_is_aligned() {
    static EXAMPLES: &[Example] = &[
        Example { command: "cbirds -n 1500", what: "a bigger flock" },
        Example { command: "cbirds", what: "the default" },
    ];
    let mut buffer = Vec::new();
    options::usage(
        &mut buffer,
        b"cbirds",
        Some("A flock in your terminal."),
        Some(EXAMPLES),
        TABLE,
        true,
    )
    .unwrap();
    assert!(buffer.len() < 4096, "the C captured this in a 4096 byte buffer");

    assert_eq!(find(&buffer, "A flock in your terminal."), Some(0));
    assert!(contains(&buffer, "Usage: cbirds [OPTIONS]"));
    // Groups appear once each, in table order.
    let flock = find(&buffer, "\nFlock\n");
    let output = find(&buffer, "\nOutput\n");
    let general = find(&buffer, "\nGeneral\n");
    let (Some(flock), Some(output), Some(general)) = (flock, output, general) else {
        panic!("a group heading is missing");
    };
    assert!(flock < output && output < general);
    assert!(contains(&buffer, "-n, --birds COUNT"));
    assert!(contains(&buffer, "    --label TEXT")); // No shorthand, still aligned.
    assert!(contains(&buffer, "-h, --help"));
    assert!(contains(&buffer, "-V, --version"));
    assert!(contains(&buffer, "Examples"));
    // The examples are columns too, aligned to the widest command rather than
    // padded by hand, which is how they came to be misaligned in the first
    // place.
    assert!(contains(&buffer, "  cbirds -n 1500  a bigger flock\n"));
    assert!(contains(&buffer, "  cbirds          the default\n"));
    assert!(contains(&buffer, "cbirds -n 1500"));

    // Every help text starts at the same column. As in the C, lines are what
    // strtok yields (empty ones skipped), and the scan starts at the first two
    // spaces of the line.
    let text = String::from_utf8(buffer.clone()).expect("the help is UTF-8");
    let mut column = 0;
    for line in text.split('\n').filter(|line| !line.is_empty()) {
        let bytes = line.as_bytes();
        if bytes[0] != b' ' || bytes.get(2) != Some(&b'-') {
            continue;
        }
        let mut help = line.find("  ").expect("two spaces in an option line");
        while bytes.get(help + 2) == Some(&b' ') {
            help += 1;
        }
        let here = help + 2;
        if column == 0 {
            column = here;
        }
        assert_eq!(here, column);
    }
    assert!(column > 0);
}

#[test]
fn test_aliases_and_the_short_help() {
    let mut error = Vec::new();

    // An old name keeps working without being advertised, which is how a
    // rename costs nobody anything.
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--boids", "1200"]) == Status::Ok && s.birds == 1200);
    let mut s = reset();
    assert!(parse(&mut s, &mut error, &["--boids=1200"]) == Status::Ok && s.birds == 1200);

    // -h shows only the essential rows, --help shows them all.
    let mut brief = Vec::new();
    let mut full = Vec::new();
    options::usage(&mut brief, b"cbirds", None, None, TABLE, false).unwrap();
    options::usage(&mut full, b"cbirds", None, None, TABLE, true).unwrap();
    assert!(brief.len() < 2048 && full.len() < 4096, "the C buffers were 2048 and 4096");
    assert!(contains(&brief, "--birds")); // Essential.
    assert!(!contains(&brief, "--weight")); // Not.
    assert!(contains(&full, "--weight"));
    assert!(brief.len() < full.len());
    // The old name is advertised in neither: accepted, never shown.
    assert!(!contains(&brief, "--boids") && !contains(&full, "--boids"));
}

/// The C test's `static int flag` behind QUOTED: a target nothing reads.
#[allow(dead_code)]
struct Shy {
    flag: bool,
}

#[test]
fn test_completions() {
    for shell in ["bash", "zsh", "fish"] {
        let mut buffer = Vec::new();
        assert!(options::completion(&mut buffer, shell.as_bytes(), "cbirds", TABLE).unwrap());
        assert!(buffer.len() < 4096, "the C captured this in a 4096 byte buffer");
        // Every long name reaches the shell, or the completion is a lie.
        for option in TABLE {
            assert!(contains(&buffer, option.name));
        }
        // And so does every short one, and the switches the table does not hold.
        let expected: &[&str] = match shell {
            "bash" => &[
                "-n ",
                "-w ",
                "-q ",
                "-m ",
                "-P ",
                "-h ",
                "--help ",
                "--completion ",
                "-V ",
                "--version ",
            ],
            "zsh" => &[
                "'-n[",
                "'-w[",
                "'-q[",
                "'-m[",
                "'-P[",
                "'-h[",
                "'--help[",
                "'--completion[",
                "'-V[",
                "'--version[",
            ],
            _ => &[
                "-s n",
                "-s w",
                "-s q",
                "-s m",
                "-s P",
                "-s h",
                "-l help",
                "-l completion",
                "-l version",
                "-s V",
            ],
        };
        for want in expected {
            assert!(contains(&buffer, want), "{shell}: {want}");
        }
    }
    let mut buffer = Vec::new();
    assert!(!options::completion(&mut buffer, b"tcsh", "cbirds", TABLE).unwrap());
    // In addition to the C: an unknown shell gets nothing written.
    assert!(buffer.is_empty());

    // Help is free text, and a quote in it once ended zsh's quoted spec half
    // way through: the whole file failed to load.
    static QUOTED: &[OptionSpec<Shy>] = &[OptionSpec {
        shorthand: None,
        name: "shy",
        alias: None,
        kind: Kind::Flag(|s| &mut s.flag),
        metavar: None,
        help: "keeps out of each other's [way] \"$HOME\"",
        group: "Flock",
        essential: true,
    }];
    let mut buffer = Vec::new();
    assert!(options::completion(&mut buffer, b"zsh", "cbirds", QUOTED).unwrap());
    assert!(contains(&buffer, "'--shy[keeps out of each other'\\''s \\[way\\] \"$HOME\"]'"));
    let mut buffer = Vec::new();
    assert!(options::completion(&mut buffer, b"fish", "cbirds", QUOTED).unwrap());
    assert!(contains(&buffer, "-d \"keeps out of each other's [way] \\\"\\$HOME\\\"\""));
}
