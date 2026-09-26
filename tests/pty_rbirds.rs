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

/// A live run of `args` with `frames` frames, each marked as it arrives, on
/// a terminal that answers device status requests or doesn't.
fn marked_run(args: &[&str], frames: usize, answers: bool) -> pty::Outcome {
    use support::pty::{Action, Reply, Spec, Step};
    let mut spec = Spec::new(&rbirds(), args);
    for _ in 0..frames {
        spec = spec.step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")));
    }
    if answers {
        spec = spec.reply(Reply::whole(b"\x1b[5n", b"\x1b[0n"));
    }
    let outcome = pty::run(&spec);
    cases::assert_clean_exit(&outcome, false);
    cases::assert_frames(&outcome, frames);
    outcome
}

/// No frame after the first second takes much longer than frames usually
/// do: the flock never stops in flight. Relative to the run's own median, so
/// a slow machine passes and a stall of a few hundred milliseconds does not.
fn assert_no_stall(outcome: &pty::Outcome) {
    let at: Vec<_> = outcome
        .events
        .iter()
        .filter(|e| e.what == "mark frame" && e.at.as_secs_f64() > 1.0)
        .map(|e| e.at)
        .collect();
    let mut gaps: Vec<_> = at.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.len() >= 20, "too few frames timed\n{}", outcome.describe());
    gaps.sort();
    let median = gaps[gaps.len() / 2];
    let longest = *gaps.last().unwrap();
    let allowed = median * 4 + std::time::Duration::from_millis(50);
    assert!(longest <= allowed, "a {longest:?} frame against a {median:?} median");
}

/// Big-flock mode (docs/DEVIATIONS.md D-006) on a terminal that answers:
/// every frame is followed by a device status request, the run never
/// stalls, and it leaves the terminal as it found it.
#[test]
fn a_big_flock_is_paced_by_the_terminal_and_never_stalls() {
    let _serial = serial();
    let args = ["--big-flock", "6000", "--color", "ember", "--frames", "90", "--seed", "1"];
    let outcome = marked_run(&args, 90, true);
    // The one at startup, then one a frame.
    assert_eq!(outcome.count(b"\x1b[5n"), 91, "{}", outcome.describe());
    assert_no_stall(&outcome);
}

/// A terminal slow to answer gets frames no faster than it answers: here
/// every answer comes 100 ms after the request.
#[test]
fn a_big_flock_waits_for_a_slow_terminal() {
    use std::time::Duration;
    use support::pty::{Action, Reply, Spec, Step};
    let _serial = serial();
    let args = ["--big-flock", "2000", "--color", "ember", "--frames", "15", "--seed", "1"];
    let mut spec = Spec::new(&rbirds(), &args);
    for _ in 0..15 {
        spec = spec.step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")));
    }
    let slow = Reply {
        query: b"\x1b[5n".to_vec(),
        fragments: vec![Vec::new(), b"\x1b[0n".to_vec()],
        gap: Duration::from_millis(100),
        limit: None,
    };
    let outcome = pty::run(&spec.reply(slow));
    cases::assert_clean_exit(&outcome, false);
    let at: Vec<_> =
        outcome.events.iter().filter(|e| e.what == "mark frame").map(|e| e.at).collect();
    let mut gaps: Vec<_> = at.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort();
    assert!(gaps[gaps.len() / 2] >= Duration::from_millis(90), "{gaps:?}");
}

/// Answers 100 ms after every request, as a slow terminal.
fn slow_answers() -> support::pty::Reply {
    support::pty::Reply {
        query: b"\x1b[5n".to_vec(),
        fragments: vec![Vec::new(), b"\x1b[0n".to_vec()],
        gap: std::time::Duration::from_millis(100),
        limit: None,
    }
}

/// A burst of input while a frame waits for its answer (the pointer moved
/// about: more than a read's worth of mouse reports) is read and acted on,
/// and the next frame still waits for the answer.
#[test]
fn input_while_waiting_does_not_let_frames_ahead_of_the_terminal() {
    use std::time::Duration;
    use support::pty::{Action, Spec, Step};
    let _serial = serial();
    let args = ["--big-flock", "2000", "--color", "ember", "--frames", "12", "--seed", "1"];
    let mut spec = Spec::new(&rbirds(), &args);
    for k in 0..12 {
        spec = spec.step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")));
        if k == 3 {
            let motion = b"\x1b[<35;40;12M".repeat(30);
            spec = spec.step(Step::after(Duration::from_millis(20), Action::Input(motion)));
        }
    }
    let outcome = pty::run(&spec.reply(slow_answers()));
    cases::assert_clean_exit(&outcome, false);
    let at: Vec<_> =
        outcome.events.iter().filter(|e| e.what == "mark frame").map(|e| e.at).collect();
    let shortest = at.windows(2).map(|w| w[1] - w[0]).min().unwrap();
    assert!(shortest >= Duration::from_millis(80), "frames {shortest:?} apart");
}

