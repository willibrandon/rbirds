#![cfg(unix)]

//! The rbirds binary through the same scripted-PTY lifecycle cases the C
//! reference is characterized with in tests/pty_reference.rs
//! (docs/PORTING.md §4.C, docs/COMPATIBILITY.md C13–C15): theme queries and
//! replies, raw mode and its exact undoing, every handled signal, blocked and
//! broken output, Kitty uploads and their release, non-terminal input, tiny
//! windows and resizes, snapshots. Each case asserts the reference's
//! observed behavior, byte sequences included, so passing here and there is
//! the comparison. Where a case only records (a vanished terminal), the two
//! programs' outcomes are compared directly.

mod support;

use std::sync::Mutex;
use std::time::Duration;

use rbirds::platform;
use support::oracle;
use support::pty::{self, Subject, cases};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn rbirds() -> Subject {
    Subject { exe: oracle::rust_binary(), name: "rbirds".into() }
}

#[test]
fn live_trace_covers_every_flushed_frame_and_reports_cpu_time() {
    let _serial = serial();
    let path = std::env::temp_dir().join(format!("rbirds-trace-{}.jsonl", std::process::id()));
    let outcome = pty::run(
        &pty::Spec::new(&rbirds(), &["--frames", "8", "--color", "ember", "--birds", "10"])
            .env("RBIRDS_TRACE", &path),
    );
    assert_eq!(outcome.exit, Some(pty::Exit::Code(0)), "{}", outcome.describe());
    outcome.assert_attributes_restored();
    let report = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let lines: Vec<_> = report.lines().collect();
    assert_eq!(lines.len(), 9);
    assert!(lines[0].contains("\"samples\":8,\"omitted\":0"));
    assert!(lines[0].contains("\"cpu_us\":"));
    assert!(!lines[0].contains("\"cpu_us\":0,"));
    for line in &lines[1..] {
        assert!(line.starts_with("{\"kind\":\"frame\","));
        assert!(line.contains("\"wake_late_us\":"));
        assert!(line.contains("\"flush_us\":"));
    }
}

