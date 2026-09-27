#![cfg(target_os = "macos")]

mod support;
use rbirds::platform::{self, SharedImage};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

fn reader() -> PathBuf {
    static READER: OnceLock<PathBuf> = OnceLock::new();
    READER
        .get_or_init(|| {
            let root = support::oracle::repository();
            let dir = root.join("target/oracle");
            std::fs::create_dir_all(&dir).unwrap();
            let exe =
                dir.join(format!("shared-image-{}-{}", std::env::consts::ARCH, std::process::id()));
            let result = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
                .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-arch"])
                .arg(if cfg!(target_arch = "x86_64") { "x86_64" } else { "arm64" })
                .arg(root.join("tools/oracle/shared_image.c"))
                .arg("-o")
                .arg(&exe)
                .output()
                .unwrap();
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
            exe
        })
        .clone()
}

fn read(name: &[u8], action: &str, length: usize) -> Vec<u8> {
    let out = Command::new(reader())
        .arg(action)
        .arg(std::str::from_utf8(name).unwrap())
        .arg(length.to_string())
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    out.stdout
}

#[test]
fn independent_reader_checks_pixels_permissions_pending_frames_and_cleanup() {
    let pixels: Vec<_> = (0..240).map(|n| (n * 7) as u8).collect();
    let expected: Vec<_> =
        (0..4).flat_map(|row| pixels[36 + row * 40..60 + row * 40].iter().copied()).collect();
    let mut image = SharedImage::new().unwrap();
    let name = image.name().to_vec();
    assert!(!image.pending().unwrap());
    assert!(image.stage(&pixels, 40, 36, 24, 4).unwrap());
    assert!(image.pending().unwrap());
    assert_eq!(read(&name, "peek", expected.len()), expected);
    assert!(!image.stage(&[0; 16], 16, 0, 16, 1).unwrap());
    assert_eq!(
        read(&name, "consume", expected.len()),
        expected,
        "an unread image cannot be overwritten"
    );
    assert!(!image.pending().unwrap());
    assert!(image.stage(&pixels, 40, 0, 40, 6).unwrap());
    assert_eq!(read(&name, "peek", pixels.len()), pixels);
    drop(image);
    assert!(read(&name, "missing", 0).is_empty());
}

#[test]
fn invalid_regions_do_not_publish_resources() {
    let mut image = SharedImage::new().unwrap();
    for (stride, offset, row_bytes, rows) in [
        (4, 0, 4, 0),
        (4, 0, 0, 1),
        (3, 0, 4, 1),
        (4, 1, 4, 1),
        (usize::MAX, 0, 4, 3),
        (4, usize::MAX, 4, 1),
    ] {
        assert!(image.stage(&[0; 4], stride, offset, row_bytes, rows).is_err());
        assert!(!image.pending().unwrap());
    }
}

#[test]
fn resource_child() {
    let Ok(mode) = std::env::var("RBIRDS_SHARED_IMAGE_CHILD") else {
        return;
    };
    platform::install_signal_handlers();
    let mut images = Vec::new();
    for _ in 0..4 {
        let mut image = SharedImage::new().unwrap();
        assert!(image.stage(&[42; 16], 8, 0, 8, 2).unwrap());
        println!("IMAGE {}", std::str::from_utf8(image.name()).unwrap());
        images.push(image);
    }
    std::io::stdout().flush().unwrap();
    if mode == "panic" {
        panic!("exercise resource unwinding");
    }
    std::thread::sleep(std::time::Duration::from_secs(10));
    panic!("parent did not send the expected signal");
}

#[test]
fn pending_objects_are_removed_by_unwinding_and_signal_exit() {
    for (mode, expected) in [("panic", 101), ("signal", 143)] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "resource_child", "--nocapture"])
            .env("RBIRDS_SHARED_IMAGE_CHILD", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut names = Vec::new();
        while names.len() < 4 {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if let Some(name) = line.trim().strip_prefix("IMAGE ") {
                names.push(name.as_bytes().to_vec());
            }
        }
        if mode == "signal" {
            platform::send_signal(child.id() as i32, platform::SIGTERM).unwrap();
        }
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(expected));
        for name in names {
            assert!(read(&name, "missing", 0).is_empty());
        }
    }
}

#[test]
fn negotiated_live_transport_cleans_up_after_completion_signal_and_terminal_loss() {
    let result = Command::new("python3")
        .arg(support::oracle::repository().join("tools/test_shared_renderer.py"))
        .arg(env!("CARGO_BIN_EXE_rbirds"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