/// Keys read while a frame waits are never dropped: a q after a burst of
/// other keys still ends the run.
#[test]
fn a_key_after_a_burst_while_waiting_is_not_lost() {
    use std::time::Duration;
    use support::pty::{Action, Spec, Step};
    let _serial = serial();
    let args = ["--big-flock", "2000", "--color", "ember", "--seed", "1"];
    let spec = Spec::new(&rbirds(), &args)
        .step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")))
        .step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")))
        .step(Step::after(Duration::from_millis(10), Action::Input(vec![b'x'; 96])))
        .step(Step::after(Duration::from_millis(30), Action::Input(b"xxxxq".to_vec())));
    let spec = spec.reply(slow_answers());
    let outcome = pty::run(&Spec { deadline: Duration::from_secs(8), ..spec });
    assert!(!outcome.timed_out, "q was lost\n{}", outcome.describe());
    cases::assert_clean_exit(&outcome, false);
}

/// A terminal that answers at startup and then stops holds up one frame
/// for a second, and after that the frames go out as in cbirds.
#[test]
fn a_terminal_that_stops_answering_holds_up_one_frame_only() {
    use std::time::Duration;
    use support::pty::{Action, Reply, Spec, Step};
    let _serial = serial();
    let args = ["--big-flock", "2000", "--color", "ember", "--frames", "40", "--seed", "1"];
    let mut spec = Spec::new(&rbirds(), &args);
    for _ in 0..40 {
        spec = spec.step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")));
    }
    // Only the request at startup is answered.
    let once = Reply { limit: Some(1), ..Reply::whole(b"\x1b[5n", b"\x1b[0n") };
    let outcome = pty::run(&spec.reply(once));
    cases::assert_clean_exit(&outcome, false);
    cases::assert_frames(&outcome, 40);
    // One second's wait, then 40 frames at 60 a second.
    assert!(outcome.run_time < Duration::from_secs(4), "{:?}", outcome.run_time);
}

/// A signal while an answer is still to come: the handler reads it before
/// putting the terminal back, so it never reaches the shell, and the exit is
/// 128 + signal as always.
#[test]
fn a_signal_while_an_answer_is_due_leaves_nothing_behind() {
    use std::time::Duration;
    use support::pty::{Action, Reply, Spec, Step};
    let _serial = serial();
    let args = ["--big-flock", "2000", "--color", "ember", "--seed", "1"];
    let mut spec = Spec::new(&rbirds(), &args);
    for _ in 0..5 {
        spec = spec.step(Step::after_output(cases::FRAME_BEGIN, Action::Mark("frame")));
    }
    let spec = spec.step(Step::after(Duration::from_millis(20), Action::Signal(platform::SIGTERM)));
    let slow = Reply {
        query: b"\x1b[5n".to_vec(),
        fragments: vec![Vec::new(), b"\x1b[0n".to_vec()],
        gap: Duration::from_millis(100),
        limit: None,
    };
    let outcome = pty::run(&spec.reply(slow));
    cases::assert_signal_exit(&outcome, platform::SIGTERM, false);
}

/// A terminal that doesn't answer is asked once, and the frames go out as
/// in cbirds.
#[test]
fn a_big_flock_on_a_terminal_that_does_not_answer_is_not_paced() {
    let _serial = serial();
    let args = ["--big-flock", "6000", "--color", "ember", "--frames", "40", "--seed", "1"];
    let outcome = marked_run(&args, 40, false);
    assert_eq!(outcome.count(b"\x1b[5n"), 1, "{}", outcome.describe());
}

/// Without --big-flock the terminal is never asked, and the default flock
/// flies without a stall either.
#[test]
fn the_default_flock_is_not_paced_and_never_stalls() {
    let _serial = serial();
    let outcome = marked_run(&["--color", "ember", "--frames", "90", "--seed", "1"], 90, true);
    assert_eq!(outcome.count(b"\x1b[5n"), 0, "{}", outcome.describe());
    assert_no_stall(&outcome);
}

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
