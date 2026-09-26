//! Optional live measurements. Samples stay in bounded memory until exit so
//! recording a trace does not add disk writes to the animation loop.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::time::{Duration, Instant};

use crate::platform;

const MAX_SAMPLES: usize = 65_536;

/// Keep the cadence independent of small sleep overruns. An over-budget frame
/// starts its successor immediately and rebases the schedule; it never causes
/// a burst of catch-up renders or a busy wait.
pub struct FramePacer {
    next: Instant,
}

impl FramePacer {
    const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / crate::config::FRAME_RATE as u64);

    pub fn new(start: Instant) -> Self {
        Self { next: start + Self::PERIOD }
    }

    pub fn delay_after(&mut self, finished: Instant) -> Duration {
        let delay = self.next.saturating_duration_since(finished);
        self.next =
            if finished >= self.next { finished + Self::PERIOD } else { self.next + Self::PERIOD };
        delay
    }

    pub fn target(&self) -> Instant {
        self.next - Self::PERIOD
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameProfile {
    pub started: Instant,
    pub target: Instant,
    pub updated: Instant,
    pub composed: Instant,
    pub rendered: Instant,
}

impl FrameProfile {
    pub fn new() -> Self {
        let started = Instant::now();
        Self { started, target: started, updated: started, composed: started, rendered: started }
    }

    pub fn updated(&mut self) {
        self.updated = Instant::now();
        self.composed = self.updated;
    }

    pub fn composed(&mut self) {
        self.composed = Instant::now();
    }

    pub fn rendered(&mut self) {
        self.rendered = Instant::now();
    }
}

impl Default for FrameProfile {
    fn default() -> Self {
        Self::new()
    }
}

struct Sample {
    start_us: u64,
    wake_late_us: u64,
    update_us: u64,
    compose_us: u64,
    encode_us: u64,
    flush_us: u64,
    sleep_us: u64,
    bytes: usize,
    input_bytes: usize,
}

pub struct Trace {
    file: File,
    started: Instant,
    cpu_started: Duration,
    samples: Vec<Sample>,
    omitted: u64,
    renderer: &'static str,
    viewport: [i32; 4],
}

fn micros(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

impl Trace {
    pub fn from_environment() -> io::Result<Option<Self>> {
        let Some(path) = std::env::var_os("RBIRDS_TRACE") else { return Ok(None) };
        let mut samples = Vec::new();
        samples.try_reserve_exact(MAX_SAMPLES).map_err(io::Error::other)?;
        Ok(Some(Self {
            file: File::create(path)?,
            started: Instant::now(),
            cpu_started: platform::process_cpu_time()?,
            samples,
            omitted: 0,
            renderer: "unset",
            viewport: [0; 4],
        }))
    }

    /// Excludes terminal negotiation and sprite upload from steady-state CPU.
    pub fn begin(&mut self, renderer: &'static str, viewport: [i32; 4]) -> io::Result<()> {
        self.cpu_started = platform::process_cpu_time()?;
        self.started = Instant::now();
        self.renderer = renderer;
        self.viewport = viewport;
        Ok(())
    }

    pub fn record(
        &mut self,
        profile: FrameProfile,
        flushed: Instant,
        sleep: Duration,
        bytes: usize,
        input_bytes: usize,
    ) {
        if self.samples.len() == MAX_SAMPLES {
            self.omitted += 1;
            return;
        }
        self.samples.push(Sample {
            start_us: micros(profile.started.duration_since(self.started)),
            wake_late_us: micros(profile.started.saturating_duration_since(profile.target)),
            update_us: micros(profile.updated.duration_since(profile.started)),
            compose_us: micros(profile.composed.duration_since(profile.updated)),
            encode_us: micros(profile.rendered.duration_since(profile.composed)),
            flush_us: micros(flushed.duration_since(profile.rendered)),
            sleep_us: micros(sleep),
            bytes,
            input_bytes,
        });
    }

    pub fn finish(self) -> io::Result<()> {
        let elapsed = self.started.elapsed();
        let cpu = platform::process_cpu_time()?.saturating_sub(self.cpu_started);
        let mut out = BufWriter::new(self.file);
        writeln!(
            out,
            "{{\"kind\":\"summary\",\"version\":1,\"renderer\":\"{}\",\"viewport\":{:?},\"wall_us\":{},\"cpu_us\":{},\"samples\":{},\"omitted\":{}}}",
            self.renderer,
            self.viewport,
            micros(elapsed),
            micros(cpu),
            self.samples.len(),
            self.omitted
        )?;
        for sample in self.samples {
            writeln!(
                out,
                "{{\"kind\":\"frame\",\"start_us\":{},\"wake_late_us\":{},\"update_us\":{},\"compose_us\":{},\"encode_us\":{},\"flush_us\":{},\"sleep_us\":{},\"bytes\":{},\"input_bytes\":{}}}",
                sample.start_us,
                sample.wake_late_us,
                sample.update_us,
                sample.compose_us,
                sample.encode_us,
                sample.flush_us,
                sample.sleep_us,
                sample.bytes,
                sample.input_bytes
            )?;
        }
        out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_overshoot_does_not_accumulate() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        let work = Duration::from_millis(3);
        let overshoot = Duration::from_millis(2);
        assert_eq!(pacer.delay_after(start + work), FramePacer::PERIOD - work);
        for frame in 1..1000 {
            let finished = start + FramePacer::PERIOD * frame + overshoot + work;
            assert_eq!(pacer.delay_after(finished), FramePacer::PERIOD - overshoot - work);
        }
    }

    #[test]
    fn overload_rebases_without_catch_up_bursts() {
        let start = Instant::now();
        let mut pacer = FramePacer::new(start);
        let delayed = start + Duration::from_secs(2);
        assert_eq!(pacer.delay_after(delayed), Duration::ZERO);
        let work = Duration::from_millis(3);
        assert_eq!(pacer.delay_after(delayed + work), FramePacer::PERIOD - work);
    }
}
