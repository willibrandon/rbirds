#![cfg(unix)]
//! Sixel negotiation and cleanup exercised through the real application.
mod support;
use support::pty::{self, Action, Exit, Reply, Spec, Step, Subject};

fn spec() -> Spec {
    Spec::new(
        &Subject { exe: env!("CARGO_BIN_EXE_rbirds").into(), name: "rbirds".into() },
        &["--render", "sixel", "--color", "ember", "--birds", "3", "--frames", "2", "--seed", "1"],
    )
}

#[test]
fn sixel_negotiates_virtual_pixels_renders_frames_and_restores_mode() {
    for was_enabled in [false, true] {
        let mode = if was_enabled { &b"\x1b[?80;1$y"[..] } else { &b"\x1b[?80;2$y"[..] };
        let spec = spec()
            .reply(Reply {
                query: b"\x1b[c".to_vec(),
                fragments: vec![b"\x1b[?64;".to_vec(), b"4;22c".to_vec()],
                gap: std::time::Duration::from_millis(10),
            })
            .reply(Reply::whole(b"\x1b[16t", b"\x1b[6;20;10t"))
            .reply(Reply::whole(b"\x1b[?80$p", mode));
        let outcome = pty::run(&spec);
        assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        assert_eq!(
            outcome.transcript.windows(b"\x1bP0;1q".len()).filter(|s| *s == b"\x1bP0;1q").count(),
            2
        );
        assert!(
            outcome.contains(b"\"1;1;800;480"),
            "use 10x20 virtual cells rather than PTY pixel size"
        );
        assert!(outcome.contains(b"\x1b[?80h"));
        assert_eq!(outcome.contains(b"\x1b[?80l"), !was_enabled);
        assert!(outcome.transcript.ends_with(rbirds::platform::ALT_SCREEN_OFF));
    }
}

