//! Characterization of the C reference's terminal lifecycle under a scripted
//! PTY (docs/PORTING.md §4.C), with the lifecycle cases of
//! `support::pty::cases` run on `cbirds` built from the pinned sources with
//! the canonical flags (`cc -std=c99 -Wall -Wextra -O3 -g ... -lm`). The same
//! case functions take any [`Subject`], so the rbirds binary can later be run
//! through them unchanged.
//!
//! Also here: the Rust emergency path (`rbirds::platform`'s signal handler
//! and `restore_terminal`) exercised for real in a subprocess (this test
//! binary re-executed in a child mode) for every handled signal, with the
//! same assertions the C results are held to.
//!
//! The cases run one at a time (a process-wide lock): each drives a live,
//! CPU-hungry animation, and running many at once could starve a program's
//! 60 ms theme-query window. Run with `--nocapture` to see each record.

mod support;

use std::ffi::c_int;
use std::sync::Mutex;
use std::time::Duration;

use rbirds::platform;
use support::oracle;
use support::pty::{self, Action, Spec, Step, Subject, cases};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// cbirds from the pinned reference, as its makefile builds it; `None` only
/// when `RBIRDS_NO_ORACLE=1` allows skipping.
fn cbirds() -> Option<Subject> {
    // oracle::build compiles tools/oracle/<program>; an absolute path names
    // the reference's own main file instead, so nothing but the unmodified
    // sources is compiled.
    let boids = oracle::reference_dir().join("boids.c");
    let exe = oracle::build(
        "cbirds",
        boids.to_str().expect("a UTF-8 reference path"),
        &["cells.c", "font.c", "gif.c", "kitty_graphics.c", "options.c", "png.c", "spatial_grid.c"],
    )?;
    Some(Subject { exe, name: "cbirds".into() })
}

fn record(case: &str, outcome: &pty::Outcome) {
    eprintln!("== {case}\n{}", outcome.describe());
}

macro_rules! reference_case {
    ($name:ident, $case:expr) => {
        #[test]
        fn $name() {
            let Some(subject) = cbirds() else { return };
            let _serial = serial();
            let run: fn(&Subject) -> pty::Outcome = $case;
            let outcome = run(&subject);
            record(stringify!($name), &outcome);
        }
    };
}

reference_case!(c_frames_run_queries_theme_and_restores, cases::frames_run);
reference_case!(c_fixed_palette_asks_nothing, cases::fixed_palette);
reference_case!(c_theme_replies_are_used, cases::theme_run);
reference_case!(c_fragmented_theme_reply_is_accumulated, cases::fragmented_theme_run);
reference_case!(c_quit_key_flies_out_or_leaves_at_once, cases::quit_key);
reference_case!(c_quit_while_output_blocked_leaves_at_once, cases::quit_key_while_output_blocked);
reference_case!(c_sigterm_exits_143_restored, |s| cases::signal_while_running(
    s,
    platform::SIGTERM
));
reference_case!(c_sigint_exits_130_restored, |s| cases::signal_while_running(s, platform::SIGINT));
reference_case!(c_sighup_exits_129_restored, |s| cases::signal_while_running(s, platform::SIGHUP));
reference_case!(c_sigquit_exits_131_restored, |s| cases::signal_while_running(
    s,
    platform::SIGQUIT
));
reference_case!(c_sigsegv_exits_139_restored, |s| cases::signal_while_running(
    s,
    platform::SIGSEGV
));
reference_case!(c_sigfpe_exits_136_restored, |s| cases::signal_while_running(s, platform::SIGFPE));
reference_case!(c_sigbus_exits_128_plus_sigbus_restored, |s| cases::signal_while_running(
    s,
    platform::SIGBUS
));
reference_case!(c_sigabrt_exits_134_restored, |s| cases::signal_while_running(
    s,
    platform::SIGABRT
));
reference_case!(c_sigterm_with_output_blocked_restores_after_drain, |s| {
    cases::signal_while_output_blocked(s, platform::SIGTERM)
});
reference_case!(c_sigpipe_is_ignored, cases::sigpipe_ignored);
reference_case!(c_broken_pipe_stdout_reports_epipe, cases::broken_pipe_stdout);
reference_case!(c_kitty_uploads_and_frees_images, cases::kitty_run);
reference_case!(c_stdin_dev_null_fails_raw_mode, |s| cases::stdin_not_a_terminal(
    s,
    pty::Stream::Null
));
reference_case!(c_stdin_pipe_fails_raw_mode, |s| cases::stdin_not_a_terminal(
    s,
    pty::Stream::ClosedPipe
));
reference_case!(c_tiny_window_and_resizes, cases::tiny_window_and_resizes);

