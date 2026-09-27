//! Controlled inputs for the separate CPU measurement executable.
//! These helpers are not used by ordinary live playback.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::platform::{self, Timespec};

pub(crate) const RATE: i64 = 60;

pub(crate) fn frame_time(frame: i64) -> Timespec {
    Timespec { tv_sec: frame / RATE, tv_nsec: frame % RATE * 1_000_000_000 / RATE }
}

pub(crate) struct Gate {
    directory: PathBuf,
    pub warmup: i64,
    pub end: i64,
}

impl Gate {
    pub fn from_environment(frame_limit: i32) -> io::Result<Option<Self>> {
        let Some(directory) = std::env::var_os("RBIRDS_FIXTURE_GATE") else { return Ok(None) };
        let directory = PathBuf::from(directory);
        let config = std::fs::read_to_string(directory.join("config"))?;
        let values = config
            .split_whitespace()
            .map(str::parse::<i64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(io::Error::other)?;
        let invalid = || {
            io::Error::other(
                "fixture gate requires warmup >= 0, frames > 0 and --frames equal to their sum",
            )
        };
        let [warmup, frames] = values.as_slice() else { return Err(invalid()) };
        let end = warmup.checked_add(*frames).ok_or_else(invalid)?;
        if *warmup < 0 || *frames <= 0 || end != i64::from(frame_limit) {
            return Err(invalid());
        }
        Ok(Some(Self { directory, warmup: *warmup, end }))
    }

    pub fn wait(&self, stage: &str) -> io::Result<()> {
        std::fs::write(self.directory.join(stage), b"ready\n")?;
        let ack = self.directory.join(format!("{stage}-ack"));
        let started = Instant::now();
        while !ack.try_exists()? {
            if platform::exit_requested() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "fixture cancelled"));
            }
            if started.elapsed() >= Duration::from_secs(30) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture counter handshake timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_clock_has_no_accumulating_rounding_error() {
        let mut previous = frame_time(0);
        for frame in 1..=60 * 180 {
            let now = frame_time(frame);
            let ns =
                (now.tv_sec - previous.tv_sec) * 1_000_000_000 + now.tv_nsec - previous.tv_nsec;
            assert!([16_666_666, 16_666_667].contains(&ns));
            if frame % 60 == 0 {
                assert_eq!(now.tv_sec, frame / 60);
                assert_eq!(now.tv_nsec, 0);
            }
            previous = now;
        }
    }
}
