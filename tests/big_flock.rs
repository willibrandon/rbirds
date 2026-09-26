//! Big-flock mode (docs/DEVIATIONS.md D-006).
//!
//! Up to cbirds' 4096 birds, the parallel steps are held to the C reference
//! by `tests/sim_oracle.rs`, which flies and draws every scenario on several
//! threads as well as on one. Past 4096 there is no C to compare with, so
//! here the same script is run on one thread and on several, and every
//! double (as its bits) and every frame (as its bytes) must agree. The
//! command line, the pacing that keeps a big flock from getting ahead of the
//! terminal, and the frame delay's sleep are checked too.

mod support;

use std::fmt::Write as _;
use std::process::{Command, Output};

use rbirds::config::{BIG_FLOCK_BIRDS, MAX_BIRDS};
use rbirds::input::InputParser;
use rbirds::live::{KEYS_ROOM, Pacing, STATUS_ANSWER, elapsed_microseconds, sleep_until};
use rbirds::platform;
use rbirds::simulation::Sim;
use support::sim::World;

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// A script's transcript in big-flock mode on `threads` threads, and the
/// flock it ended with.
fn transcript(script: &str, threads: usize) -> (String, i32) {
    let mut world = World::new();
    world.sim.big_flock = true;
    world.sim.threads = threads;
    for line in script.lines() {
        world.command(line);
    }
    (world.take(), world.sim.config.birds)
}

fn same_on_every_thread_count(name: &str, script: &str, threads: &[usize]) -> i32 {
    let (expected, birds) = transcript(script, 1);
    assert!(expected.contains("digest "), "{name}: the script took no digests");
    for &count in threads {
        let (actual, _) = transcript(script, count);
        if actual != expected {
            let line = expected
                .lines()
                .zip(actual.lines())
                .position(|(e, a)| e != a)
                .map_or_else(|| "a different length".to_owned(), |at| format!("line {}", at + 1));
            panic!("{name} on {count} threads differs from one thread at {line}");
        }
    }
    birds
}

/// Every look at once, past the cap: three flocks keeping apart, four
/// hawks, trails and the far sky, recorded, then drawn in every renderer.
#[test]
fn a_flock_past_the_cap_is_the_same_on_any_number_of_threads() {
    let birds = 6000;
    let fps = 25.0;
    let mut s = String::new();
    let _ = writeln!(s, "set birds {birds}\nset flocks 3\nset hawks 4\nset trails 1\nset deep 1");
    let _ = writeln!(s, "set avoid 8\nset palette 1\nset size 30\nset hawk_sets 1\nnotches");
    let _ = writeln!(s, "seconds {}\nset legend 1\nscreen 200 50 1600 800\ngrid", bits(1.0 / fps));
    let _ = writeln!(s, "seed 7\nalloc {birds}\ninit\nhawks\nintro\ndigest");
    for frame in 0..40 {
        let _ = writeln!(s, "record {frame} {}", bits(fps));
        if frame % 8 == 7 {
            s.push_str("digest\n");
        }
    }
    // Braille, sextants, blocks, Kitty, Sixel.
    for render in [1, 2, 3, 0, 4] {
        let _ =
            writeln!(s, "set render {render}\nsprites\nrender\nrecord 40 {}\nrender", bits(fps));
    }
    s.push_str("digest\n");
    same_on_every_thread_count("six thousand birds", &s, &[2, 5, 12]);
}

/// `+` in a live session grows the flock past cbirds' cap, a quarter at a
/// time, and each grown flock flies the same on any number of threads.
#[test]
fn keys_grow_a_big_flock_past_the_cap_the_same_on_any_number_of_threads() {
    let birds = 4000;
    let mut s = String::new();
    let _ = writeln!(s, "set birds {birds}\nset palette 1\nset size 30\nset render 1\nnotches");
    let _ = writeln!(s, "seconds {}\nscreen 200 50 1600 800\ngrid", bits(1.0 / 60.0));
    let _ = writeln!(s, "seed 3\nalloc {birds}\ninit\nhawks\nsprites\nlive_begin 10 0");
    for frame in 1..=8_i64 {
        let keys = if frame <= 5 { "2b" } else { "-" };
        let nanoseconds = frame * 16_666_667;
        let _ = writeln!(s, "live {keys} 10 {nanoseconds} 200 50 1600 800\ndigest");
    }
    let grown = same_on_every_thread_count("a growing flock", &s, &[2, 5, 12]);
    assert!(grown > MAX_BIRDS, "the flock stopped at {grown}");
}