#[test]
fn c_snapshot_is_written_after_restoring() {
    let Some(subject) = cbirds() else { return };
    let _serial = serial();
    let scratch = oracle::Scratch::new("pty-snapshot");
    let outcome = cases::snapshot_run(&subject, &scratch.file("snap.png"));
    record("c_snapshot_is_written_after_restoring", &outcome);
}

#[test]
fn c_snapshot_failure_exits_1_after_restoring() {
    let Some(subject) = cbirds() else { return };
    let _serial = serial();
    let scratch = oracle::Scratch::new("pty-snapshot-failure");
    let outcome = cases::snapshot_failure(&subject, &scratch.file("missing/snap.png"));
    record("c_snapshot_failure_exits_1_after_restoring", &outcome);
}

/// Recorded rather than asserted: what the reference does when its terminal
/// disappears (the outcome depends on the OS's hangup semantics).
#[test]
fn c_terminal_closed_is_recorded() {
    let Some(subject) = cbirds() else { return };
    let _serial = serial();
    let outcome = cases::terminal_closed(&subject);
    record("c_terminal_closed_is_recorded", &outcome);
}

// --- The Rust emergency path, in a subprocess -----------------------------

const CHILD_MODE: &str = "RBIRDS_PTY_CHILD";
const CHILD_READY: &[u8] = b"child ready\n";

/// Not a test of its own: the body of the subprocess. In the parent test run
/// it returns at once. In the child (this binary run with `CHILD_MODE` set,
/// on a PTY) it takes the terminal the way cbirds does (raw mode, the
/// handlers, the alternate screen, optionally the sprite flag), says it is
/// ready, and waits for a signal (for at most 20 s): idle, or in mode
/// `congested` writing to the terminal without end, so that a terminal that
/// stops reading leaves it blocked in write(2). (Blocking writes, unlike
/// cbirds' nonblocking ones, leave no window in which the handler could find
/// O_NONBLOCK set, so the case is deterministic.)
#[test]
fn emergency_path_child() {
    let Some(mode) = std::env::var_os(CHILD_MODE) else { return };
    if platform::enter_terminal().is_err() {
        platform::exit_immediately(90);
    }
    platform::install_signal_handlers();
    platform::enter_alt_screen();
    if mode == "sprites" {
        platform::mark_sprites_uploaded();
    }
    platform::write_all_quietly(platform::STDOUT_FILENO, CHILD_READY);
    let started = platform::monotonic_now();
    while platform::monotonic_now().tv_sec - started.tv_sec < 20 {
        if mode == "congested" {
            let _ = platform::write(platform::STDOUT_FILENO, &[b'x'; 4096]);
        } else {
            let _ = platform::nanosleep(&platform::Timespec { tv_sec: 0, tv_nsec: 10_000_000 });
        }
    }
    platform::exit_immediately(91);
}

fn rust_child(signal: c_int, mode: &str) -> pty::Outcome {
    let subject = Subject {
        exe: std::env::current_exe().expect("this test binary"),
        name: "pty_reference".into(),
    };
    let spec = Spec::new(&subject, &["emergency_path_child", "--exact", "--test-threads=1"])
        .env(CHILD_MODE, mode)
        .step(Step::after_output(CHILD_READY, Action::SnapshotTermios))
        .step(Step::after(Duration::from_millis(50), Action::Signal(signal)));
    let spec = Spec { initial_termios: Some(cases::cooked_with_everything), ..spec };
    let outcome = pty::run(&spec);
    let raw = outcome.termios_before.raw_mode();
    assert_eq!(outcome.termios_during, vec![raw], "raw mode as enter_terminal sets it");
    let taken = cases::screen_taken();
    let alt_screen = &taken[..taken.len() - 3]; // without the erase
    assert!(outcome.contains(alt_screen), "{}", outcome.describe());
    cases::assert_signal_exit(&outcome, signal, mode == "sprites");
    outcome
}

