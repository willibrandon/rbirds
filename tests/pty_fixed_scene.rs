#![cfg(unix)]

mod support;

use std::time::Duration;
use support::pty::{self, Action, Exit, Reply, Spec, Step, Subject};

const CHILD: &str = "RBIRDS_FIXED_SCENE_CHILD";

#[test]
fn fixture_child() {
    let Ok(renderer) = std::env::var(CHILD) else { return };
    let mut args = vec![
        "fixed-scene",
        "--render",
        &renderer,
        "--seed",
        "42",
        "--frames",
        "18",
        "--birds",
        "40",
        "--color",
        "ember",
        "--hawks",
        "1",
        "--depth",
        "--trails",
    ];
    if std::env::var_os("RBIRDS_FIXED_SCENE_UNLOCKED").is_some() {
        args.push("--unlock-fps");
    }
    let mut program = rbirds::app::Program::new();
    let mut output = rbirds::stdio::CStdout::new();
    rbirds::app::read_options(
        &mut program,
        &args.iter().map(std::ffi::OsString::from).collect::<Vec<_>>(),
        &mut output,
    )
    .unwrap();
    let code = rbirds::live::run_fixed_scene(&mut program, &mut output);
    output.flush();
    drop(program);
    std::process::exit(code);
}

fn subject() -> Subject {
    Subject { exe: std::env::current_exe().unwrap(), name: "fixed-scene-test".into() }
}

fn spec(renderer: &str) -> Spec {
    Spec::new(&subject(), &["fixture_child", "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, renderer)
        .reply(Reply::whole(b"\x1b[c", b"\x1b[?62;4c"))
        .reply(Reply::whole(b"\x1b[16t", b"\x1b[6;16;8t"))
        .reply(Reply::whole(b"\x1b[?80$p", b"\x1b[?80;2$y"))
}

#[test]
fn identical_frames_despite_pacing_and_output_delays() {
    for renderer in ["braille", "sixel", "kitty"] {
        let path = std::env::temp_dir()
            .join(format!("rbirds-fixed-{}-{renderer}.jsonl", std::process::id()));
        let normal = pty::run(&spec(renderer).env("RBIRDS_TRACE", &path));
        assert_eq!(normal.exit, Some(Exit::Code(0)), "{}", normal.describe());
        normal.assert_attributes_restored();
        let trace = std::fs::read_to_string(&path).unwrap();
        assert_eq!(trace.lines().count(), 19);
        assert!(trace.lines().next().unwrap().contains("\"simulation_clock\":\"fixed-60-hz\""));
        assert!(trace.lines().next().unwrap().contains("\"fixed_scene\":\"v1;seed=42;Sim"));
        let unlocked = pty::run(&spec(renderer).env("RBIRDS_FIXED_SCENE_UNLOCKED", "1"));
        let delayed = pty::run(&spec(renderer).step(Step::after_output(
            b"\x1b[?2026l",
            Action::PauseReading(Duration::from_millis(150)),
        )));
        for run in [unlocked, delayed] {
            assert_eq!(run.exit, Some(Exit::Code(0)), "{}", run.describe());
            run.assert_attributes_restored();
            assert_eq!(run.transcript, normal.transcript, "{renderer}: timing changed output");
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn input_and_resizes_invalidate_the_fixture_and_restore_the_terminal() {
    for action in [
        Action::Input(b" ".to_vec()),
        Action::Input(b"\x1b[<35;2;2M".to_vec()),
        Action::Resize(rbirds::platform::WinSize { row: 25, col: 81, xpixel: 648, ypixel: 400 }),
    ] {
        let outcome = pty::run(&spec("braille").step(Step::after_output(b"\x1b[?2026l", action)));
        assert_eq!(outcome.exit, Some(Exit::Code(1)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        assert!(outcome.contains(b"Fixed scene invalidated"));
    }
}
