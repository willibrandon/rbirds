//! Native CLI and file workflows: these do not need a C compiler or a terminal.
use std::ffi::OsStr;
use std::process::{Command, Output};

fn run(args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rbirds")).args(args).output().unwrap()
}

fn strings(args: &[&str]) -> Output {
    run(&args.iter().map(OsStr::new).collect::<Vec<_>>())
}

#[test]
fn help_completions_and_invalid_choice_include_sixel() {
    for args in
        [vec!["--help"], vec!["-h"], vec!["--completion", "zsh"], vec!["--completion", "fish"]]
    {
        let output = strings(&args);
        assert!(output.status.success());
        if args[0] == "--help" || args[0] == "-h" {
            assert!(output.stdout.starts_with(b"rbirds - a flock of birds in your terminal.\n"));
        }
        assert!(String::from_utf8(output.stdout).unwrap().contains("sixel"));
        assert!(output.stderr.is_empty());
    }
    let output = strings(&["--render", "unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("kitty, braille, sextants, blocks, sixel")
    );
}

#[test]
fn sixel_bench_is_headless_and_reports_encoded_bytes() {
    let output = strings(&["--render", "sixel", "--bench", "2", "--birds", "3", "--seed", "1"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!output.stdout.contains(&0x1b));
    let text = String::from_utf8(output.stdout).unwrap();
    let bytes = text.lines().find_map(|line| line.strip_prefix("bytes/frame")).unwrap();
    assert!(bytes.split_whitespace().next().unwrap().parse::<u64>().unwrap() > 100);
}

#[test]
fn unicode_recording_paths_work_and_sixel_keeps_full_colour_gif_and_braille_cast() {
    let dir = std::env::temp_dir().join(format!("rbirds-鳥-é-recording-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for suffix in ["gif", "cast"] {
        let mut files = Vec::new();
        for mode in ["sixel", "kitty"] {
            let path = dir.join(format!("flock-{mode}.{suffix}"));
            let output = run(&[
                "--record".as_ref(),
                path.as_os_str(),
                "--render".as_ref(),
                mode.as_ref(),
                "--birds".as_ref(),
                "3".as_ref(),
                "--seed".as_ref(),
                "42".as_ref(),
                "--record-seconds".as_ref(),
                "1".as_ref(),
                "--record-fps".as_ref(),
                "2".as_ref(),
                "--record-size".as_ref(),
                "40x14".as_ref(),
            ]);
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("2 frames,"));
            files.push(std::fs::read(&path).unwrap());
            std::fs::remove_file(path).unwrap();
        }
        if suffix == "gif" {
            assert!(files[0].starts_with(b"GIF89a"));
            assert_eq!(files[0].last(), Some(&b';'));
            assert_eq!(files[0], files[1], "Sixel records the full-colour sprite composition");
        } else {
            // Only the header's wall-clock timestamp may differ between runs.
            let bodies: Vec<_> = files
                .iter()
                .map(|file| {
                    assert!(file.starts_with(b"{\"version\": 2,"));
                    let start = file.iter().position(|&byte| byte == b'\n').unwrap() + 1;
                    &file[start..]
                })
                .collect();
            assert_eq!(bodies[0], bodies[1]);
            assert_eq!(
                bodies[0].split(|&byte| byte == b'\n').filter(|line| !line.is_empty()).count(),
                4
            );
        }
    }
    let missing = dir.join("missing/flock.gif");
    let output = run(&["--record".as_ref(), missing.as_os_str(), "--birds".as_ref(), "1".as_ref()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
    assert!(!missing.exists());
    std::fs::remove_dir(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn ill_formed_utf16_is_a_usage_error() {
    use std::os::windows::ffi::OsStringExt;
    let path = std::ffi::OsString::from_wide(&[0xd800]);
    let output = run(&["--record".as_ref(), &path]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unicode"));
}