/// The most birds there can be, for a few frames: the widest the work is
/// ever split.
#[test]
fn the_largest_flock_is_the_same_on_any_number_of_threads() {
    let birds = BIG_FLOCK_BIRDS;
    let mut s = String::new();
    let _ = writeln!(s, "set birds {birds}\nset palette 1\nset size 30\nset render 1\nnotches");
    let _ = writeln!(s, "seconds {}\nscreen 200 50 1600 800\ngrid", bits(1.0 / 60.0));
    let _ = writeln!(s, "seed 11\nalloc {birds}\ninit\nsprites");
    for frame in 0..3 {
        let _ = writeln!(s, "record {frame} {}", bits(60.0));
    }
    s.push_str("render\ndigest\n");
    same_on_every_thread_count("the largest flock", &s, &[3, 12]);
}

#[test]
fn plus_stops_at_the_big_flock_cap_and_cbirds_cap_without_it() {
    for (big_flock, from, limit) in
        [(false, MAX_BIRDS - 10, MAX_BIRDS), (true, BIG_FLOCK_BIRDS - 10, BIG_FLOCK_BIRDS)]
    {
        let mut sim = Sim::new();
        sim.big_flock = big_flock;
        sim.config.birds = from;
        let mut parser = InputParser::default();
        sim.handle_input(&mut parser, Some(b"+"));
        assert_eq!(sim.config.birds, limit);
        sim.population_changed = false;
        sim.handle_input(&mut parser, Some(b"+"));
        assert_eq!(sim.config.birds, limit);
        assert!(!sim.population_changed, "nothing to grow");
    }
    let mut sim = Sim::new();
    sim.big_flock = true;
    sim.config.birds = MAX_BIRDS;
    sim.handle_input(&mut InputParser::default(), Some(b"+"));
    assert_eq!(sim.config.birds, MAX_BIRDS + MAX_BIRDS / 4 + 1);
}

/// Answers come out of the input, whole or split across reads, and
/// everything else stays in order, arrow keys included, with nothing lost.
#[test]
fn the_terminal_answers_are_taken_out_of_the_keys() {
    let mut pacing = Pacing::new(true);
    let mut keys = [0_u8; KEYS_ROOM];
    // An answer on its own.
    pacing.requested();
    assert_eq!(pacing.keys_from(STATUS_ANSWER, &mut keys), 0);
    assert!(!pacing.waiting());
    // Split over three reads, with keys on either side.
    pacing.requested();
    assert_eq!(pacing.keys_from(b"a\x1b", &mut keys), 1);
    assert_eq!(&keys[..1], b"a");
    assert_eq!(pacing.keys_from(b"[0", &mut keys), 0);
    assert!(pacing.waiting());
    let n = pacing.keys_from(b"nq", &mut keys);
    assert_eq!(&keys[..n], b"q");
    assert!(!pacing.waiting());
    // Something that starts like an answer and isn't one is keys: an arrow,
    // Escape then q, a mouse report.
    for input in [&b"\x1b[A"[..], b"\x1bq", b"\x1b[<0;3;4M", b"\x1b[00n"] {
        let n = pacing.keys_from(input, &mut keys);
        assert_eq!(&keys[..n], input, "{input:?}");
    }
    // Keys and answers mixed.
    pacing.requested();
    pacing.requested();
    let n = pacing.keys_from(b"+\x1b[0n+\x1b[0n", &mut keys);
    assert_eq!(&keys[..n], b"++");
    assert!(!pacing.waiting());
    // The start of an answer held from one read, then a whole read of keys:
    // every byte comes back.
    assert_eq!(pacing.keys_from(b"\x1b[", &mut keys), 0);
    let read = [b'x'; 100];
    let n = pacing.keys_from(&read, &mut keys);
    assert_eq!(n, 102);
    assert_eq!(&keys[..2], b"\x1b[");
    assert!(keys[2..n].iter().all(|&k| k == b'x'));
}

/// A request is counted before its frame is written, so an answer read
/// while the frame is still being written (as on Windows, whose writes go
/// through a thread) answers it rather than being lost.
#[test]
fn an_answer_read_while_its_frame_is_written_counts() {
    let mut pacing = Pacing::new(true);
    let mut keys = [0_u8; KEYS_ROOM];
    pacing.requested();
    // The write blocks, the keys are read, and the answer is there.
    assert_eq!(pacing.keys_from(STATUS_ANSWER, &mut keys), 0);
    pacing.sent(platform::monotonic_now());
    assert!(!pacing.waiting(), "nothing is due");
}

