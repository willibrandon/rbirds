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
rust_case!(quit_key_flies_out_then_exits, cases::quit_key);
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
    let spec = Spec::new(&subject, &["panic_child", "--exact", "--test-threads=1"])
        .env(PANIC_CHILD, "1")
        .step(Step::after_output(PANIC_READY, Action::SnapshotTermios));
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
