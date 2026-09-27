#![cfg(unix)]
//! The live iTerm workaround must be selected by the terminal's reply, including
//! through a remote shell, and must retain ordinary terminal restoration.
mod support;
use support::pty::{self, Exit, Reply, Spec, Subject};

#[test]
fn affected_iterm_versions_exit_before_any_image_upload_and_restore_raw_mode() {
    use std::time::Duration;
    for version in ["3.6.6", "3.7.2", "3.7.20260918-nightly", "unknown"] {
        let reply = format!("{version}\x1b\\");
        let outcome = pty::run(
            &Spec::new(
                &Subject { exe: env!("CARGO_BIN_EXE_rbirds").into(), name: "rbirds".into() },
                &["--render", "kitty", "--color", "ember", "--frames", "2"],
            )
            .reply(Reply {
                query: b"\x1b[>q".to_vec(),
                fragments: vec![b"\x1bP>|iTerm2 ".to_vec(), reply.into_bytes()],
                gap: Duration::from_millis(10),
            }),
        );
        assert_eq!(outcome.exit, Some(Exit::Code(1)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        assert!(outcome.contains(b"Kitty animation needs iTerm2 3.7.3 or newer"));
        assert!(outcome.contains(b"--render sixel or --render braille"));
        assert!(!outcome.contains(b"\x1b_G"));
        assert!(!outcome.contains(rbirds::platform::ALT_SCREEN_ON));
    }
}

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
        .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.7.3\x1b\\"))
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
        (b"\x1bP>|iTerm2 3.7.3\x1b\\".as_slice(), true),
        (b"\x1bP>|iTerm2 3.7.20260926-nightly\x1b\\", true),
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

#[cfg(target_os = "macos")]
#[test]
fn a_refused_or_unconsumed_shared_memory_query_keeps_inline_transport() {
    for response in [
        b"\x1b_Gi=1919052146;EBADF:unavailable\x1b\\".as_slice(),
        b"\x1b_Gi=1919052146;OK\x1b\\",
        b"",
    ] {
        let outcome = pty::run(
            &Spec::new(
                &Subject { exe: env!("CARGO_BIN_EXE_rbirds").into(), name: "rbirds".into() },
                &[
                    "--render", "kitty", "--color", "ember", "--birds", "3", "--frames", "2",
                    "--seed", "1",
                ],
            )
            .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.7.3\x1b\\"))
            .reply(Reply::whole(b"\x1b_Ga=q,f=32,t=s,i=1919052146", response)),
        );
        assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        assert!(outcome.contains(b"a=t,q=2,f=32,o=z,i="));
        assert!(!outcome.contains(b"a=t,q=2,f=32,t=s,i="));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn shared_query_waits_for_a_fragmented_error_string_to_finish() {
    use std::time::Duration;
    use support::pty::{Action, Step};
    let query = b"\x1b_Ga=q,f=32,t=s,i=1919052146";
    let outcome = pty::run(
        &Spec::new(
            &Subject { exe: env!("CARGO_BIN_EXE_rbirds").into(), name: "rbirds".into() },
            &[
                "--render", "kitty", "--color", "ember", "--birds", "3", "--frames", "2", "--seed",
                "1",
            ],
        )
        .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.7.3\x1b\\"))
        .reply(Reply {
            query: query.to_vec(),
            fragments: vec![
                b"\x1b_Gi=1919052146;EBADF:cannot read".to_vec(),
                b" image\x1b".to_vec(),
                b"\\".to_vec(),
            ],
            gap: Duration::from_millis(25),
        })
        .step(Step::after_output(b"a=t,q=2,f=32,o=z,i=", Action::Mark("inline upload"))),
    );
    assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
    let last_reply = outcome
        .events
        .iter()
        .find(|event| event.what.contains("1919052146") && event.what.ends_with("[2])"))
        .unwrap_or_else(|| panic!("missing final reply fragment: {}", outcome.describe()));
    assert!(last_reply.at <= outcome.event("mark inline upload").unwrap().at);
    outcome.assert_attributes_restored();
}