/// The frame delay's sleep is never early, and lands close to its time even
/// where one long sleep would be milliseconds late (macOS).
#[test]
fn the_frame_delay_ends_on_time() {
    let mut late = Vec::new();
    for _ in 0..10 {
        let from = platform::monotonic_now();
        sleep_until(&from, 14_000);
        let slept = elapsed_microseconds(&from, &platform::monotonic_now());
        assert!(slept >= 14_000, "woke after {slept} us");
        late.push(slept - 14_000);
    }
    late.sort_unstable();
    assert!(late[5] < 3_000, "the median sleep was {} us late", late[5]);
}

fn rbirds(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rbirds")).args(args).output().expect("run rbirds")
}

fn report(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).expect("the report is text")
}

#[test]
fn big_flock_takes_the_count_and_half_the_cores() {
    let text = report(&rbirds(&["--bench", "2", "--big-flock", "5000", "--birds", "10"]));
    assert!(text.contains("birds        5000\n"), "{text}");
    let threads = rbirds::parallel::big_flock_threads();
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    assert_eq!(threads, (cores / 2).max(1));
    assert!(text.contains(&format!("threads      {threads}\n")), "{text}");
    // Without it, the report is cbirds'.
    assert!(!report(&rbirds(&["--bench", "1"])).contains("threads"));
}

#[test]
fn counts_out_of_range_are_refused_and_birds_keeps_its_cap() {
    for (args, message) in [
        (&["--big-flock", "0"][..], "--big-flock must be between 1 and 65536\n"),
        (&["--big-flock", "65537"][..], "--big-flock must be between 1 and 65536\n"),
        (&["--big-flock", "2.5"][..], "--big-flock wants a whole number, not '2.5'\n"),
        (&["--birds", "4097", "--big-flock", "5000"][..], "--birds must be between 1 and 4096\n"),
    ] {
        let output = rbirds(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

/// The lines of a benchmark report that do not depend on the machine.
fn measured(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| {
            !["frame time", "ceiling", "threads", "at 60 fps"].iter().any(|p| line.starts_with(p))
        })
        .collect()
}

#[test]
fn a_flock_cbirds_could_fly_is_the_one_birds_flies() {
    for render in ["kitty", "braille", "sixel"] {
        let common = ["--bench", "20", "--seed", "9", "--hawks", "2", "--render", render];
        let plain = report(&rbirds(&[&common[..], &["--birds", "900"]].concat()));
        let big = report(&rbirds(&[&common[..], &["--big-flock", "900"]].concat()));
        assert_eq!(measured(&plain), measured(&big), "{render}");
    }
    // Recordings: the sprites as a GIF, the cells painted as a GIF, and the
    // cells as a cast, whose header line holds the time it was made.
    let dir = std::env::temp_dir().join(format!("rbirds-big-flock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (suffix, render) in [("gif", "kitty"), ("gif", "braille"), ("cast", "braille")] {
        let mut files = Vec::new();
        for count in [["--birds", "700"], ["--big-flock", "700"]] {
            let path = dir.join(format!("{}-{render}.{suffix}", &count[0][2..]));
            let path_text = path.to_str().expect("a UTF-8 temporary directory");
            let recorded = rbirds(
                &[
                    &["--record", path_text, "--seed", "4", "--trails", "--render", render][..],
                    &["--record-seconds", "1", "--record-size", "60x20"][..],
                    &count[..],
                ]
                .concat(),
            );
            assert!(recorded.status.success(), "{}", String::from_utf8_lossy(&recorded.stderr));
            let file = std::fs::read(&path).unwrap();
            let body = match suffix {
                "cast" => file[file.iter().position(|&b| b == b'\n').unwrap()..].to_vec(),
                _ => file,
            };
            files.push(body);
        }
        assert_eq!(files[0], files[1], "the same {render} {suffix}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_big_flock_keeps_the_size_it_is_given() {
    let mut sim = Sim::new();
    sim.big_flock = true;
    sim.config.birds = 30000;
    sim.settle_the_bird_size();
    assert_eq!(sim.config.bird_size, 30, "cbirds' default");
}
