//! `--bench`: frames built with no terminal in the way, and the numbers.
//! Translated from cbirds `boids.c` (`run_benchmark`), including its quirk of
//! hunting once before `render_frame` flies (and hunts) again.

#![forbid(unsafe_code)]

use crate::app::{EXIT_FAILURE, EXIT_SUCCESS, Settings};
use crate::cfmt;
use crate::config::*;
use crate::platform;
use crate::render::Renderer;
use crate::render::kitty::KittyGraphics;
use crate::simulation::{Bird, RenderMode, Sim};
use crate::spatial_grid::SpatialGrid;
use crate::stdio::{CStdout, eprint};

/// What one benchmark run measured, before it is printed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchResult {
    pub seconds: f64,
    pub bytes: f64,
}

/// The simulation half of one benchmark frame: the snapshot and grid, a hunt,
/// and `render_frame`'s flight, which hunts again.
pub fn bench_step(
    sim: &mut Sim,
    birds: &mut [Bird],
    snapshot: &mut [Bird],
    grid: &mut SpatialGrid,
) {
    let _ = sim.snapshot_and_build(birds, snapshot, grid);
    sim.hunt(snapshot);
    sim.advance(birds, snapshot, grid);
}

/// The frames of a benchmark, with the time they took. `None` where the C
/// returns EXIT_FAILURE before printing anything.
pub fn bench_frames(
    sim: &mut Sim,
    renderer: &mut Renderer,
    settings: &Settings,
) -> Option<BenchResult> {
    sim.settle_the_palette_without_a_terminal();
    sim.apply_screen_size(200, 50, 1600, 800);
    // No terminal, so unasked means the sprites, as in a recording.
    if sim.render_mode == RenderMode::Unset {
        sim.render_mode = RenderMode::Kitty;
    }
    sim.settle_the_bird_size();
    if sim.drawing_with_text()
        && let Err(error) = renderer.prepare_text_renderer(
            sim,
            settings.sprite_path.as_deref(),
            &settings.program_name,
        )
    {
        if let crate::sprites::SpriteError::Fatal(message) = error {
            eprint(&message);
        }
        return None;
    }
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    let mut grid = SpatialGrid::new(SPATIAL_CELL_SIZE).ok()?;
    grid.prepare(sim.screen.width, sim.screen.height, sim.config.birds).ok()?;
    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).ok()?;

    let count = sim.config.birds.max(0) as usize;
    let mut birds: Vec<Bird> = Vec::new();
    let mut snapshot: Vec<Bird> = Vec::new();
    birds.try_reserve_exact(count).ok()?;
    snapshot.try_reserve_exact(count).ok()?;
    birds.resize(count, Bird::default());
    snapshot.resize(count, Bird::default());
    sim.rng.seed(if settings.requested_seed >= 0 { settings.requested_seed as u32 } else { 1 });
    sim.initialize_birds(&mut birds);
    sim.place_hawks();

    let mut bytes = 0.0_f64;
    let start = platform::monotonic_now();
    for _ in 0..settings.bench_frames {
        bench_step(sim, &mut birds, &mut snapshot, &mut grid);
        graphics.clear();
        let _ = renderer.queue_render_frame(&mut graphics, sim, &birds);
        bytes += graphics.len() as f64;
    }
    let finish = platform::monotonic_now();
    Some(BenchResult { seconds: crate::live::elapsed_seconds(&start, &finish), bytes })
}

/// `run_benchmark`.
pub fn run_benchmark(
    sim: &mut Sim,
    renderer: &mut Renderer,
    settings: &Settings,
    stdout: &mut CStdout,
) -> i32 {
    let Some(result) = bench_frames(sim, renderer, settings) else { return EXIT_FAILURE };
    let frames = f64::from(settings.bench_frames);
    let per_frame = result.seconds / frames;
    let bytes = result.bytes;
    let report = format!(
        "birds        {}\nflocks       {}\nhawks        {}\nviewport     {}x{} px\n\
         render       {}\nframes       {}\nframe time   {} ms\nceiling      {} fps\n\
         bytes/frame  {} ({} KB)\nat {} fps    {} MB/s\n",
        sim.config.birds,
        sim.config.flocks,
        sim.config.hawks,
        sim.screen.width,
        sim.screen.height,
        sim.render_mode.name(),
        settings.bench_frames,
        cfmt::fixed(per_frame * 1000.0, 3),
        cfmt::fixed(1.0 / per_frame, 0),
        cfmt::fixed(bytes / frames, 0),
        cfmt::fixed(bytes / frames / 1024.0, 1),
        FRAME_RATE,
        cfmt::fixed(bytes / frames * f64::from(FRAME_RATE) / 1e6, 1),
    );
    stdout.print(report.as_bytes());
    EXIT_SUCCESS
}