#[test]
fn unsupported_or_sizeless_sixel_fails_cleanly() {
    for (capabilities, size_reply, message) in [
        (&b"\x1b[?64;22c"[..], &b""[..], "terminal did not advertise Sixel"),
        (&b"\x1b[?64;4c"[..], &b"\x1b[6;0;10t"[..], "graphics cell size"),
    ] {
        let outcome = pty::run(
            &spec()
                .size(rbirds::platform::WinSize { row: 24, col: 80, xpixel: 0, ypixel: 0 })
                .reply(Reply::whole(b"\x1b[c", capabilities))
                .reply(Reply::whole(b"\x1b[16t", size_reply)),
        );
        assert_eq!(outcome.exit, Some(Exit::Code(1)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        assert!(outcome.contains(message.as_bytes()));
        assert!(!outcome.contains(b"\x1bP"));
        assert!(!outcome.contains(rbirds::platform::ALT_SCREEN_ON));
    }
}

#[test]
fn native_pixel_dimensions_work_when_the_cell_query_is_unimplemented() {
    let outcome = pty::run(
        &spec()
            .reply(Reply::whole(b"\x1b[c", b"\x1b[?64;4c"))
            .reply(Reply::whole(b"\x1b[?80$p", b"\x1b[?80;2$y")),
    );
    assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
    outcome.assert_attributes_restored();
    assert!(outcome.contains(b"\"1;1;640;384"));
    assert!(outcome.transcript.ends_with(rbirds::platform::ALT_SCREEN_OFF));
}

#[test]
fn iterm_frames_retire_the_previous_image_inside_each_synchronized_update() {
    for (mode, restore) in [(b"\x1b[?80;1$y", b"\x1b[?80h"), (b"\x1b[?80;2$y", b"\x1b[?80l")] {
        // No environment hint or CSI 16 t response: this also covers a remote shell.
        let outcome = pty::run(
            &spec()
                .reply(Reply::whole(b"\x1b[c", b"\x1b[?64;4c"))
                .reply(Reply {
                    query: b"\x1b[>q".to_vec(),
                    fragments: vec![b"\x1bP>|iTerm2 ".to_vec(), b"3.6.6\x1b\\".to_vec()],
                    gap: std::time::Duration::from_millis(10),
                })
                .reply(Reply::whole(b"\x1b[6n", b"\x1b[1;2R"))
                .reply(Reply::whole(b"\x1b[?80$p", mode)),
        );
        assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        let prefix = b"\x1b[?2026h\x1b[2J\x1b[H\x1b[0;38;2;18;18;23m";
        assert_eq!(outcome.transcript.windows(prefix.len()).filter(|s| *s == prefix).count(), 2);
        assert!(outcome.contains("█".as_bytes()), "paint opaque sky outside the cropped raster");
        assert!(outcome.contains(b"\x1b[?80l"));
        assert_eq!(outcome.count(b"\x1b\\\x1b[?80h"), 2, "restore display mode after each crop");
        let cleanup = outcome.transcript.windows(3).position(|s| s == b"\x18\x1b\x18").unwrap();
        assert!(outcome.transcript[cleanup + 3..].starts_with(restore));
        assert!(outcome.transcript.ends_with(rbirds::platform::ALT_SCREEN_OFF));
    }
}

#[test]
fn iterm_keeps_full_rasters_without_a_changeable_display_mode() {
    for mode in [b"".as_slice(), b"\x1b[?80;0$y", b"\x1b[?80;3$y", b"\x1b[?80;4$y"] {
        let outcome = pty::run(
            &spec()
                .reply(Reply::whole(b"\x1b[c", b"\x1b[?64;4c"))
                .reply(Reply::whole(b"\x1b[16t", b"\x1b[6;20;10t"))
                .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.6.6\x1b\\"))
                .reply(Reply::whole(b"\x1b[6n", b"\x1b[1;2R"))
                .reply(Reply::whole(b"\x1b[?80$p", mode)),
        );
        outcome.assert_exit_code(0);
        outcome.assert_attributes_restored();
        assert_eq!(outcome.count(b"\x1b[?2026h\x1b[2J\x1b[H\x1bP0;1q\"1;1;800;480"), 2);
    }
}

#[test]
fn interrupted_iterm_output_restores_both_original_display_modes() {
    use std::time::Duration;
    for (mode, restore) in [(b"\x1b[?80;1$y", b"\x1b[?80h"), (b"\x1b[?80;2$y", b"\x1b[?80l")] {
        let mut run = spec();
        run.args = ["--render", "sixel", "--color", "ember", "--birds", "4096", "--seed", "42"]
            .map(Into::into)
            .into();
        let outcome = pty::run(
            &run.size(rbirds::platform::WinSize { row: 64, col: 200, xpixel: 2800, ypixel: 2176 })
                .reply(Reply::whole(b"\x1b[c", b"\x1b[?64;4c"))
                .reply(Reply::whole(b"\x1b[16t", b"\x1b[6;34;14t"))
                .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.6.6\x1b\\"))
                .reply(Reply::whole(b"\x1b[6n", b"\x1b[1;2R"))
                .reply(Reply::whole(b"\x1b[?80$p", mode))
                .step(Step::after_output(
                    b"\x1bP0;1q",
                    Action::PauseReading(Duration::from_millis(800)),
                ))
                .step(Step::after(
                    Duration::from_millis(200),
                    Action::Signal(rbirds::platform::SIGINT),
                )),
        );
        pty::cases::assert_signal_exit(&outcome, rbirds::platform::SIGINT, false);
        let cleanup = outcome.transcript.windows(3).position(|s| s == b"\x18\x1b\x18").unwrap();
        assert!(outcome.transcript[cleanup + 3..].starts_with(restore));
        let start =
            outcome.transcript[..cleanup].windows(6).rposition(|s| s == b"\x1bP0;1q").unwrap();
        assert!(
            !outcome.transcript[start..cleanup].windows(2).any(|s| s == b"\x1b\\"),
            "exercise cleanup while a Sixel image is incomplete"
        );
    }
}

#[test]
fn iterm_keeps_full_rasters_for_wide_or_unconfirmed_backdrop_characters() {
    for reply in [b"".as_slice(), b"\x1b[1;3R", b"\x1b[1;1R", b"invalidR"] {
        let outcome = pty::run(
            &spec()
                .reply(Reply::whole(b"\x1b[c", b"\x1b[?64;4c"))
                .reply(Reply::whole(b"\x1b[16t", b"\x1b[6;20;10t"))
                .reply(Reply::whole(b"\x1b[>q", b"\x1bP>|iTerm2 3.6.6\x1b\\"))
                .reply(Reply::whole(b"\x1b[6n", reply))
                .reply(Reply::whole(b"\x1b[?80$p", b"\x1b[?80;2$y")),
        );
        assert_eq!(outcome.exit, Some(Exit::Code(0)), "{}", outcome.describe());
        outcome.assert_attributes_restored();
        let prefix = b"\x1b[?2026h\x1b[2J\x1b[H\x1bP0;1q\"1;1;800;480";
        assert_eq!(outcome.transcript.windows(prefix.len()).filter(|s| *s == prefix).count(), 2);
        assert!(outcome.transcript.ends_with(rbirds::platform::ALT_SCREEN_OFF));
    }
}
