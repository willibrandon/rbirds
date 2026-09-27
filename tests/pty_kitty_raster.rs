#![cfg(unix)]
//! The live iTerm workaround must be selected by the terminal's reply, including
//! through a remote shell, and must retain ordinary terminal restoration.
mod support;
use support::pty::{self, Exit, Reply, Spec, Subject};

#[test]
fn interrupted_composed_upload_restores_the_terminal_after_output_resumes() {
    use std::time::Duration;
    use support::pty::{Action, Step};
    let outcome = pty::run(
        &Spec::new(
            &Subject { exe: env!("CARGO_BIN_EXE_rbirds").into(), name: "rbirds".into() },
            &["--render", "kitty", "--color", "ember", "--seed", "1"],
        )
        .size(rbirds::platform::WinSize { row: 50, col: 200, xpixel: 1600, ypixel: 800 })
        .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.6.6\x1b\\"))
        .step(Step::after_output(
            b"a=t,q=2,f=32,o=z,i=",
            Action::PauseReading(Duration::from_millis(500)),
        ))
        .step(Step::after(Duration::from_millis(100), Action::Signal(rbirds::platform::SIGTERM))),
    );
    assert_eq!(outcome.exit, Some(Exit::Code(143)), "{}", outcome.describe());
    outcome.assert_attributes_restored();
    assert!(outcome.contains(b"\x1b_Ga=d,d=A"));
    assert!(outcome.transcript.ends_with(rbirds::platform::ALT_SCREEN_OFF));
    assert!(
        outcome.event("sent signal").unwrap().at < outcome.event("reading resumed").unwrap().at
    );
}

#[test]
fn iterm_uses_explicit_ids_and_composed_surfaces_while_other_terminals_keep_atlases() {
    for (version, raster) in [
        (b"\x1bP>|iTerm2 3.6.6\x1b\\".as_slice(), true),
        (b"\x1bP>|kitty 0.44.0\x1b\\", false),
        (b"", false),
    ] {
        let spec = Spec::new(
            &Subject { exe: env!("CARGO_BIN_EXE_rbirds").into(), name: "rbirds".into() },
            &[
                "--render", "kitty", "--color", "ember", "--birds", "3", "--frames", "2", "--seed",
                "1",
            ],
        )
        .reply(Reply::whole(b"\x1b[>q", version));
        let outcome = pty::run(&spec);
        assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        assert_eq!(outcome.contains(b"a=t,q=2,f=32,o=z,i="), raster);
        assert_eq!(outcome.contains(b"a=p,i="), raster);
        assert_eq!(outcome.contains(b"a=t,q=2,f=100,I="), !raster);
        assert!(outcome.contains(b"\x1b_Ga=d,d=A"));
        assert!(outcome.transcript.ends_with(rbirds::platform::ALT_SCREEN_OFF));
    }
}