macro_rules! rust_signal_case {
    ($name:ident, $signal:expr, $mode:expr) => {
        #[test]
        fn $name() {
            let _serial = serial();
            let outcome = rust_child($signal, $mode);
            record(stringify!($name), &outcome);
        }
    };
}

rust_signal_case!(rust_sigterm_restores_and_exits_143, platform::SIGTERM, "text");
rust_signal_case!(rust_sigint_restores_and_exits_130, platform::SIGINT, "text");
rust_signal_case!(rust_sighup_restores_and_exits_129, platform::SIGHUP, "text");
rust_signal_case!(rust_sigquit_restores_and_exits_131, platform::SIGQUIT, "text");
rust_signal_case!(rust_sigsegv_restores_and_exits_139, platform::SIGSEGV, "text");
rust_signal_case!(rust_sigfpe_restores_and_exits_136, platform::SIGFPE, "text");
rust_signal_case!(rust_sigbus_restores_and_exits_128_plus_sigbus, platform::SIGBUS, "text");
rust_signal_case!(rust_sigabrt_restores_and_exits_134, platform::SIGABRT, "text");
rust_signal_case!(rust_sigterm_with_sprites_frees_images, platform::SIGTERM, "sprites");

/// SIGPIPE is ignored by `install_signal_handlers`: the child survives it and
/// the test ends it with SIGTERM, still through the handler.
#[test]
fn rust_sigpipe_is_ignored() {
    let _serial = serial();
    let subject = Subject {
        exe: std::env::current_exe().expect("this test binary"),
        name: "pty_reference".into(),
    };
    let spec = Spec::new(&subject, &["emergency_path_child", "--exact", "--test-threads=1"])
        .env(CHILD_MODE, "text")
        .step(Step::after_output(CHILD_READY, Action::Signal(platform::SIGPIPE)))
        .step(Step::after(Duration::from_millis(200), Action::Signal(platform::SIGTERM)));
    let outcome = pty::run(&spec);
    record("rust_sigpipe_is_ignored", &outcome);
    cases::assert_signal_exit(&outcome, platform::SIGTERM, false);
}

/// The handler while the program is blocked writing to a terminal that has
/// stopped reading: still 128 + signal, the attributes back, and the restore
/// sequence written in full once the terminal reads again. On Darwin, as for
/// the reference, the exit waits for that (see
/// `cases::signal_while_output_blocked`).
///
/// This child is a libtest binary, which runs the test body on a thread of
/// its own, so the signal is handled on another thread while the blocked
/// writer carries on until `_exit`: its `x`s may follow or interleave with the
/// restore sequence. (rbirds, like cbirds, is single-threaded.) So the
/// assertion is that, apart from those `x`s, everything after the ready line
/// is exactly the restore sequence.
#[test]
fn rust_sigterm_with_output_blocked_restores() {
    let _serial = serial();
    let subject = Subject {
        exe: std::env::current_exe().expect("this test binary"),
        name: "pty_reference".into(),
    };
    let spec = Spec::new(&subject, &["emergency_path_child", "--exact", "--test-threads=1"])
        .env(CHILD_MODE, "congested")
        .step(Step::after_output(CHILD_READY, Action::PauseReading(Duration::from_millis(1500))))
        .step(Step::after(Duration::from_millis(500), Action::Signal(platform::SIGTERM)));
    let outcome = pty::run(&spec);
    record("rust_sigterm_with_output_blocked_restores", &outcome);
    outcome.assert_exit_code(128 + platform::SIGTERM);
    outcome.assert_attributes_restored();
    let ready = outcome.position(CHILD_READY).expect("ready line") + CHILD_READY.len();
    let not_x: Vec<u8> =
        outcome.transcript[ready..].iter().copied().filter(|&b| b != b'x').collect();
    assert_eq!(
        pty::escape(&not_x),
        pty::escape(&cases::screen_restored(false)),
        "{}",
        outcome.describe()
    );
    let signalled = outcome.event("sent signal").expect("signalled").at;
    let resumed = outcome.event("reading resumed").expect("resumed").at;
    let exited = outcome.event("exited").expect("exited").at;
    assert!(signalled < resumed, "signalled while blocked\n{}", outcome.describe());
    if cfg!(target_os = "macos") {
        assert!(resumed <= exited, "exit waited for the drain\n{}", outcome.describe());
    }
}