#[test]
fn paused_playback_sends_nothing_steps_once_and_sleeps_even_when_unlocked() {
    use support::pty::{Action, Spec, Step};
    let _serial = serial();
    let path =
        std::env::temp_dir().join(format!("rbirds-paused-trace-{}.jsonl", std::process::id()));
    for unlocked in [false, true] {
        let mut args = vec!["--color", "ember", "--birds", "100"];
        if unlocked {
            args.push("--unlock-fps");
        }
        let frame_end = b"\x1b[?2026l";
        let outcome = pty::run(
            &Spec::new(&rbirds(), &args)
                .env("RBIRDS_TRACE", &path)
                .step(Step::after_output(frame_end, Action::Input(b" ".to_vec())))
                .step(Step::after(Duration::from_millis(200), Action::Mark("idle begin")))
                .step(Step::after(Duration::from_millis(200), Action::Mark("idle end")))
                .step(Step::after(Duration::ZERO, Action::Input(b".".to_vec())))
                .step(Step::after_output(frame_end, Action::Mark("stepped")))
                .step(Step::after(Duration::from_millis(200), Action::Mark("still")))
                .step(Step::after(Duration::ZERO, Action::Input(b"q".to_vec()))),
        );
        assert_eq!(outcome.exit, Some(pty::Exit::Code(0)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        let offset = |name: &str| outcome.events.iter().find(|e| e.what == name).unwrap().offset;
        assert_eq!(offset("mark idle begin"), offset("mark idle end"));
        let stepped = &outcome.transcript[offset("mark idle end")..offset("mark stepped")];
        assert_eq!(stepped.windows(frame_end.len()).filter(|bytes| *bytes == frame_end).count(), 1);
        assert_eq!(offset("mark stepped"), offset("mark still"));
        let report = std::fs::read_to_string(&path).unwrap();
        let idle_starts: Vec<u64> = report
            .lines()
            .filter(|line| line.contains("\"drawn\":false"))
            .map(|line| {
                line.split("\"start_us\":")
                    .nth(1)
                    .unwrap()
                    .split(',')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert!(idle_starts.len() >= 5, "no paused ticks: {report}");
        let span = idle_starts.last().unwrap() - idle_starts.first().unwrap();
        assert!(
            idle_starts.len() as u64 <= span / 10_000 + 5,
            "paused loop spun while unlocked={unlocked}"
        );
    }
    std::fs::remove_file(path).unwrap();
}

macro_rules! rust_case {
    ($name:ident, $case:expr) => {
        #[test]
        fn $name() {
            let _serial = serial();
            let run: fn(&Subject) -> pty::Outcome = $case;
            let outcome = run(&rbirds());
            eprintln!("== {}\n{}", stringify!($name), outcome.describe());
        }
    };
}

rust_case!(frames_run_queries_theme_and_restores, cases::frames_run);
rust_case!(fixed_palette_asks_nothing, cases::fixed_palette);
rust_case!(theme_replies_are_used, cases::theme_run);
rust_case!(fragmented_theme_reply_is_accumulated, cases::fragmented_theme_run);
rust_case!(quit_key_flies_out_or_leaves_at_once, cases::quit_key);
rust_case!(quit_while_output_blocked_leaves_at_once, cases::quit_key_while_output_blocked);
rust_case!(sigterm_exits_143_restored, |s| cases::signal_while_running(s, platform::SIGTERM));
rust_case!(sigint_exits_130_restored, |s| cases::signal_while_running(s, platform::SIGINT));
rust_case!(sighup_exits_129_restored, |s| cases::signal_while_running(s, platform::SIGHUP));
rust_case!(sigquit_exits_131_restored, |s| cases::signal_while_running(s, platform::SIGQUIT));
rust_case!(sigsegv_exits_139_restored, |s| cases::signal_while_running(s, platform::SIGSEGV));
rust_case!(sigfpe_exits_136_restored, |s| cases::signal_while_running(s, platform::SIGFPE));
rust_case!(sigbus_exits_128_plus_sigbus_restored, |s| cases::signal_while_running(
    s,
    platform::SIGBUS
));
rust_case!(sigabrt_exits_134_restored, |s| cases::signal_while_running(s, platform::SIGABRT));
rust_case!(sigterm_with_output_blocked_restores_after_drain, |s| {
    cases::signal_while_output_blocked(s, platform::SIGTERM)
});
rust_case!(sigpipe_is_ignored, cases::sigpipe_ignored);
rust_case!(broken_pipe_stdout_reports_epipe, cases::broken_pipe_stdout);
rust_case!(kitty_uploads_and_frees_images, cases::kitty_run);
rust_case!(stdin_dev_null_fails_raw_mode, |s| cases::stdin_not_a_terminal(s, pty::Stream::Null));
rust_case!(stdin_pipe_fails_raw_mode, |s| cases::stdin_not_a_terminal(s, pty::Stream::ClosedPipe));
rust_case!(tiny_window_and_resizes, cases::tiny_window_and_resizes);

#[test]
fn snapshot_is_written_after_restoring() {
    let _serial = serial();
    let scratch = oracle::Scratch::new("rust-pty-snapshot");
    let outcome = cases::snapshot_run(&rbirds(), &scratch.file("snap.png"));
    eprintln!("{}", outcome.describe());
}

#[test]
fn snapshot_failure_exits_1_after_restoring() {
    let _serial = serial();
    let scratch = oracle::Scratch::new("rust-pty-snapshot-failure");
    let outcome = cases::snapshot_failure(&rbirds(), &scratch.file("missing/snap.png"));
    eprintln!("{}", outcome.describe());
}

/// Whatever the OS does when the terminal disappears, rbirds ends the way
/// cbirds ends, with the terminal's attributes left as they were.
#[test]
fn a_vanished_terminal_ends_the_run_as_the_reference_does() {
    let boids = oracle::reference_dir().join("boids.c");
    let Some(exe) = oracle::build(
        "cbirds",
        boids.to_str().expect("a UTF-8 reference path"),
        &["cells.c", "font.c", "gif.c", "kitty_graphics.c", "options.c", "png.c", "spatial_grid.c"],
    ) else {
        return;
    };
    let _serial = serial();
    let c = cases::terminal_closed(&Subject { exe, name: "cbirds".into() });
    let r = cases::terminal_closed(&rbirds());
    assert_eq!(c.exit, r.exit, "C:\n{}\nRust:\n{}", c.describe(), r.describe());
    assert_eq!(c.timed_out, r.timed_out);
}

const PANIC_CHILD: &str = "RBIRDS_PANIC_CHILD";
const PANIC_READY: &[u8] = b"panic child ready\n";

/// Not a test of its own: the child for `a_panic_after_raw_mode_restores`.
/// It takes the terminal through the application's own guard, as the live
/// run does, and panics while holding it.
#[test]
fn panic_child() {
    if std::env::var_os(PANIC_CHILD).is_none() {
        return;
    }
    let mut terminal = rbirds::terminal::Terminal::enter().expect("raw mode");
    terminal.enter_alt_screen();
    platform::write_all_quietly(platform::STDOUT_FILENO, PANIC_READY);
    // Hold raw mode until the harness has read the attributes and types a
    // key. Raw mode reads return at once, so wait for input first.
    let mut fds = [platform::PollFd::new(platform::STDIN_FILENO, platform::POLLIN)];
    platform::poll(&mut fds, -1).expect("wait for the key");
    platform::read(platform::STDIN_FILENO, &mut [0u8; 1]).expect("read the key");
    panic!("a controlled panic with the terminal taken");
}

/// docs/DESIGN.md §6: panics unwind, and unwinding through the terminal
/// guard restores the attributes and writes the restore sequence, after the
/// panic message, before the process exits.
#[test]
fn a_panic_after_raw_mode_restores() {
    use support::pty::{Action, Spec, Step};
    let _serial = serial();
    let subject = Subject {
        exe: std::env::current_exe().expect("this test binary"),
        name: "pty_rbirds".into(),
    };
    // --nocapture: the panic hook writes to the terminal at once, as in the
    // real program, instead of into libtest's capture.
    let spec = Spec::new(&subject, &["panic_child", "--exact", "--test-threads=1", "--nocapture"])
        .env(PANIC_CHILD, "1")
        .step(Step::after_output(PANIC_READY, Action::SnapshotTermios))
        .step(Step::after_bytes(0, Action::Input(b"x".to_vec())));
    let spec = Spec { initial_termios: Some(cases::cooked_with_everything), ..spec };
    let outcome = pty::run(&spec);
    eprintln!("{}", outcome.describe());
    // libtest reports the failed child test with exit status 101.
    assert_eq!(outcome.exit, Some(pty::Exit::Code(101)), "{}", outcome.describe());
    outcome.assert_attributes_restored();
    assert_eq!(outcome.termios_during, vec![outcome.termios_before.raw_mode()]);
    let message = outcome.position(b"a controlled panic").expect("the panic message");
    let restored = outcome.position(&cases::screen_restored(false)).expect("the restore sequence");
    assert!(message < restored, "restored after the message\n{}", outcome.describe());
    assert_eq!(outcome.count(platform::ALT_SCREEN_OFF), 1);
}
