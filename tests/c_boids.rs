//! Translation of cbirds `tests/boids_test.c`: one `#[test]` per C test,
//! under the C test's own name, with every assertion and tolerance kept.
//!
//! The C suite runs its tests in one process, and several begin from what
//! earlier tests left in boids.c's globals. Each test here therefore starts
//! from the exact state the C test was entered with, captured by running the
//! unmodified C suite (tests/support/suite.rs); the body then does what the C
//! body does. Local test fixtures (the C tests' own bird arrays) are built the
//! same way they are in C.

// The translation keeps the C tests' index loops and their assertions on
// constants (`LEGEND_BAR_CELLS == 12`), so a reader can hold the two side by side.
#![allow(clippy::needless_range_loop, clippy::assertions_on_constants)]

mod support;

use std::f64::consts::PI;
use std::ffi::OsString;

use rbirds::app::{OPTIONS, frame_delay_after, read_options};
use rbirds::config::*;
use rbirds::image::png;
use rbirds::palette::{
    HAWK_COLOURS, PALETTE_COUNT, PALETTES, Theme, colour_distance, contrast_between, palette_named,
    parse_osc_colour, saturation_of,
};
use rbirds::record::{record_delay_for, run_recording};
use rbirds::render::kitty::KittyGraphics;
use rbirds::render::panel::build_legend;
use rbirds::simulation::*;
use rbirds::spatial_grid::SpatialGrid;
use rbirds::sprites::{CATALOGUE_IMAGES, empty_catalogue, free_sprites};
use rbirds::stdio::CStdout;
use support::sim::World;
use support::suite::entry;

/// The C suite's state as this test begins; skips only where the oracle may.
macro_rules! enter {
    ($name:literal) => {
        match entry($name) {
            Some(world) => world,
            None => return,
        }
    };
}

/// The speed slider's two ends, written out rather than derived.
const PACE_FLOOR: f64 = 0.2;
const PACE_CEILING: f64 = 2.6;

fn reset_test_config(w: &mut World) {
    let sim = &mut w.sim;
    sim.frame_seconds = 1.0 / f64::from(FRAME_RATE);
    let c = &mut sim.config;
    c.birds = 800;
    c.bird_size = DEFAULT_BIRD_SIZE;
    c.boundary_notch = DEFAULT_NOTCH;
    c.separation_notch = DEFAULT_NOTCH;
    c.alignment_notch = DEFAULT_NOTCH;
    c.vision_notch = 6;
    c.pace_notch = DEFAULT_NOTCH;
    c.avoid_notch = DEFAULT_NOTCH;
    c.palette = 0;
    c.turning_notch = DEFAULT_TURNING_NOTCH;
    c.flocks = 1;
    c.trails = false;
    c.hawks = 0;
    w.settings.matrix_mode = false;
    w.settings.unlock_fps = false;
    sim.apply_notches();
}

/// Bands only: the panel has its own tests.
fn set_test_screen(sim: &mut Sim, width: i32, height: i32) {
    sim.screen.width = width;
    sim.screen.height = height;
    sim.screen.legend_width = 0;
    sim.screen.legend_height = 0;
    sim.update_turn_distances();
}

fn angle_difference(a: f64, b: f64) -> f64 {
    (a - b).sin().atan2((a - b).cos()).abs()
}

/// `feed_input`: one read, as the C's pipe gives `handle_input`.
fn feed_input(w: &mut World, keys: &[u8]) -> bool {
    let taken = &keys[..keys.len().min(INPUT_BUFFER_SIZE)];
    w.sim.handle_input(&mut w.live.parser, Some(taken))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Filled cells of one slider row as drawn.
fn filled_cells(line: &[u8]) -> usize {
    line.windows(3).filter(|w| *w == "\u{2593}".as_bytes()).count()
}

/// Cells, not bytes: every glyph the panel uses is one cell wide.
fn legend_cells(line: &[u8]) -> usize {
    line.iter().filter(|&&b| (b & 0xc0) != 0x80).count()
}

/// The forbidden rectangle, written out rather than borrowed.
fn sprite_overlaps_legend(sim: &Sim, x: f64, y: f64) -> bool {
    sim.screen.legend_width > 0
        && x < f64::from(sim.screen.legend_width)
        && y < f64::from(sim.screen.legend_height)
}

fn grid_for(sim: &Sim, capacity: usize) -> SpatialGrid {
    let mut grid = SpatialGrid::new(SPATIAL_CELL_SIZE).expect("init");
    grid.prepare(sim.screen.width, sim.screen.height, capacity as i32).expect("prepare");
    grid
}

fn build(grid: &mut SpatialGrid, birds: &[Bird]) {
    grid.build(birds.len() as i32, |i| (birds[i].x, birds[i].y)).expect("build");
}

fn bird(x: f64, y: f64, direction: f64) -> Bird {
    Bird { x, y, direction, ..Bird::default() }
}

fn legend(w: &World) -> Vec<Vec<u8>> {
    build_legend(&w.sim, &w.renderer.stats)
}

/// The model written a second time the obvious way; it deliberately knows
/// nothing of the pointer, the hawks or the wind.
fn brute_force_flock_direction(sim: &Sim, birds: &[Bird], target_index: usize) -> f64 {
    let config = &sim.config;
    let target = &birds[target_index];
    let (mut sep, mut ali, mut coh, mut wary) = ((0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let boundary = sim.boundary_vector(target);
    let leash = sim.leash_vector(target);
    let (mut neighbors, mut strangers, mut kin) = (0, 0, 0.0);
    for (i, other) in birds[..config.birds as usize].iter().enumerate() {
        if i == target_index {
            continue;
        }
        let dx = target.x - other.x;
        let dy = target.y - other.y;
        if dx * dx + dy * dy >= f64::from(config.vision_radius_squared) {
            continue;
        }
        sep.0 += dx;
        sep.1 += dy;
        neighbors += 1;
        if other.flock != target.flock {
            if config.avoid_kinship > 0.0 {
                let heading = trig_lookup(other.direction);
                ali.0 += config.avoid_kinship * f64::from(heading.cosine);
                ali.1 += config.avoid_kinship * f64::from(heading.sine);
                coh.0 += config.avoid_kinship * other.x;
                coh.1 += config.avoid_kinship * other.y;
                kin += config.avoid_kinship;
            }
            let distance = (dx * dx + dy * dy).sqrt();
            if config.avoid_weight > 0.0 && distance > 1e-9 {
                wary.0 += (1.0 - distance / f64::from(config.vision_radius)) * dx / distance;
                wary.1 += (1.0 - distance / f64::from(config.vision_radius)) * dy / distance;
                strangers += 1;
            }
            continue;
        }
        let heading = trig_lookup(other.direction);
        ali.0 += f64::from(heading.cosine);
        ali.1 += f64::from(heading.sine);
        coh.0 += other.x;
        coh.1 += other.y;
        kin += 1.0;
    }
    if neighbors != 0 {
        if kin != 0.0 {
            ali = (ali.0 / kin, ali.1 / kin);
            coh = (coh.0 / kin - target.x, coh.1 / kin - target.y);
        }
        if strangers != 0 {
            wary = (wary.0 / f64::from(strangers), wary.1 / f64::from(strangers));
        }
        let x = sep.0 * config.separation
            + ali.0 * config.alignment
            + coh.0 * COHESION_W
            + boundary.x * config.boundary
            + leash.x * LEASH_WEIGHT
            + wary.0 * config.avoid_weight;
        let y = sep.1 * config.separation
            + ali.1 * config.alignment
            + coh.1 * COHESION_W
            + boundary.y * config.boundary
            + leash.y * LEASH_WEIGHT
            + wary.1 * config.avoid_weight;
        return if x == 0.0 && y == 0.0 { target.direction } else { normalized_angle(y, x) };
    }
    let bx = boundary.x * config.boundary + leash.x * LEASH_WEIGHT;
    let by = boundary.y * config.boundary + leash.y * LEASH_WEIGHT;
    if bx != 0.0 || by != 0.0 {
        let x = target.direction.cos() + bx;
        let y = target.direction.sin() + by;
        if x != 0.0 || y != 0.0 {
            return normalized_angle(y, x);
        }
    }
    target.direction
}

fn test_random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1103515245).wrapping_add(12345);
    *state
}

fn initialize_test_birds(count: usize) -> Vec<Bird> {
    let mut state: u32 = 0x93d765b1;
    let mut birds = vec![Bird::default(); count];
    for bird in birds.iter_mut() {
        bird.x = f64::from(test_random(&mut state) % 7600) / 10.0 - 60.0;
        bird.y = f64::from(test_random(&mut state) % 5000) / 10.0 - 50.0;
        bird.direction = f64::from(test_random(&mut state) % 3600) * PI / 1800.0;
        bird.frame = direction_frame(bird.direction);
    }
    birds[0] = bird(12.0, 12.0, 0.0);
    birds[1] = bird(24.0, 12.0, PI / 2.0);
    birds[2] = bird(36.0, 36.0, PI);
    birds[3] = bird(-1.0, 20.0, PI / 4.0);
    birds[4] = bird(641.0, 20.0, 3.0 * PI / 2.0);
    birds
}

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

/// However the program ends, the terminal is put back, and a reader that goes
/// away is one of the ways. The whole program runs on a terminal of its own,
/// writing into a pipe that is closed under it after its first byte.
#[test]
fn test_a_closed_pipe_leaves_the_terminal_as_it_was() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let _w = enter!("test_a_closed_pipe_leaves_the_terminal_as_it_was");
    let size = rbirds::platform::WinSize { row: 24, col: 80, xpixel: 0, ypixel: 0 };
    let pty = rbirds::platform::pty::open_pty(&size).expect("a pty");
    let before = rbirds::platform::tcgetattr(std::os::fd::AsRawFd::as_raw_fd(&pty.slave))
        .expect("attributes");
    let (mut reader, writer) = std::io::pipe().expect("a pipe");
    let mut child = Command::new(support::oracle::rust_binary())
        .args(["--render", "braille", "-n", "50"])
        .stdin(Stdio::from(pty.slave.try_clone().expect("dup")))
        .stdout(Stdio::from(writer))
        .stderr(Stdio::null())
        .spawn()
        .expect("run rbirds");
    let mut byte = [0_u8; 1];
    assert_eq!(reader.read(&mut byte).expect("first byte"), 1);
    drop(reader);
    // A hang fails the test rather than stalling the suite.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("rbirds did not exit after its reader went away");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    // An error it reports and exits on, not a signal it dies of.
    assert_eq!(status.code(), Some(1), "{status:?}");
    let after = rbirds::platform::tcgetattr(std::os::fd::AsRawFd::as_raw_fd(&pty.slave))
        .expect("attributes");
    assert_eq!(after, before);
}

#[test]
fn test_the_trig_lookup_covers_the_circle() {
    let _w = enter!("test_the_trig_lookup_covers_the_circle");
    let delta = 2.0 * PI / f64::from(TRIG_LOOKUP_SIZE);
    let tolerance = delta / 2.0 + 1e-6;

    let zero = trig_lookup(0.0);
    let quarter = trig_lookup(PI / 2.0);
    let half = trig_lookup(PI);
    let three_quarters = trig_lookup(3.0 * PI / 2.0);
    assert!((zero.cosine - 1.0).abs() < 1e-6 && zero.sine.abs() < 1e-6);
    assert!(quarter.cosine.abs() < 1e-6 && (quarter.sine - 1.0).abs() < 1e-6);
    assert!((half.cosine + 1.0).abs() < 1e-6 && half.sine.abs() < 1e-6);
    assert!(three_quarters.cosine.abs() < 1e-6 && (three_quarters.sine + 1.0).abs() < 1e-6);

    // The mask wraps in either direction, including the exact seam.
    let full_turn = trig_lookup(2.0 * PI);
    assert!(full_turn.cosine == zero.cosine && full_turn.sine == zero.sine);
    let before_zero = trig_lookup(-delta);
    let before_full_turn = trig_lookup(2.0 * PI - delta);
    assert_eq!(before_zero.cosine, before_full_turn.cosine);
    assert_eq!(before_zero.sine, before_full_turn.sine);

    for i in -TRIG_LOOKUP_SIZE * 2..=TRIG_LOOKUP_SIZE * 2 {
        let angle = f64::from(i) * delta / 7.0;
        let got = trig_lookup(angle);
        let got_angle = f64::from(got.sine).atan2(f64::from(got.cosine));
        assert!(angle_difference(got_angle, angle) <= tolerance);
    }
}

#[test]
fn test_the_frame_rate_can_be_unlocked() {
    let mut w = enter!("test_the_frame_rate_can_be_unlocked");
    reset_test_config(&mut w);
    let budget = 1_000_000 / i64::from(FRAME_RATE);
    assert_eq!(frame_delay_after(w.settings.unlock_fps, 1000), budget - 1000);
    assert_eq!(frame_delay_after(w.settings.unlock_fps, budget), 0);
    let mut program = rbirds::app::Program {
        sim: w.sim.clone(),
        settings: w.settings.clone(),
        renderer: Default::default(),
    };
    let parsed =
        rbirds::options::parse(&OPTIONS, &mut program, &args(&["cbirds", "--unlock-fps"]), 128);
    assert_eq!(parsed.status, rbirds::options::Status::Ok);
    assert!(program.settings.unlock_fps);
    assert_eq!(frame_delay_after(program.settings.unlock_fps, 0), 0);
    assert_eq!(frame_delay_after(program.settings.unlock_fps, 1000), 0);
}

#[test]
fn test_engine_matches_brute_force() {
    let mut w = enter!("test_engine_matches_brute_force");
    const COUNT: usize = 256;
    set_test_screen(&mut w.sim, 640, 384);
    w.sim.config.birds = COUNT as i32;
    w.sim.config.speed = 0.75;
    w.sim.config.separation = 0.005;
    w.sim.config.alignment = 1.5;
    w.sim.config.boundary = 0.2;
    let mut snapshot = initialize_test_birds(COUNT);
    let mut grid = grid_for(&w.sim, COUNT);
    build(&mut grid, &snapshot);

    // The three terms the reference does not model must all be quiet.
    assert_eq!(w.sim.config.hawks, 0);
    assert!(!w.sim.mouse.present);
    assert!(!w.sim.rain);

    w.sim.config.flocks = 1;
    while w.sim.config.flocks <= MAX_FLOCKS {
        for (i, b) in snapshot.iter_mut().enumerate() {
            b.flock = i as i32 % w.sim.config.flocks;
        }
        for avoidance in [0, 2, DEFAULT_NOTCH, 8, LEGEND_BAR_CELLS] {
            w.sim.config.avoid_notch = avoidance;
            w.sim.apply_notches();
            w.sim.measure_flocks(&snapshot);
            w.sim.config.vision_notch = 0;
            while w.sim.config.vision_notch <= LEGEND_BAR_CELLS {
                w.sim.apply_notches();
                for i in 0..COUNT {
                    let expected = brute_force_flock_direction(&w.sim, &snapshot, i);
                    let actual = w.sim.flock_direction(&snapshot, &grid, i);
                    assert!(angle_difference(expected, actual) < 1e-11);
                }
                w.sim.config.vision_notch += 1;
            }
        }
        w.sim.config.flocks += 1;
    }
    w.sim.config.avoid_notch = DEFAULT_NOTCH;
    w.sim.config.flocks = 1;
    for b in snapshot.iter_mut() {
        b.flock = 0;
    }

    w.sim.config.vision_notch = 6;
    w.sim.apply_notches();
    let mut optimized = snapshot.clone();
    let mut reference = snapshot.clone();
    w.sim.update_birds(&mut optimized, &snapshot, &grid);
    for i in 0..COUNT {
        let direction = turn_towards(
            snapshot[i].direction,
            brute_force_flock_direction(&w.sim, &snapshot, i),
            w.sim.turn_limit(),
        );
        reference[i].direction = direction;
        reference[i].x += w.sim.config.speed * direction.cos();
        reference[i].y += w.sim.config.speed * direction.sin();
        optimized[i].frame = direction_frame(optimized[i].direction);
        reference[i].frame = direction_frame(reference[i].direction);
        assert!(angle_difference(optimized[i].direction, reference[i].direction) < 1e-11);
        assert!((optimized[i].x - reference[i].x).abs() < 1e-11);
        assert!((optimized[i].y - reference[i].y).abs() < 1e-11);
        assert_eq!(optimized[i].frame, reference[i].frame);
    }
}

#[test]
fn test_boundary_bands_follow_the_viewport() {
    let mut w = enter!("test_boundary_bands_follow_the_viewport");
    set_test_screen(&mut w.sim, 900, 600);
    assert_eq!(w.sim.screen.turn_x, 300);
    assert_eq!(w.sim.screen.turn_y, 200);
    assert_eq!(w.sim.screen.turn_bottom, 100);

    let force = |x: f64, y: f64| w.sim.boundary_vector(&bird(x, y, 0.0));
    let left = force(299.0, 300.0);
    let right = force(601.0, 300.0);
    let top = force(450.0, 199.0);
    let bottom = force(450.0, 501.0);
    let below_top = force(450.0, 201.0);
    let above_bottom = force(450.0, 499.0);
    let center = force(450.0, 300.0);

    assert!(left.x > 0.0 && left.y == 0.0);
    assert!(right.x == -left.x && right.y == 0.0);
    assert!(top.x == 0.0 && top.y > 0.0);
    assert!(bottom.x == 0.0 && bottom.y < 0.0);
    assert!(-bottom.y > top.y);
    assert!(below_top.x == 0.0 && below_top.y == 0.0);
    assert!(above_bottom.x == 0.0 && above_bottom.y == 0.0);
    assert!(center.x == 0.0 && center.y == 0.0);
}

#[test]
fn test_the_edge_pushes_harder_the_further_out_a_bird_is() {
    let mut w = enter!("test_the_edge_pushes_harder_the_further_out_a_bird_is");
    set_test_screen(&mut w.sim, 900, 600);

    let mut previous = 0.0;
    for x in (-200..=299).rev().step_by(10) {
        let push = w.sim.boundary_vector(&bird(f64::from(x), 300.0, 0.0)).x;
        assert!(push > previous);
        previous = push;
    }
    previous = 0.0;
    for x in (601..=1100).step_by(10) {
        let push = -w.sim.boundary_vector(&bird(f64::from(x), 300.0, 0.0)).x;
        assert!(push > previous);
        previous = push;
    }
    previous = 0.0;
    for y in (-200..=199).rev().step_by(10) {
        let push = w.sim.boundary_vector(&bird(450.0, f64::from(y), 0.0)).y;
        assert!(push > previous);
        previous = push;
    }
    previous = 0.0;
    for y in (501..=900).step_by(10) {
        let push = -w.sim.boundary_vector(&bird(450.0, f64::from(y), 0.0)).y;
        assert!(push > previous);
        previous = push;
    }

    // No step at the screen's own edge, at any notch.
    for notch in 0..=LEGEND_BAR_CELLS {
        w.sim.config.boundary_notch = notch;
        w.sim.apply_notches();
        let inside = w.sim.boundary_vector(&bird(0.001, 300.0, 0.0)).x * w.sim.config.boundary;
        let outside = w.sim.boundary_vector(&bird(-0.001, 300.0, 0.0)).x * w.sim.config.boundary;
        assert!((outside - inside).abs() < 1e-3);
    }

    let gone = bird(-100.0, 300.0, 0.0);
    let leaving = bird(0.0, 300.0, 0.0);
    w.sim.config.boundary_notch = 0;
    w.sim.apply_notches();
    let softest = w.sim.boundary_vector(&gone).x * w.sim.config.boundary;
    let soft_edge = w.sim.boundary_vector(&leaving).x * w.sim.config.boundary;
    w.sim.config.boundary_notch = LEGEND_BAR_CELLS;
    w.sim.apply_notches();
    let firmest = w.sim.boundary_vector(&gone).x * w.sim.config.boundary;
    let firm_edge = w.sim.boundary_vector(&leaving).x * w.sim.config.boundary;
    assert!(softest > firmest * 0.8);
    assert!(softest > EDGE_FIRM);
    assert!(soft_edge < firm_edge / 10.0);
}

#[test]
fn test_bottom_band_scales_on_a_short_viewport() {
    let mut w = enter!("test_bottom_band_scales_on_a_short_viewport");
    set_test_screen(&mut w.sim, 640, 96);
    assert_eq!(w.sim.screen.turn_y, 32);
    assert_eq!(w.sim.screen.turn_bottom, 16);
    assert!(w.sim.screen.turn_y < w.sim.screen.height - w.sim.screen.turn_bottom);
    for y in w.sim.screen.turn_y..=w.sim.screen.height - w.sim.screen.turn_bottom {
        let force = w.sim.boundary_vector(&bird(320.0, f64::from(y), 0.0));
        assert!(force.x == 0.0 && force.y == 0.0);
    }
    assert!(w.sim.boundary_vector(&bird(320.0, 95.0, 0.0)).y < 0.0);
    assert!(w.sim.boundary_vector(&bird(320.0, 1.0, 0.0)).y > 0.0);
}

#[test]
fn test_birds_start_spread_inside_the_free_region() {
    let mut w = enter!("test_birds_start_spread_inside_the_free_region");
    const COUNT: usize = 512;
    let mut birds = vec![Bird::default(); COUNT];
    set_test_screen(&mut w.sim, 900, 600);
    w.sim.config.birds = COUNT as i32;
    w.sim.rng.seed(20260911);
    w.sim.initialize_birds(&mut birds);
    let mut distinct = 0;
    for b in &birds {
        let force = w.sim.boundary_vector(b);
        assert!(force.x == 0.0 && force.y == 0.0);
        assert_eq!(b.frame, direction_frame(b.direction));
        if b.x != birds[0].x || b.y != birds[0].y {
            distinct += 1;
        }
    }
    assert!(distinct > COUNT * 3 / 4);
}

#[test]
fn test_a_grown_flock_starts_its_new_birds_clean() {
    let mut w = enter!("test_a_grown_flock_starts_its_new_birds_clean");
    const FEW: i32 = 8;
    const MANY: i32 = 64;
    reset_test_config(&mut w);
    set_test_screen(&mut w.sim, 900, 600);
    w.sim.config.trails = true;
    w.sim.rng.seed(11);

    // Whatever the memory held, a placed bird starts from nothing.
    let mut poisoned = Bird {
        x: f64::from_bits(0xa5a5a5a5a5a5a5a5),
        trail_at: -1515870811,
        trail_held: -1515870811,
        gliding: f64::from_bits(0xa5a5a5a5a5a5a5a5),
        wing: -1515870811,
        ..Bird::default()
    };
    w.sim.place_one_bird(&mut poisoned, 0);
    assert!(poisoned.trail_at == 0 && poisoned.trail_held == 0 && poisoned.gliding == 0.0);

    w.sim.config.birds = FEW;
    let mut birds = vec![Bird::default(); FEW as usize];
    let mut snapshot = vec![Bird::default(); FEW as usize];
    w.sim.initialize_birds(&mut birds);
    let first = birds[0];

    w.sim.config.birds = MANY;
    assert!(w.sim.resize_the_flock(&mut birds, &mut snapshot, FEW, MANY));
    assert_eq!(birds[0], first);
    for b in &birds[FEW as usize..MANY as usize] {
        assert!(b.trail_at == 0 && b.trail_held == 0 && b.wing < WING_CYCLE);
    }

    let mut grid = grid_for(&w.sim, MANY as usize);
    for _ in 0..2 * TRAIL_LENGTH {
        snapshot.copy_from_slice(&birds);
        build(&mut grid, &snapshot);
        w.sim.update_birds(&mut birds, &snapshot, &grid);
    }
    for b in birds.iter().step_by(TRAIL_EVERY as usize) {
        assert!(b.trail_held == TRAIL_LENGTH && b.trail_at < TRAIL_LENGTH);
    }

    let first = birds[0];
    w.sim.config.birds = FEW / 2;
    assert!(w.sim.resize_the_flock(&mut birds, &mut snapshot, MANY, FEW / 2));
    assert_eq!(birds[0], first);
}

#[test]
fn test_the_recording_rate_is_one_a_gif_has() {
    let _w = enter!("test_the_recording_rate_is_one_a_gif_has");
    assert_eq!(record_delay_for(50), 2);
    assert_eq!(record_delay_for(25), 4);
    assert_eq!(record_delay_for(20), 5);
    assert_eq!(record_delay_for(10), 10);
    assert_eq!(record_delay_for(2), 50);
    assert_eq!(record_delay_for(60), 2);
    assert_eq!(record_delay_for(120), 2);
    assert_eq!(100 / record_delay_for(60), MAX_RECORD_FPS);
    for fps in 2..=120 {
        let delay = record_delay_for(fps);
        assert!(delay >= 2);
        assert!(100 / delay <= MAX_RECORD_FPS);
        let got = 100.0 / f64::from(delay);
        for other in 2..=100 {
            let mine = (got - f64::from(fps)).abs();
            let theirs = (100.0 / f64::from(other) - f64::from(fps)).abs();
            assert!(mine <= theirs + 1e-9);
        }
    }
}

#[test]
fn test_birds_bank_rather_than_snap() {
    let mut w = enter!("test_birds_bank_rather_than_snap");
    reset_test_config(&mut w);
    w.sim.config.turning_notch = DEFAULT_TURNING_NOTCH;
    let most = w.sim.turn_limit();
    assert!(most > 0.0 && most < 2.0 * PI);

    let turned = turn_towards(0.0, PI, most);
    assert!(angle_difference(turned, most) < 1e-12);

    let from_high = turn_towards(2.0 * PI - 0.05, 0.05, most);
    assert!((0.0..2.0 * PI).contains(&from_high));
    assert!(angle_difference(from_high, 0.05) < 1e-12);
    let clockwise = turn_towards(0.05, 2.0 * PI - 0.05, most);
    assert!(angle_difference(clockwise, 2.0 * PI - 0.05) < 1e-12);

    assert!(angle_difference(turn_towards(1.0, 1.0 + most / 2.0, most), 1.0 + most / 2.0) < 1e-12);

    for a in (0..360).step_by(7) {
        for b in (0..360).step_by(11) {
            let from = f64::from(a) * PI / 180.0;
            let got = turn_towards(from, f64::from(b) * PI / 180.0, most);
            assert!((0.0..2.0 * PI).contains(&got));
            assert!(angle_difference(got, from) <= most + 1e-12);
        }
    }

    w.sim.config.turning_notch = LEGEND_BAR_CELLS;
    assert!(angle_difference(turn_towards(0.0, PI, w.sim.turn_limit()), PI) < 1e-12);
    w.sim.config.turning_notch = 0;
    assert!(w.sim.turn_limit() > PI / 8.0);
    assert!(w.sim.turn_limit() < PI / 4.0);
    assert!(turn_towards(1.0, 3.0, w.sim.turn_limit()) > 1.0);
    assert!(turn_towards(1.0, 3.0, w.sim.turn_limit()) < 1.0 + PI / 4.0);
    let mut previous = w.sim.turn_limit();
    for notch in 1..LEGEND_BAR_CELLS {
        w.sim.config.turning_notch = notch;
        assert!(w.sim.turn_limit() > previous);
        previous = w.sim.turn_limit();
    }

    // Writing is exempt from the limit, or a bird could not land on a letter.
    w.sim.legend_enabled = true;
    w.sim.apply_screen_size(200, 50, 1600, 800);
    w.sim.config.turning_notch = 0;
    assert!(w.sim.formation_layout(b"I") > 0);
    const COUNT: usize = 4;
    w.sim.config.birds = COUNT as i32;
    let start = bird(w.sim.formation.x[0] - w.sim.config.speed / 2.0, w.sim.formation.y[0], PI);
    let mut birds = vec![start; COUNT];
    let mut grid = grid_for(&w.sim, COUNT);
    let snapshot = birds.clone();
    build(&mut grid, &snapshot);
    w.sim.update_birds(&mut birds, &snapshot, &grid);
    assert!((birds[0].x - w.sim.formation.x[0]).abs() < 1e-9);
}

#[test]
fn test_the_konami_code() {
    let mut w = enter!("test_the_konami_code");
    reset_test_config(&mut w);
    w.sim.legend_enabled = true;
    w.sim.apply_screen_size(200, 50, 1600, 800);
    w.sim.config.hawks = 0;
    w.sim.konami.at = 0;
    w.sim.formation.clear();

    for &c in b"AABBDCDCb" {
        w.sim.konami_note(c);
    }
    assert_eq!(w.sim.config.hawks, 0);
    w.sim.konami_note(b'a');
    assert_eq!(w.sim.config.hawks, MAX_HAWKS);

    w.sim.config.hawks = 0;
    w.sim.formation.clear();
    w.sim.konami.at = 0;
    w.sim.konami.seen = [0; KONAMI_LENGTH];
    for &c in b"AABBDCDXCba" {
        w.sim.konami_note(c);
    }
    assert_eq!(w.sim.config.hawks, 0);

    for &c in b"AAABBDCDCba" {
        w.sim.konami_note(c);
    }
    assert_eq!(w.sim.config.hawks, MAX_HAWKS);

    w.sim.config.hawks = 0;
    w.sim.formation.clear();
    for &c in b"qwertyAABBDCDCba" {
        w.sim.konami_note(c);
    }
    assert_eq!(w.sim.config.hawks, MAX_HAWKS);
}

#[test]
fn test_only_the_rain_has_a_wind() {
    let mut w = enter!("test_only_the_rain_has_a_wind");
    reset_test_config(&mut w);
    assert!(!w.sim.rain);
    assert!(w.sim.wind_vector().x == 0.0 && w.sim.wind_vector().y == 0.0);
    w.sim.rain = true;
    let falling = w.sim.wind_vector();
    assert!(falling.y > 0.0);
    assert_eq!(falling.x, 0.0);
}

#[test]
fn test_autopilot_wanders_and_yields() {
    let mut w = enter!("test_autopilot_wanders_and_yields");
    reset_test_config(&mut w);
    w.sim.last_key_at = 0.0;
    w.sim.clock.seconds = 0.0;

    assert!(!w.sim.flying_itself());
    w.sim.clock.seconds = f64::from(IDLE_SECONDS + 1);
    assert!(w.sim.flying_itself());
    w.sim.clock.seconds = 0.0;
    assert!(!w.sim.flying_itself());

    w.sim.rng.seed(7);
    for _ in 0..400 {
        let c = &w.sim.config;
        let before = [c.boundary_notch, c.separation_notch, c.alignment_notch, c.vision_notch];
        w.sim.drift_a_slider();
        let c = &w.sim.config;
        let after = [c.boundary_notch, c.separation_notch, c.alignment_notch, c.vision_notch];
        let mut moved = 0;
        for i in 0..4 {
            assert!((0..=LEGEND_BAR_CELLS).contains(&after[i]));
            if after[i] != before[i] {
                assert!((after[i] - before[i]).abs() == 1);
                moved += 1;
            }
        }
        assert_eq!(moved, 1);
    }

    w.sim.clock.seconds = 100.0;
    w.sim.last_key_at = 100.0;
    w.sim.last_drift_at = 0.0;
    let held = w.sim.config.boundary_notch;
    w.sim.maybe_drift();
    assert_eq!(w.sim.config.boundary_notch, held);
    w.sim.clock.seconds = f64::from(100 + IDLE_SECONDS);
    w.sim.maybe_drift();
    assert_eq!(w.sim.last_drift_at, w.sim.clock.seconds);
    w.sim.clock.seconds += f64::from(AUTOPILOT_PERIOD - 1);
    w.sim.maybe_drift();
    assert_eq!(w.sim.last_drift_at, f64::from(100 + IDLE_SECONDS));
    w.sim.clock.seconds += 1.0;
    w.sim.maybe_drift();
    assert_eq!(w.sim.last_drift_at, w.sim.clock.seconds);
}

#[test]
fn test_hawks_hunt_and_the_flock_flees() {
    let mut w = enter!("test_hawks_hunt_and_the_flock_flees");
    const COUNT: usize = 20;
    reset_test_config(&mut w);
    w.sim.legend_enabled = false;
    w.sim.apply_screen_size(200, 50, 1600, 800);
    w.sim.config.birds = COUNT as i32;
    w.sim.config.hawks = 1;
    w.sim.place_hawks();

    {
        let h = &mut w.sim.hawks[0];
        h.x = 800.0;
        h.y = 400.0;
        h.direction = PI;
        h.prey = -1;
        h.commitment = 0.0;
        h.passing = 0.0;
    }
    let mut birds: Vec<Bird> = (0..COUNT)
        .map(|i| {
            bird(1100.0 + (i % 5) as f64 * 20.0, 340.0 + (i / 5) as f64 * 30.0, i as f64 * 0.3)
        })
        .collect();

    let before = w.sim.hawks[0].direction;
    w.sim.hunt(&birds);
    let turned = angle_difference(w.sim.hawks[0].direction, before);
    assert!(turned > 0.0);
    assert!(turned <= HAWK_TURN + 1e-9);
    let first = w.sim.hawks[0].prey;
    assert!(first >= 0);

    let mut grid = grid_for(&w.sim, COUNT);
    let gap_before = Sim::distance_to_bird(&birds, &w.sim.hawks[0], first);
    let (mut held, mut struck, mut struck_at) = (first, -1, 0.0);
    let mut moving = birds.clone();
    for _ in 0..60 {
        if struck >= 0 {
            break;
        }
        let snapshot = moving.clone();
        build(&mut grid, &snapshot);
        w.sim.update_birds(&mut moving, &snapshot, &grid);
        let before = w.sim.hawks[0].prey;
        let reach = if before >= 0 {
            w.sim.reach_along_the_step(&snapshot, &w.sim.hawks[0], before)
        } else {
            0.0
        };
        w.sim.hunt(&snapshot);
        if w.sim.hawks[0].passing > 0.0 && before >= 0 {
            struck = before;
            struck_at = reach;
        } else if w.sim.hawks[0].prey >= 0 {
            held = w.sim.hawks[0].prey;
        }
    }
    assert!(struck >= 0);
    assert_eq!(struck, held);
    assert!(struck_at < f64::from(w.sim.config.bird_size * 2));
    assert!(Sim::distance_to_bird(&moving, &w.sim.hawks[0], struck) < gap_before);

    // The commitment holds even against a bird put in its path.
    {
        let h = &mut w.sim.hawks[0];
        h.x = 400.0;
        h.y = 400.0;
        h.passing = 0.0;
        h.prey = -1;
        h.commitment = 0.0;
    }
    w.sim.hunt(&birds);
    let chosen = w.sim.hawks[0].prey;
    assert!(chosen >= 0);
    let bait = ((chosen + 1) % COUNT as i32) as usize;
    w.sim.hawks[0].commitment = f64::from(HAWK_COMMITMENT_FRAMES) / f64::from(FRAME_RATE);
    birds[bait].x = w.sim.hawks[0].x + 10.0;
    birds[bait].y = w.sim.hawks[0].y + 10.0;
    w.sim.hunt(&birds);
    assert!(w.sim.hawks[0].prey == chosen || w.sim.hawks[0].passing > 0.0);

    // Spent, it drops an outrun bird and picks from a distance.
    w.sim.hawks[0].passing = 0.0;
    w.sim.hawks[0].prey = chosen;
    w.sim.hawks[0].commitment = 0.0;
    birds[chosen as usize].x = w.sim.hawks[0].x + f64::from(HAWK_GIVE_UP) + 40.0;
    birds[chosen as usize].y = w.sim.hawks[0].y;
    birds[bait].x = w.sim.hawks[0].x + 80.0;
    birds[bait].y = w.sim.hawks[0].y;
    {
        let before_move = w.sim.hawks[0];
        w.sim.hunt(&birds);
        let picked_from = w.sim.hawks[0].prey;
        assert!(picked_from >= 0);
        assert!(Sim::distance_to_bird(&birds, &before_move, picked_from) >= f64::from(HAWK_STALK));
    }
    assert_ne!(w.sim.hawks[0].prey, chosen);
    assert_ne!(w.sim.hawks[0].prey, bait as i32);

    // Not enough birds to go round: they share.
    {
        let flock_was = w.sim.config.birds;
        w.sim.config.birds = 2;
        w.sim.config.hawks = 4;
        w.sim.place_hawks();
        for h in &mut w.sim.hawks[..4] {
            h.prey = -1;
            h.commitment = 0.0;
            h.passing = 0.0;
        }
        w.sim.hunt(&birds);
        for h in &w.sim.hawks[..4] {
            assert!(h.prey >= 0);
        }
        w.sim.config.birds = flock_was;
        w.sim.config.hawks = 1;
    }

    // Two hawks never share a bird.
    w.sim.config.hawks = 2;
    w.sim.place_hawks();
    for h in &mut w.sim.hawks[..2] {
        h.x = 400.0;
        h.y = 400.0;
        h.direction = 0.0;
        h.prey = -1;
        h.commitment = 0.0;
        h.passing = 0.0;
    }
    w.sim.hunt(&birds);
    assert!(w.sim.hawks[0].prey >= 0 && w.sim.hawks[1].prey >= 0);
    assert_ne!(w.sim.hawks[0].prey, w.sim.hawks[1].prey);
    w.sim.hawks[0].x = 400.0;
    w.sim.hawks[1].x = 400.0;
    w.sim.hawks[0].y = 380.0;
    w.sim.hawks[1].y = 420.0;
    assert!(w.sim.hawk_spacing(0).y < 0.0);
    assert_eq!(w.sim.hawk_spacing(1).y, -w.sim.hawk_spacing(0).y);
    w.sim.hawks[1].y = 380.0 + f64::from(HAWK_SPACING);
    assert!(w.sim.hawk_spacing(0).x == 0.0 && w.sim.hawk_spacing(0).y == 0.0);
    w.sim.config.hawks = 1;
    assert!(w.sim.hawk_spacing(0).x == 0.0 && w.sim.hawk_spacing(0).y == 0.0);

    // Flying among them starts a pass, straight out the other side.
    w.sim.hawks[0].x = birds[0].x;
    w.sim.hawks[0].y = birds[0].y;
    w.sim.hawks[0].passing = 0.0;
    w.sim.hawks[0].prey = 0;
    w.sim.hunt(&birds);
    assert!(w.sim.hawks[0].passing > 0.0);
    assert!(w.sim.hawks[0].prey < 0);
    let heading = w.sim.hawks[0].direction;
    w.sim.hunt(&birds);
    assert!(angle_difference(w.sim.hawks[0].direction, heading) < 1e-12);

    // Turned back at a wall, whole silhouette still on the screen.
    {
        let width = f64::from(w.sim.screen.width);
        let h = &mut w.sim.hawks[0];
        h.x = width - 1.0;
        h.y = 400.0;
        h.direction = 0.0;
        h.prey = -1;
        h.commitment = 0.5;
        h.passing = 0.5;
    }
    w.sim.hunt(&birds);
    let last = f64::from(w.sim.screen.width - 1 - w.sim.hawk_draw_offset());
    assert!(w.sim.hawks[0].x <= last);
    assert!(w.sim.hawks[0].direction.cos() < 0.0);
    w.sim.hunt(&birds);
    assert!(w.sim.hawks[0].x < last);

    // A wall is seen a turning circle off, and the panel is a wall too.
    w.sim.config.hawks = 1;
    w.sim.legend_enabled = true;
    w.sim.measure_legend();
    w.sim.update_turn_distances();
    let near_left = Hawk { x: 2.0, y: f64::from(w.sim.screen.height) / 2.0, ..Hawk::default() };
    let near_bottom = Hawk {
        x: f64::from(w.sim.screen.width) / 2.0,
        y: f64::from(w.sim.screen.height - 2),
        ..Hawk::default()
    };
    assert!(w.sim.screen.legend_width > 0);
    assert!(w.sim.hawk_wall_vector(&near_left).x > 0.0);
    assert!(w.sim.hawk_wall_vector(&near_bottom).y < 0.0);
    assert!(w.sim.hawk_wall_band() > w.sim.hawk_turning_radius());
    let band = w.sim.hawk_wall_band();
    let beside_panel = Hawk {
        x: f64::from(w.sim.screen.legend_width) + band / 2.0,
        y: band + 10.0,
        ..Hawk::default()
    };
    assert!(beside_panel.x > band);
    assert!(beside_panel.y < f64::from(w.sim.screen.legend_height) + band);
    let wall = w.sim.hawk_wall_vector(&beside_panel);
    assert!(wall.x > 0.0 || wall.y > 0.0);
    w.sim.legend_enabled = false;
    w.sim.measure_legend();
    w.sim.update_turn_distances();
    let wall = w.sim.hawk_wall_vector(&beside_panel);
    assert!(wall.x == 0.0 && wall.y == 0.0);

    // A hawk's alarm carries less far on a small screen.
    w.sim.apply_screen_size(
        LEGEND_MIN_COLS,
        LEGEND_MIN_ROWS,
        LEGEND_MIN_COLS * 8,
        LEGEND_MIN_ROWS * 16,
    );
    assert!(w.sim.hawk_reach() < f64::from(HAWK_REACH));
    assert!((w.sim.hawk_reach() - f64::from(w.sim.screen.height) / 3.0).abs() < 1e-9);
    w.sim.apply_screen_size(200, 50, 1600, 800);
    assert_eq!(w.sim.hawk_reach(), f64::from(HAWK_REACH));

    // Over a long run the reflection at the wall stays rare.
    w.sim.config.hawks = 2;
    w.sim.place_hawks();
    let mut grid = grid_for(&w.sim, COUNT);
    for (i, b) in birds.iter_mut().enumerate() {
        *b = bird(400.0 + (i % 5) as f64 * 30.0, 300.0 + (i / 5) as f64 * 30.0, i as f64 * 0.3);
    }
    let (mut reflections, steps) = (0, 600);
    let mut before_turn = [0.0; MAX_HAWKS as usize];
    for i in 0..2 {
        before_turn[i] = w.sim.hawks[i].direction;
    }
    for _ in 0..steps {
        let snapshot = birds.clone();
        build(&mut grid, &snapshot);
        w.sim.update_birds(&mut birds, &snapshot, &grid);
        w.sim.hunt(&snapshot);
        for i in 0..2 {
            if angle_difference(w.sim.hawks[i].direction, before_turn[i])
                > w.sim.hawk_turn_limit() + 1e-9
            {
                reflections += 1;
            }
            before_turn[i] = w.sim.hawks[i].direction;
        }
    }
    assert!(reflections * 25 < steps * 2);
    w.sim.config.hawks = 1;

    // Every bird flees every hawk in reach, hardest when closest.
    w.sim.hawks[0].x = 800.0;
    w.sim.hawks[0].y = 400.0;
    let close = bird(820.0, 400.0, 0.0);
    let further = bird(800.0 + f64::from(HAWK_REACH) - 20.0, 400.0, 0.0);
    let clear = bird(800.0 + f64::from(HAWK_REACH) + 1.0, 400.0, 0.0);
    assert!(w.sim.hawk_vector(&close).x > w.sim.hawk_vector(&further).x);
    assert!(w.sim.hawk_vector(&further).x > 0.0);
    assert!(w.sim.hawk_vector(&clear).x == 0.0 && w.sim.hawk_vector(&clear).y == 0.0);
    assert!(w.sim.hawk_vector(&close).y.abs() > 0.0);
    w.sim.apply_screen_size(
        LEGEND_MIN_COLS,
        LEGEND_MIN_ROWS,
        LEGEND_MIN_COLS * 8,
        LEGEND_MIN_ROWS * 16,
    );
    let (half_w, half_h) =
        (f64::from(w.sim.screen.width) / 2.0, f64::from(w.sim.screen.height) / 2.0);
    w.sim.hawks[0].x = half_w;
    w.sim.hawks[0].y = half_h;
    assert_eq!(w.sim.hawk_vector(&bird(half_w + f64::from(HAWK_REACH) - 20.0, half_h, 0.0)).x, 0.0);
    assert!(w.sim.hawk_vector(&bird(half_w + 10.0, half_h, 0.0)).x > 0.0);
    w.sim.apply_screen_size(200, 50, 1600, 800);
    w.sim.hawks[0].x = 800.0;
    w.sim.hawks[0].y = 400.0;
    w.sim.config.hawks = 0;
    assert_eq!(w.sim.hawk_vector(&close).x, 0.0);

    // A hawk overrules a flock heading into it.
    w.sim.config.hawks = 1;
    let crowd = vec![bird(830.0, 400.0, PI); COUNT];
    let mut grid = grid_for(&w.sim, COUNT);
    build(&mut grid, &crowd);
    assert!(w.sim.flock_direction(&crowd, &grid, 0).cos() > 0.0);
    birds = crowd;

    // Every hawk stays wholly on the screen, on the smallest viewport too.
    for (columns, rows) in [(200, 50), (LEGEND_MIN_COLS, LEGEND_MIN_ROWS)] {
        w.sim.apply_screen_size(columns, rows, columns * 8, rows * 16);
        w.sim.config.hawks = MAX_HAWKS;
        w.sim.place_hawks();
        let (mut least_x, mut most_x) = (f64::from(w.sim.screen.width), 0.0_f64);
        let (mut least_y, mut most_y) = (f64::from(w.sim.screen.height), 0.0_f64);
        for _ in 0..400 {
            w.sim.hunt(&birds);
            let offset = f64::from(w.sim.hawk_draw_offset());
            for h in &w.sim.hawks[..MAX_HAWKS as usize] {
                assert!(h.x >= offset && h.y >= offset);
                assert!(h.x <= f64::from(w.sim.screen.width - 1) - offset);
                assert!(h.y <= f64::from(w.sim.screen.height - 1) - offset);
                least_x = least_x.min(h.x);
                most_x = most_x.max(h.x);
                least_y = least_y.min(h.y);
                most_y = most_y.max(h.y);
            }
        }
        let middle = Hawk {
            x: f64::from(w.sim.screen.width) / 2.0,
            y: f64::from(w.sim.screen.height) / 2.0,
            ..Hawk::default()
        };
        assert_eq!(w.sim.hawk_wall_vector(&middle).x, 0.0);
        assert_eq!(w.sim.hawk_wall_vector(&middle).y, 0.0);
        assert!(most_x - least_x > f64::from(w.sim.screen.width) / 2.0);
        assert!(most_y - least_y > f64::from(w.sim.screen.height) / 2.0);
    }
    w.sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);

    // Summoning one leaves the others where they were.
    w.sim.config.hawks = 3;
    w.sim.place_hawks();
    for _ in 0..10 {
        w.sim.hunt(&birds);
    }
    let (kept_x, kept_y) = (w.sim.hawks[0].x, w.sim.hawks[0].y);
    w.sim.config.hawks = 4;
    w.sim.place_one_hawk(3);
    assert!(w.sim.hawks[0].x == kept_x && w.sim.hawks[0].y == kept_y);
}

#[test]
fn test_motion_follows_elapsed_time() {
    let mut w = enter!("test_motion_follows_elapsed_time");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.apply_screen_size(200, 60, 1600, 960);
    sim.config.turning_notch = 6;
    let rate = f64::from(FRAME_RATE);

    sim.set_frame_seconds(1.0 / rate);
    let (bird_at_sixty, hawk_at_sixty) = (sim.turn_limit(), sim.hawk_turn_limit());
    let step_at_sixty = sim.config.speed;

    sim.set_frame_seconds(2.0 / rate);
    assert!((sim.config.speed - step_at_sixty * 2.0).abs() < 1e-9);
    assert!((sim.turn_limit() - bird_at_sixty * 2.0).abs() < 1e-9);
    assert!((sim.hawk_turn_limit() - hawk_at_sixty * 2.0).abs() < 1e-9);
    assert!((sim.config.speed * rate / 2.0 - step_at_sixty * rate).abs() < 1e-9);

    sim.set_frame_seconds(0.5 / rate);
    assert!((sim.config.speed - step_at_sixty / 2.0).abs() < 1e-9);
    assert!((sim.turn_limit() - bird_at_sixty / 2.0).abs() < 1e-9);
    assert!((sim.hawk_turn_limit() - hawk_at_sixty / 2.0).abs() < 1e-9);
    assert!((sim.config.speed * rate * 2.0 - step_at_sixty * rate).abs() < 1e-9);

    sim.set_frame_seconds(0.0);
    assert_eq!(sim.config.speed, 0.0);
    assert_eq!(sim.turn_limit(), 0.0);
    assert_eq!(sim.hawk_turning_radius(), 0.0);

    sim.apply_screen_size(40, 14, 320, 224);
    sim.set_frame_seconds(1.0 / rate);
    let small_step_at_sixty = sim.config.speed;
    assert!(small_step_at_sixty <= f64::from(sim.screen.height) / 10.0 + 1e-9);
    sim.set_frame_seconds(2.0 / rate);
    assert!((sim.config.speed - small_step_at_sixty * 2.0).abs() < 1e-9);
    assert!((sim.config.speed * rate / 2.0 - small_step_at_sixty * rate).abs() < 1e-9);

    sim.apply_screen_size(200, 60, 1600, 960);
    assert!(sim.config.speed > f64::from(sim.screen.height) / 20.0);

    sim.config.turning_notch = LEGEND_BAR_CELLS;
    sim.set_frame_seconds(2.0 / rate);
    assert_eq!(sim.turn_limit(), 2.0 * PI);
    assert!(sim.hawk_turn_limit() <= PI);
}

#[test]
fn test_more_flocks_are_more_colours() {
    let mut w = enter!("test_more_flocks_are_more_colours");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.config.palette = palette_named("ember");
    assert_eq!(sim.palette_shades(), 5);

    sim.config.flocks = 1;
    let east = Bird { direction: 0.0, flock: 0, ..Bird::default() };
    let west = Bird { direction: PI, flock: 0, ..Bird::default() };
    assert_ne!(sim.shade_for(&east), sim.shade_for(&west));

    sim.config.flocks = 3;
    let mut seen = [false; MAX_PALETTE_SHADES as usize];
    for flock in 0..3 {
        let member = Bird { direction: 1.0, flock, ..Bird::default() };
        let other_way = Bird { direction: 4.0, flock, ..Bird::default() };
        let shade = sim.shade_for(&member);
        assert_eq!(shade, sim.shade_for(&other_way));
        assert!(shade >= 0 && shade < sim.palette_shades());
        seen[shade as usize] = true;
    }
    assert_eq!(seen.iter().filter(|&&s| s).count(), 3);
    assert_eq!(sim.shade_for(&Bird { flock: 0, ..Bird::default() }), 0);
    assert_eq!(sim.shade_for(&Bird { flock: 2, ..Bird::default() }), sim.palette_shades() - 1);
}

#[test]
fn test_the_flock_can_be_laid_out_as_text() {
    let mut w = enter!("test_the_flock_can_be_laid_out_as_text");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = true;
    sim.apply_screen_size(200, 50, 1600, 800);

    let made = sim.formation_layout(b"HELLO");
    assert!(made > 0 && made <= rbirds::font::text_cells(b"HELLO"));
    assert!(sim.formation.writing);
    for i in 0..made as usize {
        let (x, y) = (sim.formation.x[i], sim.formation.y[i]);
        assert!(!sim.legend_turn_zone(x, y));
        assert!(x >= 0.0 && x <= f64::from(sim.screen.width));
        assert!(y >= 0.0 && y <= f64::from(sim.screen.height));
    }

    let first = sim.formation.target_of(0).expect("a target");
    let wrapped = sim.formation.target_of(made).expect("a wrapped target");
    assert_eq!(first, wrapped);

    const COUNT: usize = 8;
    sim.config.birds = COUNT as i32;
    let mut birds = vec![bird(sim.formation.x[0] - 100.0, sim.formation.y[0], PI); COUNT];
    let mut grid = grid_for(sim, COUNT);
    build(&mut grid, &birds);
    assert!(angle_difference(sim.flock_direction(&birds, &grid, 0), 0.0) < 1e-12);

    birds[0].x = sim.formation.x[0] - sim.config.speed / 2.0;
    birds[0].y = sim.formation.y[0];
    let snapshot = birds.clone();
    sim.update_birds(&mut birds, &snapshot, &grid);
    assert!((birds[0].x - sim.formation.x[0]).abs() < 1e-9);
    assert!((birds[0].y - sim.formation.y[0]).abs() < 1e-9);

    sim.formation.clear();
    assert!(!sim.formation.writing);
    assert!(sim.formation.target_of(0).is_none());

    assert_eq!(sim.formation_layout(b""), 0);
    assert_eq!(sim.formation_layout(b"\x01\x02"), 0);
    sim.apply_screen_size(44, 15, 44 * 8, 15 * 16);
    assert_eq!(sim.formation_layout(b"A VERY LONG MESSAGE INDEED THAT WILL NOT FIT AT ALL"), 0);
    assert!(!sim.formation.writing);
}

#[test]
fn test_presets_set_every_notch() {
    let mut w = enter!("test_presets_set_every_notch");
    reset_test_config(&mut w);
    for i in 0..PRESET_COUNT {
        w.sim.apply_preset(i);
        let c = &w.sim.config;
        let notches = [c.boundary_notch, c.separation_notch, c.alignment_notch, c.vision_notch];
        assert!(notches.iter().all(|n| (0..=LEGEND_BAR_CELLS).contains(n)));
        assert!(notches.iter().any(|&n| n != DEFAULT_NOTCH));
        assert!(c.boundary >= BOUNDARY_MIN && c.boundary <= BOUNDARY_MAX);
        assert!(c.vision_radius >= MIN_VISION_RADIUS && c.vision_radius <= MAX_VISION_RADIUS);
    }
    for i in 0..PRESETS.len() {
        for j in i + 1..PRESETS.len() {
            assert_ne!(PRESETS[i].notch, PRESETS[j].notch);
        }
    }
}

#[test]
fn test_a_notch_survives_the_round_trip() {
    let _w = enter!("test_a_notch_survives_the_round_trip");
    for notch in 0..=LEGEND_BAR_CELLS {
        let pixels = notch_integer(notch, MIN_VISION_RADIUS, MAX_VISION_RADIUS);
        assert_eq!(notch_for_integer(pixels, MIN_VISION_RADIUS, MAX_VISION_RADIUS), notch);
    }
    assert_eq!(notch_for_integer(MIN_VISION_RADIUS, MIN_VISION_RADIUS, MAX_VISION_RADIUS), 0);
    assert_eq!(
        notch_for_integer(MAX_VISION_RADIUS, MIN_VISION_RADIUS, MAX_VISION_RADIUS),
        LEGEND_BAR_CELLS
    );
    assert_eq!(notch_for_integer(50, MIN_VISION_RADIUS, MAX_VISION_RADIUS), 10);
}

#[test]
fn test_the_pointer_moves_the_flock() {
    let mut w = enter!("test_the_pointer_moves_the_flock");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = false;
    sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);
    sim.mouse.present = true;
    sim.mouse.x = 800.0;
    sim.mouse.y = 400.0;

    let east = bird(840.0, 400.0, 0.0);
    let away = sim.pointer_vector(&east);
    assert!(away.x > 0.0 && away.y.abs() < 1e-12);

    let near = bird(810.0, 400.0, 0.0);
    let far = bird(900.0, 400.0, 0.0);
    let beyond = bird(800.0 + f64::from(MOUSE_REACH) + 1.0, 400.0, 0.0);
    assert!(sim.pointer_vector(&near).x > sim.pointer_vector(&far).x);
    assert!(sim.pointer_vector(&beyond).x == 0.0 && sim.pointer_vector(&beyond).y == 0.0);

    sim.mouse.present = false;
    assert_eq!(sim.pointer_vector(&east).x, 0.0);
    sim.mouse.present = true;

    const COUNT: usize = 30;
    sim.config.birds = COUNT as i32;
    let birds = vec![bird(840.0, 400.0, PI); COUNT];
    let mut grid = grid_for(sim, COUNT);
    build(&mut grid, &birds);
    assert!(sim.flock_direction(&birds, &grid, 0).cos() > 0.0);
    sim.mouse.present = false;
    assert!(sim.flock_direction(&birds, &grid, 0).cos() < 0.0);
}

#[test]
fn test_the_shade_follows_the_heading() {
    let mut w = enter!("test_the_shade_follows_the_heading");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.config.palette = palette_named("ember");
    let shades = sim.palette_shades();
    assert_eq!(shades, 5);

    let heading = |direction: f64| Bird { direction, ..Bird::default() };
    let mut seen = [false; 8];
    let mut previous = sim.shade_for(&heading(0.0));
    for step in 0..360 {
        let shade = sim.shade_for(&heading(f64::from(step) * PI / 180.0));
        assert!(shade >= 0 && shade < shades);
        assert!((shade - previous).abs() <= 1);
        previous = shade;
        seen[shade as usize] = true;
    }
    assert!(seen[..shades as usize].iter().all(|&s| s));
    assert_eq!(sim.shade_for(&heading(0.0)), 0);
    assert_eq!(sim.shade_for(&heading(PI)), shades - 1);
    assert_eq!(sim.shade_for(&heading(2.0 * PI - 0.001)), 0);

    // Somebody's own sprite keeps its colours: one set, nothing tinted.
    sim.custom_sprite = true;
    assert_eq!(sim.palette_shades(), 1);
    assert_eq!(sim.shade_for(&heading(2.0)), 0);
}

#[test]
fn test_the_hawk_is_never_the_colour_of_the_flock() {
    let mut w = enter!("test_the_hawk_is_never_the_colour_of_the_flock");
    let sim = &mut w.sim;
    for palette in 0..PALETTE_COUNT {
        sim.config.palette = palette;
        if sim.palette_follows_the_theme() {
            sim.theme.ramp_between([205, 0, 0], [18, 18, 24]);
        }
        let hawk = sim.hawk_colour();
        let tints = *sim.palette_tints().expect("every palette has tints");
        let shades = sim.palette().shades as usize;
        let nearest = tints[..shades].iter().map(|t| colour_distance(hawk, *t)).fold(1e9, f64::min);
        assert!(nearest > 200.0);

        let mut feather = rbirds::image::Image::alloc(1, 1).expect("alloc");
        feather.pixels[3] = 255;
        sim.hawk_tint(&mut feather);
        assert_eq!(&feather.pixels[..3], &hawk);

        for candidate in HAWK_COLOURS {
            let other =
                tints[..shades].iter().map(|t| colour_distance(candidate, *t)).fold(1e9, f64::min);
            assert!(other <= nearest);
        }
    }
}

#[test]
fn test_the_help_names_every_ramp() {
    let mut w = enter!("test_the_help_names_every_ramp");
    reset_test_config(&mut w);
    let color = OPTIONS.iter().find(|o| o.name == "color").expect("a color option");
    let rbirds::options::Kind::Enum { names, .. } = &color.kind else { panic!("color is an enum") };
    assert_eq!(*names, &rbirds::palette::PALETTE_NAMES[..]);

    let mut listed = color.help;
    for (i, palette) in PALETTES.iter().enumerate() {
        if i > 0 {
            assert!(listed.starts_with(", "));
            listed = &listed[2..];
        }
        assert!(listed.starts_with(palette.name));
        listed = &listed[palette.name.len()..];

        let mut program = rbirds::app::Program {
            sim: w.sim.clone(),
            settings: w.settings.clone(),
            renderer: Default::default(),
        };
        program.sim.config.palette = -1;
        let parsed = rbirds::options::parse(
            &OPTIONS,
            &mut program,
            &args(&["cbirds", "--color", palette.name]),
            160,
        );
        assert_eq!(parsed.status, rbirds::options::Status::Ok);
        assert_eq!(program.sim.config.palette, i as i32);
        assert_eq!(palette.shades, 5);
    }
    assert!(listed.is_empty());
}

#[test]
fn test_no_ramp_fades_into_a_black_terminal() {
    let mut w = enter!("test_no_ramp_fades_into_a_black_terminal");
    for palette in 0..PALETTE_COUNT {
        w.sim.config.palette = palette;
        if w.sim.palette_follows_the_theme() {
            continue;
        }
        let tints = w.sim.palette_tints().expect("tints");
        for tint in &tints[..w.sim.palette().shades as usize] {
            assert!(contrast_between(*tint, [0, 0, 0]) >= 2.5);
        }
    }
}

#[test]
fn test_the_theme_ramp_never_reaches_the_background() {
    let _w = enter!("test_the_theme_ramp_never_reaches_the_background");
    let grounds: [[u8; 3]; 5] =
        [[0, 0, 0], [18, 18, 24], [40, 42, 54], [0, 43, 54], [253, 246, 227]];
    let accents: [[u8; 3]; 5] =
        [[205, 0, 0], [0, 0, 238], [189, 147, 249], [251, 73, 52], [42, 161, 152]];
    let mut theme = Theme::default();
    for ground in grounds {
        for accent in accents {
            theme.ramp_between(accent, ground);
            for tint in theme.tints {
                assert_ne!(tint, ground);
                for c in 0..3 {
                    let travelled = (i32::from(tint[c]) - i32::from(accent[c])).abs();
                    let whole = (i32::from(ground[c]) - i32::from(accent[c])).abs();
                    assert!(travelled * 3 <= whole * 2 + 1);
                }
            }
            for shade in 1..5 {
                let near = contrast_between(theme.tints[shade - 1], ground);
                let far = contrast_between(theme.tints[shade], ground);
                assert!(far <= near);
            }
        }
    }
    let dark = [18, 18, 24];
    for accent in accents {
        if contrast_between(accent, dark) < 3.0 {
            continue;
        }
        theme.ramp_between(accent, dark);
        for tint in theme.tints {
            assert!(contrast_between(tint, dark) > 1.3);
        }
    }
}

#[test]
fn test_theme_colours_are_parsed() {
    let _w = enter!("test_theme_colours_are_parsed");
    assert_eq!(parse_osc_colour(b"\x1b]4;1;rgb:cc24/1d1d/1f1f\x1b\\"), Some([0xcc, 0x1d, 0x1f]));
    assert_eq!(parse_osc_colour(b"\x1b]11;rgb:12/34/56\x1b\\"), Some([0x12, 0x34, 0x56]));
    assert_eq!(parse_osc_colour(b""), None);
    assert_eq!(parse_osc_colour(b"\x1b]4;1;?\x1b\\"), None);
    assert_eq!(parse_osc_colour(b"rgb:"), None);
    assert_eq!(parse_osc_colour(b"rgb:zz/zz/zz"), None);

    let grey = [128, 128, 128];
    let red = [204, 29, 31];
    assert_eq!(saturation_of(grey), 0);
    assert!(saturation_of(red) > saturation_of(grey));

    let mut theme = Theme::default();
    theme.ramp_between(red, [18, 18, 24]);
    assert!(theme.tints[0][0] == red[0] && theme.tints[0][1] == red[1]);
    assert!(theme.tints[4][0] < theme.tints[0][0]);
    for i in 1..5 {
        assert!(theme.tints[i][0] <= theme.tints[i - 1][0]);
    }
}

#[test]
fn test_each_flock_flies_at_its_own_pace() {
    let mut w = enter!("test_each_flock_flies_at_its_own_pace");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.config.flocks = 1;
    assert_eq!(sim.flock_pace(0), 1.0);
    sim.config.flocks = 3;
    assert_eq!(sim.flock_pace(0), 1.0);
    assert!(sim.flock_pace(1) < sim.flock_pace(0));
    assert!(sim.flock_pace(2) < sim.flock_pace(1));
    assert!(sim.flock_pace(2) > 0.8);

    sim.apply_screen_size(200, 50, 1600, 800);
    sim.config.birds = 2;
    let mut birds = vec![
        Bird { x: 800.0, y: 400.0, direction: 0.0, flock: 0, ..Bird::default() },
        Bird { x: 800.0, y: 450.0, direction: 0.0, flock: 2, ..Bird::default() },
    ];
    let snapshot = birds.clone();
    let mut grid = grid_for(sim, 2);
    build(&mut grid, &snapshot);
    sim.update_birds(&mut birds, &snapshot, &grid);
    for s in &snapshot {
        let force = sim.boundary_vector(s);
        assert!(force.x == 0.0 && force.y == 0.0);
    }
    assert!(birds[0].x - 800.0 > birds[1].x - 800.0);
}

#[test]
fn test_the_matrix_is_the_only_thing_that_rains() {
    let mut w = enter!("test_the_matrix_is_the_only_thing_that_rains");
    reset_test_config(&mut w);
    w.sim.rain = false;
    let mut program = rbirds::app::Program {
        sim: w.sim.clone(),
        settings: w.settings.clone(),
        renderer: Default::default(),
    };
    let mut stdout = CStdout::captured();
    assert!(
        read_options(&mut program, &args(&["cbirds", "--matrix", "--flocks", "3"]), &mut stdout)
            .is_ok()
    );
    let sim = &program.sim;
    assert_eq!(sim.config.palette, palette_named("matrix"));
    assert!(sim.config.trails);
    assert!(sim.rain);
    let first = Bird { flock: 0, ..Bird::default() };
    let last = Bird { flock: 2, ..Bird::default() };
    assert_ne!(sim.shade_for(&first), sim.shade_for(&last));
}

#[test]
fn test_the_sprite_catalogue_has_a_place_for_everything() {
    let mut w = enter!("test_the_sprite_catalogue_has_a_place_for_everything");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.config.palette = palette_named("ember");
    let shades = sim.palette_shades();
    let mut seen = vec![0; rbirds::sprites::MAX_SPRITE_SETS as usize];
    for shade in 0..shades {
        for wing in 0..WING_PHASES {
            seen[sim.flock_set(shade, wing, 0) as usize] += 1;
        }
        seen[sim.flock_set(shade, 0, 1) as usize] += 1;
    }
    for wing in 0..WING_PHASES {
        seen[sim.hawk_set(wing) as usize] += 1;
    }
    for step in 0..TRAIL_LENGTH {
        seen[sim.trail_set(step) as usize] += 1;
    }
    for set in 0..sim.sprite_set_count() as usize {
        assert_eq!(seen[set], 1);
    }
    assert!(sim.sprite_set_count() <= rbirds::sprites::MAX_SPRITE_SETS);
    assert_eq!(set_image_id(0, 0), 1);
    assert_eq!(set_image_id(1, 0), ROTATION_FRAMES as u32 + 1);
    let near = Bird { shade: 2, wing: 1, layer: 0, frame: 7, ..Bird::default() };
    let far = Bird { shade: 2, wing: 1, layer: 1, frame: 7, ..Bird::default() };
    assert_eq!(sim.sprite_image_id(&near), set_image_id(sim.flock_set(2, WING_SEQUENCE[1], 0), 7));
    assert_eq!(sim.sprite_image_id(&far), set_image_id(sim.flock_set(2, 0, 1), 7));
    assert_ne!(sim.sprite_image_id(&near), sim.sprite_image_id(&far));

    let mut frames = empty_catalogue();
    assert_eq!(frames.len(), CATALOGUE_IMAGES);
    sim.rasterise_sprites(&mut frames, None, b"cbirds").expect("sprites");
    let frame = |set: i32| &frames[(set * ROTATION_FRAMES) as usize];
    for set in 0..sim.sprite_set_count() {
        for f in 0..ROTATION_FRAMES {
            assert!(!frames[(set * ROTATION_FRAMES + f) as usize].is_empty());
        }
    }
    assert!(frame(sim.flock_set(0, 0, 1)).width < sim.config.bird_size);
    assert!(frame(sim.trail_set(0)).width < sim.config.bird_size);
    assert_eq!(frame(sim.hawk_set(0)).width, sim.hawk_sprite_size());
    let ink = |image: &rbirds::image::Image| -> i64 {
        image.pixels.chunks_exact(4).map(|p| i64::from(p[3])).sum()
    };
    let spread = ink(frame(sim.flock_set(0, 0, 0)));
    let folded = ink(frame(sim.flock_set(0, WING_PHASES - 1, 0)));
    assert!(folded < spread * 3 / 4);
    for step in 1..TRAIL_LENGTH {
        assert!(ink(frame(sim.trail_set(step))) < ink(frame(sim.trail_set(step - 1))));
    }
    let first_opaque = |image: &rbirds::image::Image| -> i32 {
        image
            .pixels
            .chunks_exact(4)
            .find(|p| p[3] == 255)
            .map_or(0, |p| i32::from(p[0]) + i32::from(p[1]) + i32::from(p[2]))
    };
    assert!(
        first_opaque(frame(sim.flock_set(0, 0, 1))) < first_opaque(frame(sim.flock_set(0, 0, 0)))
    );
    free_sprites(&mut frames);
}

#[test]
fn test_the_far_layer_is_another_sky() {
    let mut w = enter!("test_the_far_layer_is_another_sky");
    const COUNT: usize = 30;
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = false;
    sim.apply_screen_size(200, 50, 1600, 800);
    sim.config.birds = COUNT as i32;
    let mut birds =
        vec![Bird { x: 800.0, y: 400.0, direction: PI, layer: 1, ..Bird::default() }; COUNT];
    birds[0] = Bird { x: 800.0, y: 400.0, direction: 0.0, layer: 0, ..Bird::default() };
    let mut grid = grid_for(sim, COUNT);
    build(&mut grid, &birds);
    sim.measure_flocks(&birds);
    assert_eq!(sim.flock_direction(&birds, &grid, 0), 0.0);

    birds[1] = Bird { x: 800.0, y: 600.0, direction: 0.0, layer: 1, ..Bird::default() };
    let snapshot = birds.clone();
    build(&mut grid, &snapshot);
    sim.update_birds(&mut birds, &snapshot, &grid);
    let near_step = birds[0].x - 800.0;
    let far_step = birds[1].x - 800.0;
    assert!(near_step > 0.0 && far_step > 0.0);
    assert!((far_step - near_step * FAR_PACE).abs() < 1e-6);

    sim.config.hawks = 1;
    sim.place_hawks();
    sim.hawks[0].x = 100.0;
    sim.hawks[0].y = 400.0;
    birds[0] = Bird { x: 700.0, y: 400.0, layer: 0, ..Bird::default() };
    birds[1] = Bird { x: 150.0, y: 400.0, layer: 1, ..Bird::default() };
    for b in &mut birds[2..] {
        *b = Bird { x: 1500.0, y: 700.0, layer: 1, ..Bird::default() };
    }
    assert_eq!(sim.nearest_bird(&birds, sim.hawks[0].x, sim.hawks[0].y, 0, false, 0.0), 0);
    let v = sim.hawk_vector(&birds[1]);
    assert!(v.x == 0.0 && v.y == 0.0);
    birds[1].layer = 0;
    assert!(sim.hawk_vector(&birds[1]).x > 0.0);

    sim.config.hawks = 0;
    let mut far = 0;
    sim.rng.seed(3);
    sim.deep_look = true;
    for (i, b) in birds.iter_mut().enumerate() {
        sim.place_one_bird(b, i as i32);
        far += b.layer;
    }
    assert!(far > 0 && far < COUNT as i32);
    sim.deep_look = false;
    for (i, b) in birds.iter_mut().enumerate() {
        sim.place_one_bird(b, i as i32);
        assert_eq!(b.layer, 0);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    // Test-only: the C library's own generator, to compare against, as the
    // C test compares under __GLIBC__.
    #[allow(unsafe_code)]
    unsafe extern "C" {
        pub fn srand(seed: u32);
        pub fn rand() -> i32;
    }
}

#[test]
fn test_a_seed_draws_the_same_numbers_everywhere() {
    let _w = enter!("test_a_seed_draws_the_same_numbers_everywhere");
    const FIRST: [u32; 8] = [
        1804289383, 846930886, 1681692777, 1714636915, 1957747793, 424238335, 719885386, 1649760492,
    ];
    for seed in 0..=1 {
        let mut rng = rbirds::rng::Rng::seeded(seed);
        for expected in FIRST {
            assert_eq!(rng.next_random(), expected);
        }
    }
    let mut rng = rbirds::rng::Rng::seeded(42);
    let (mut lowest, mut highest) = (1.0_f64, 0.0_f64);
    for _ in 0..100_000 {
        let unit = rng.random_unit();
        assert!((0.0..=1.0).contains(&unit));
        lowest = lowest.min(unit);
        highest = highest.max(unit);
    }
    assert!(lowest < 0.001 && highest > 0.999);
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    for seed in [2_u32, 33, 20260911, 2147483647, 2147483648, 4294967295] {
        let mut rng = rbirds::rng::Rng::seeded(seed);
        #[allow(unsafe_code)]
        // SAFETY: srand and rand take and return plain integers; the test
        // runs them on one thread between its own calls.
        unsafe {
            glibc::srand(seed);
            for _ in 0..10_000 {
                assert_eq!(rng.next_random(), glibc::rand() as u32);
            }
        }
    }
}

#[test]
fn test_wings_beat_and_sometimes_glide() {
    let mut w = enter!("test_wings_beat_and_sometimes_glide");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    let mut b = Bird::default();
    let mut phases_seen = [0; WING_CYCLE as usize];
    sim.rng.seed(1);
    let (mut beats, mut glided, mut glides) = (0_i32, 0_i32, 0_i32);
    let frames: i32 = 6000;
    for _ in 0..frames {
        let before = b.wing;
        let was_gliding = b.gliding > 0.0;
        sim.beat_wings(&mut b);
        phases_seen[(b.wing % WING_CYCLE) as usize] += 1;
        if b.gliding > 0.0 {
            glided += 1;
        }
        if b.gliding > 0.0 && !was_gliding {
            glides += 1;
        }
        if before == WING_CYCLE - 1 && b.wing == 0 {
            beats += 1;
        }
        assert!(
            b.wing == before
                || b.wing == (before + 1) % WING_CYCLE
                || (b.gliding > 0.0 && b.wing == 0)
        );
    }
    assert!(phases_seen.iter().all(|&p| p > 0));
    let flapping = frames - glided;
    assert!((beats * 10 - flapping).abs() <= 10 + glides);
    assert!(glides > 0);
    assert!(glided < frames / 2);
    assert_eq!(WING_SEQUENCE[1], WING_SEQUENCE[3]);
    assert!(WING_SEQUENCE[0] == 0 && WING_SEQUENCE[2] == WING_PHASES - 1);
}

#[test]
fn test_braille_unless_asked() {
    let mut w = enter!("test_braille_unless_asked");
    w.sim.render_mode = RenderMode::Unset;
    assert_eq!(w.sim.live_render_mode(), RenderMode::Braille);
    for asked in [RenderMode::Kitty, RenderMode::Braille, RenderMode::Sextants, RenderMode::Blocks]
    {
        w.sim.render_mode = asked;
        assert_eq!(w.sim.live_render_mode(), asked);
    }
    let mut program = rbirds::app::Program {
        sim: w.sim.clone(),
        settings: w.settings.clone(),
        renderer: Default::default(),
    };
    program.settings.render_request = -1;
    let parsed = rbirds::options::parse(
        &OPTIONS,
        &mut program,
        &args(&["cbirds", "--render", "kitty"]),
        160,
    );
    assert_eq!(parsed.status, rbirds::options::Status::Ok);
    assert_eq!(RenderMode::from_index(program.settings.render_request), RenderMode::Kitty);
    let parsed =
        rbirds::options::parse(&OPTIONS, &mut program, &args(&["cbirds", "--render", "auto"]), 160);
    assert_eq!(parsed.status, rbirds::options::Status::Error);
    assert!(contains(&parsed.message, b"kitty, braille, sextants, blocks"));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = support::oracle::repository()
        .join("target/scratch")
        .join(format!("c_boids.{}.{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

#[test]
fn test_a_text_terminal_gets_the_flock_in_braille() {
    let mut w = enter!("test_a_text_terminal_gets_the_flock_in_braille");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = false;
    sim.config.palette = palette_named("ember");
    sim.config.birds = 3;
    sim.config.hawks = 1;
    sim.apply_screen_size(60, 20, 480, 320);
    let birds = vec![
        bird(100.0, 100.0, 0.3),
        Bird { x: 104.0, y: 103.0, direction: 2.9, shade: 4, ..Bird::default() },
        bird(300.0, 200.0, 4.0),
    ];
    sim.place_hawks();

    sim.render_mode = RenderMode::Braille;
    assert!(sim.drawing_with_text());
    w.renderer.prepare_text_renderer(sim, None, b"cbirds").expect("text renderer");
    let mut graphics = KittyGraphics::new(1).expect("graphics");
    w.renderer.queue_render_frame(&mut graphics, sim, &birds).expect("frame");
    let buffer = graphics.buffer().to_vec();

    assert!(buffer.starts_with(b"\x1b[?2026h"));
    assert!(!contains(&buffer, b"\x1b_G"));
    assert!(contains(&buffer, b"\x1b[38;2;") || contains(&buffer, b"\x1b[38;5;"));
    let braille = buffer.windows(2).filter(|c| c[0] == 0xE2 && (c[1] & 0xFC) == 0xA0).count();
    assert!(braille >= 3);
    let tints = *sim.palette_tints().expect("tints");
    let hawk = sim.hawk_colour();
    let own =
        |rgb: &[u8]| rgb == hawk || tints[..sim.palette_shades() as usize].iter().any(|t| rgb == t);
    for pixel in w.renderer.canvas.pixels.chunks_exact(4) {
        if pixel[3] != 0 {
            assert!(own(&pixel[..3]));
        }
    }
    let mut at = 0;
    while let Some(found) = find(&buffer[at..], b"\x1b[38;2;") {
        let start = at + found + 7;
        let end = start + buffer[start..].iter().position(|&b| b == b'm').expect("sgr end");
        let rgb: Vec<u8> = std::str::from_utf8(&buffer[start..end])
            .unwrap()
            .split(';')
            .map(|v| v.parse::<i32>().unwrap() as u8)
            .collect();
        assert!(own(&rgb));
        at = end;
    }
    assert!(!contains(&buffer, b"\x1b[2J"));

    let first = graphics.len();
    graphics.clear();
    w.renderer.queue_render_frame(&mut graphics, sim, &birds).expect("frame");
    assert!(graphics.len() < first / 4);

    sim.render_mode = RenderMode::Blocks;
    graphics.clear();
    w.renderer.cells.invalidate();
    w.renderer.queue_render_frame(&mut graphics, sim, &birds).expect("frame");
    assert!(
        contains(graphics.buffer(), b"\xe2\x96\x80")
            || contains(graphics.buffer(), b"\xe2\x96\x84")
    );

    let dir = scratch("text_snapshot");
    let path = dir.join("text_snapshot.png");
    assert_eq!(
        rbirds::live::write_snapshot(sim, &w.renderer, path.as_os_str(), &birds, None, b"cbirds"),
        Ok(true)
    );
    if std::path::Path::new("/dev/full").exists() {
        let full = std::ffi::OsStr::new("/dev/full");
        assert_eq!(
            rbirds::live::write_snapshot(sim, &w.renderer, full, &birds, None, b"cbirds"),
            Ok(false)
        );
    }
    let bytes = std::fs::read(&path).expect("snapshot");
    let _ = std::fs::remove_dir_all(&dir);
    let picture = png::decode(&bytes).expect("decode");
    assert_eq!(picture.width, sim.screen.cols * sim.screen.cell_width);
    assert_eq!(picture.height, sim.screen.rows * sim.screen.cell_height);
}

fn record(w: &mut World, path: &std::path::Path) -> i32 {
    w.settings.record_path = Some(path.as_os_str().to_owned());
    let mut quiet = CStdout::captured();
    run_recording(&mut w.sim, &mut w.renderer, &w.settings, &mut quiet)
}

#[test]
fn test_a_text_renderer_records_its_cells() {
    let mut w = enter!("test_a_text_renderer_records_its_cells");
    let dir = scratch("record_braille");
    let path = dir.join("record_braille.gif");
    reset_test_config(&mut w);
    w.sim.config.birds = 60;
    w.sim.config.palette = palette_named("ember");
    w.sim.render_mode = RenderMode::Braille;
    w.settings.record_fps = 20;
    w.settings.record_seconds = 1;
    w.settings.record_columns = 60;
    w.settings.record_rows = 20;
    assert_eq!(record(&mut w, &path), 0);
    let bytes = std::fs::read(&path).expect("recording");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(&bytes[..6], b"GIF89a");
    assert_eq!(i32::from(bytes[6]) | i32::from(bytes[7]) << 8, 60 * DEFAULT_CELL_WIDTH);
    assert_eq!(i32::from(bytes[8]) | i32::from(bytes[9]) << 8, 20 * DEFAULT_CELL_HEIGHT);
    let descriptors = bytes[10..].iter().filter(|&&c| c == 0x2C).count();
    assert!(descriptors >= 20);
}

#[test]
fn test_a_cast_is_the_flock_as_text() {
    let mut w = enter!("test_a_cast_is_the_flock_as_text");
    let dir = scratch("record_cast");
    let path = dir.join("record.cast");
    reset_test_config(&mut w);
    w.sim.config.birds = 60;
    w.sim.config.palette = palette_named("ember");
    w.settings.record_fps = 20;
    w.settings.record_seconds = 1;
    w.settings.record_columns = 60;
    w.settings.record_rows = 20;
    assert_eq!(record(&mut w, &path), 0);
    let text = std::fs::read(&path).expect("cast");
    let _ = std::fs::remove_dir_all(&dir);
    let mut lines = text.split(|&b| b == b'\n').filter(|l| !l.is_empty());
    assert!(
        lines
            .next()
            .expect("header")
            .starts_with(b"{\"version\": 2, \"width\": 60, \"height\": 20,")
    );
    let (mut events, mut braille, mut raw_escapes) = (0, 0, 0);
    for line in lines {
        events += 1;
        assert_eq!(line[0], b'[');
        assert!(contains(line, b", \"o\", \""));
        assert!(contains(line, b"\\u001b"));
        raw_escapes += line.iter().filter(|&&c| c == 0x1b).count();
        braille += line.windows(2).filter(|c| c[0] == 0xE2 && (c[1] & 0xFC) == 0xA0).count();
    }
    assert_eq!(raw_escapes, 0);
    assert_eq!(events, 20 + 2);
    assert!(braille > 60);
}

#[test]
fn test_recording_gives_the_whole_frame_to_the_flock() {
    let mut w = enter!("test_recording_gives_the_whole_frame_to_the_flock");
    let dir = scratch("record_whole");
    let path = dir.join("record.gif");
    w.sim.legend_enabled = true;
    reset_test_config(&mut w);
    w.sim.config.palette = palette_named("theme");
    assert!(w.sim.palette_follows_the_theme());
    w.sim.config.birds = 40;
    w.settings.record_fps = 25;
    w.settings.record_seconds = 1;
    w.settings.record_columns = 60;
    w.settings.record_rows = 20;
    assert_eq!(record(&mut w, &path), 0);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!w.sim.palette_follows_the_theme());
    assert!(w.sim.palette_shades() > 1);
    assert!(!w.sim.legend_enabled);
    assert!(w.sim.screen.legend_width == 0 && w.sim.screen.legend_height == 0);
}

#[test]
fn test_flocks_keep_to_their_own_side_of_the_sky() {
    let mut w = enter!("test_flocks_keep_to_their_own_side_of_the_sky");
    const COUNT: usize = 300;
    let mut birds = vec![Bird::default(); COUNT];
    for flocks in 2..=MAX_FLOCKS {
        reset_test_config(&mut w);
        let sim = &mut w.sim;
        sim.legend_enabled = false;
        sim.config.avoid_notch = 8;
        sim.apply_notches();
        sim.apply_screen_size(100, 28, 800, 448);
        sim.config.birds = COUNT as i32;
        sim.config.flocks = flocks;
        sim.rng.seed(1);
        let (half_w, half_h) =
            (f64::from(sim.screen.width) / 2.0, f64::from(sim.screen.height) / 2.0);
        for (i, b) in birds.iter_mut().enumerate() {
            *b = Bird {
                x: half_w + (i % 17) as f64 - 8.0,
                y: half_h + (i % 13) as f64 - 6.0,
                direction: i as f64 * 0.21,
                flock: i as i32 % flocks,
                ..Bird::default()
            };
        }
        let mut grid = grid_for(sim, COUNT);
        let (mut gap_sum, mut measured) = (0.0, 0);
        for frame in 0..700 {
            let snapshot = birds.clone();
            build(&mut grid, &snapshot);
            sim.update_birds(&mut birds, &snapshot, &grid);
            if frame < 300 {
                continue;
            }
            let mut least = f64::from(sim.screen.width + sim.screen.height);
            for f in 0..flocks as usize {
                assert!(
                    sim.flock_home_x[f] >= 0.0
                        && sim.flock_home_x[f] <= f64::from(sim.screen.width)
                );
                assert!(
                    sim.flock_home_y[f] >= 0.0
                        && sim.flock_home_y[f] <= f64::from(sim.screen.height)
                );
                let shove_x = sim.flock_home_x[f] - sim.flock_center_x[f];
                let shove_y = sim.flock_home_y[f] - sim.flock_center_y[f];
                assert!((shove_x * shove_x + shove_y * shove_y).sqrt() <= sim.flock_room() + 1e-9);
                for g in f + 1..flocks as usize {
                    let dx = sim.flock_center_x[f] - sim.flock_center_x[g];
                    let dy = sim.flock_center_y[f] - sim.flock_center_y[g];
                    least = least.min((dx * dx + dy * dy).sqrt());
                }
            }
            gap_sum += least;
            measured += 1;
        }
        assert!(measured > 0);
        assert!(gap_sum / f64::from(measured) > f64::from(FLOCK_LEASH) * 0.8);

        sim.measure_flocks(&birds);
        let (mut foreign, mut counted) = (0, 0);
        for i in (0..COUNT).step_by(7) {
            let mut nearest = None;
            let mut best = 0.0;
            for j in 0..COUNT {
                if j == i {
                    continue;
                }
                let dx = birds[i].x - birds[j].x;
                let dy = birds[i].y - birds[j].y;
                let distance = dx * dx + dy * dy;
                if nearest.is_none() || distance < best {
                    best = distance;
                    nearest = Some(j);
                }
            }
            counted += 1;
            if birds[nearest.unwrap()].flock != birds[i].flock {
                foreign += 1;
            }
        }
        assert!(counted > 0);
        assert!(foreign * 4 < counted);
    }
    w.sim.config.flocks = 1;
    for b in birds.iter_mut() {
        b.flock = 0;
    }
    w.sim.measure_flocks(&birds);
    for f in 0..MAX_FLOCKS as usize {
        assert!(w.sim.flock_center_x[f] == 0.0 && w.sim.flock_center_y[f] == 0.0);
    }
    let leash = w.sim.leash_vector(&birds[0]);
    assert!(leash.x == 0.0 && leash.y == 0.0);
}

#[test]
fn test_flocks_do_not_align_with_each_other() {
    let mut w = enter!("test_flocks_do_not_align_with_each_other");
    const COUNT: usize = 40;
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = false;
    sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);
    sim.config.birds = COUNT as i32;
    sim.config.flocks = 2;
    let mut birds =
        vec![Bird { x: 800.0, y: 400.0, direction: PI, flock: 1, ..Bird::default() }; COUNT];
    birds[0] = bird(800.0, 400.0, 0.0);
    let mut grid = grid_for(sim, COUNT);
    build(&mut grid, &birds);
    sim.measure_flocks(&birds);
    assert_eq!(sim.flock_direction(&birds, &grid, 0), 0.0);
    sim.config.flocks = 1;
    for b in birds.iter_mut() {
        b.flock = 0;
    }
    sim.measure_flocks(&birds);
    assert!(angle_difference(sim.flock_direction(&birds, &grid, 0), PI) < 1e-12);
}

#[test]
fn test_a_key_ends_the_intro() {
    let mut w = enter!("test_a_key_ends_the_intro");
    reset_test_config(&mut w);
    w.sim.legend_enabled = true;
    w.sim.apply_screen_size(200, 50, 1600, 800);
    w.sim.begin_the_intro();
    assert!(w.sim.formation.writing);
    assert_eq!(w.sim.formation.until, f64::from(INTRO_SECONDS));
    assert!(feed_input(&mut w, b"\x1b[<35;10;5M"));
    assert!(w.sim.formation.writing);
    assert!(feed_input(&mut w, b" "));
    assert!(!w.sim.formation.writing);
}

#[test]
fn test_mouse_reports_are_parsed() {
    let mut w = enter!("test_mouse_reports_are_parsed");
    reset_test_config(&mut w);
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    w.sim.mouse.present = false;

    assert!(feed_input(&mut w, b"\x1b[<35;10;5M"));
    assert!(w.sim.mouse.present);
    assert_eq!(w.sim.mouse.x, 9.5 * f64::from(w.sim.screen.cell_width));
    assert_eq!(w.sim.mouse.y, 4.5 * f64::from(w.sim.screen.cell_height));

    assert!(feed_input(&mut w, b"\x1b[<0;20;9m"));
    assert_eq!(w.sim.mouse.x, 19.5 * f64::from(w.sim.screen.cell_width));
    assert_eq!(w.sim.mouse.y, 8.5 * f64::from(w.sim.screen.cell_height));

    w.sim.config.boundary_notch = DEFAULT_NOTCH;
    w.sim.apply_notches();
    assert!(feed_input(&mut w, b"\x1b[<35;3;3MB"));
    assert_eq!(w.sim.config.boundary_notch, DEFAULT_NOTCH + 1);

    let before_x = w.sim.mouse.x;
    w.sim.config.boundary_notch = DEFAULT_NOTCH;
    w.sim.apply_notches();
    assert!(feed_input(&mut w, b"\x1b[A\x1b[1;2B\x1bOP"));
    assert_eq!(w.sim.mouse.x, before_x);
    assert_eq!(w.sim.config.boundary_notch, DEFAULT_NOTCH);

    let mut overlong = b"\x1b[<".to_vec();
    overlong.extend(std::iter::repeat_n(b'9', 63));
    overlong.push(b'M');
    assert!(feed_input(&mut w, &overlong));
    assert!(!feed_input(&mut w, b"q"));
}

#[test]
fn test_vision_controls() {
    let mut w = enter!("test_vision_controls");
    reset_test_config(&mut w);
    assert_eq!(w.sim.config.vision_radius, DEFAULT_VISION_RADIUS);
    assert_eq!(w.sim.config.vision_radius_squared, DEFAULT_VISION_RADIUS * DEFAULT_VISION_RADIUS);
    assert_eq!(w.sim.config.vision_cells, 3);

    assert!(feed_input(&mut w, b"P"));
    assert_eq!(w.sim.config.vision_notch, 7);
    assert_eq!(w.sim.config.vision_radius, 40);
    assert_eq!(w.sim.config.vision_radius_squared, 1600);
    assert_eq!(w.sim.config.vision_cells, 4);

    assert!(feed_input(&mut w, &[b'P'; INPUT_BUFFER_SIZE]));
    assert_eq!(w.sim.config.vision_notch, LEGEND_BAR_CELLS);
    assert_eq!(w.sim.config.vision_radius, MAX_VISION_RADIUS);
    assert_eq!(w.sim.config.vision_cells, MAX_VISION_CELLS);

    assert!(feed_input(&mut w, &[b'p'; INPUT_BUFFER_SIZE]));
    assert_eq!(w.sim.config.vision_notch, 0);
    assert_eq!(w.sim.config.vision_radius, MIN_VISION_RADIUS);
    assert_eq!(w.sim.config.vision_cells, 1);
    assert!(!feed_input(&mut w, b"q"));
}

#[test]
fn test_flicker_free_render_queue() {
    let mut w = enter!("test_flicker_free_render_queue");
    let mut b = Bird { x: 9.0, y: 17.0, direction: 0.0, frame: 3, ..Bird::default() };
    w.sim.config.birds = 1;
    w.sim.screen.cols = 80;
    w.sim.screen.rows = 24;
    w.sim.screen.cell_width = 8;
    w.sim.screen.cell_height = 16;
    w.sim.screen.legend_width = 0;
    w.sim.screen.legend_height = 0;
    w.renderer.legend_drawn = false;
    let mut graphics = KittyGraphics::new(1).expect("graphics");

    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    let buffer = graphics.buffer();
    assert!(buffer.starts_with(b"\x1b[?2026h"));
    let clear = find(buffer, b"a=d,d=a").expect("clear");
    let placement = find(buffer, b"a=p,I=4,q=2,X=1,Y=1,C=1").expect("placement");
    assert!(clear < placement);
    assert!(!contains(buffer, b"a=d,d=n"));
    assert!(buffer.ends_with(b"\x1b[?2026l"));

    graphics.clear();
    b.x = 18.0;
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(contains(graphics.buffer(), b"a=p,I=4,q=2,X=2,Y=1,C=1"));
    assert!(contains(graphics.buffer(), b"a=d,d=a"));

    graphics.clear();
    b.frame = 4;
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(contains(graphics.buffer(), b"a=p,I=5,q=2,X=2,Y=1,C=1"));
    assert!(!contains(graphics.buffer(), b"a=d,d=n"));

    graphics.clear();
    b.x = -1.0;
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(!contains(graphics.buffer(), b"a=p"));
    assert!(contains(graphics.buffer(), b"a=d,d=a"));
}

#[test]
fn test_legend_panel_layout() {
    let mut w = enter!("test_legend_panel_layout");
    reset_test_config(&mut w);
    assert!(LEGEND_COLUMNS * LEGEND_MAX_ROWS * 4 <= LEGEND_MIN_COLS * LEGEND_MIN_ROWS);
    w.sim.apply_screen_size(
        LEGEND_MIN_COLS - 1,
        LEGEND_MIN_ROWS,
        (LEGEND_MIN_COLS - 1) * 8,
        LEGEND_MIN_ROWS * 16,
    );
    assert_eq!(w.sim.screen.legend_width, 0);
    w.sim.apply_screen_size(
        LEGEND_MIN_COLS,
        LEGEND_MIN_ROWS - 1,
        LEGEND_MIN_COLS * 8,
        (LEGEND_MIN_ROWS - 1) * 16,
    );
    assert_eq!(w.sim.screen.legend_width, 0);
    w.sim.apply_screen_size(
        LEGEND_MIN_COLS,
        LEGEND_MIN_ROWS,
        LEGEND_MIN_COLS * 8,
        LEGEND_MIN_ROWS * 16,
    );
    assert!(w.sim.screen.legend_width > 0);

    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    assert_eq!(w.sim.screen.legend_width, LEGEND_COLUMNS * w.sim.screen.cell_width);
    assert_eq!(w.sim.screen.legend_height, LEGEND_ROWS * w.sim.screen.cell_height);
    let lines = legend(&w);
    for line in &lines[..LEGEND_ROWS as usize] {
        assert_eq!(legend_cells(line), LEGEND_COLUMNS as usize);
    }

    w.renderer.stats.frame_ms = 2.125;
    w.renderer.stats.bytes = 31000.0;
    w.renderer.stats.rate = 60.0;
    let lines = legend(&w);
    for line in &lines[..LEGEND_ROWS as usize] {
        assert_eq!(legend_cells(line), LEGEND_COLUMNS as usize);
    }
    let stats_row = &lines[LEGEND_ROWS as usize - 3];
    for word in [&b"frame"[..], b"ms", b"KB", b"fps"] {
        assert!(contains(stats_row, word));
    }
    assert!(lines[0].starts_with("\u{256d}".as_bytes()));
    assert!(contains(&lines[0], "\u{256e}".as_bytes()));
    assert!(lines[LEGEND_ROWS as usize - 1].starts_with("\u{2570}".as_bytes()));
    assert!(contains(&lines[LEGEND_ROWS as usize - 1], "\u{256f}".as_bytes()));

    w.sim.config.turning_notch = 0;
    w.sim.config.boundary_notch = 0;
    w.sim.config.pace_notch = LEGEND_BAR_CELLS;
    w.sim.apply_preset_defaults();
    assert_eq!(w.sim.config.turning_notch, DEFAULT_TURNING_NOTCH);
    assert_eq!(w.sim.config.boundary_notch, DEFAULT_NOTCH);
    assert!(w.sim.config.pace_notch == DEFAULT_PACE_NOTCH && w.sim.config.pace == DEFAULT_PACE);

    // The C checks these rows as built before the reset, with the stats set.
    let names = ["boundary", "separation", "alignment", "turning", "perception", "speed"];
    let pairs = ["b/B", "s/S", "a/A", "t/T", "p/P", "v/V"];
    for i in 0..6 {
        assert!(contains(&lines[1 + i], names[i].as_bytes()));
        assert!(contains(&lines[1 + i], pairs[i].as_bytes()));
        assert!(contains(&lines[1 + i], "\u{2591}".as_bytes()));
    }
    assert!(contains(&lines[LEGEND_ROWS as usize - 2], b"quit"));
    for line in &lines[..LEGEND_ROWS as usize] {
        for reversed in ["B/b", "S/s", "A/a", "T/t", "P/p", "R/r", "V/v"] {
            assert!(!contains(line, reversed.as_bytes()));
        }
    }
    let values = ["0.20", "0.005", "1.50", "70\u{b0}", "36px", "1.0\u{d7}"];
    for i in 0..6 {
        assert!(contains(&lines[1 + i], values[i].as_bytes()));
    }
    assert!(contains(&lines[LEGEND_ROWS as usize - 3], b"60fps"));
}

#[test]
fn test_legend_values_follow_their_notch() {
    let mut w = enter!("test_legend_values_follow_their_notch");
    let floors = ["0.01", "0.001", "0.10", "30\u{b0}", "12px", "0.2\u{d7}"];
    let ceilings = ["0.58", "0.013", "4.30", "360\u{b0}", "60px", "2.6\u{d7}"];
    reset_test_config(&mut w);
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);

    let set_all = |sim: &mut Sim, notch: i32| {
        let c = &mut sim.config;
        c.boundary_notch = notch;
        c.separation_notch = notch;
        c.alignment_notch = notch;
        c.turning_notch = notch;
        c.vision_notch = notch;
        c.pace_notch = notch;
        sim.apply_notches();
    };
    set_all(&mut w.sim, 0);
    let lines = legend(&w);
    for i in 0..6 {
        assert!(contains(&lines[1 + i], floors[i].as_bytes()));
        assert_eq!(filled_cells(&lines[1 + i]), 0);
    }
    set_all(&mut w.sim, LEGEND_BAR_CELLS);
    let lines = legend(&w);
    for i in 0..6 {
        assert!(contains(&lines[1 + i], ceilings[i].as_bytes()));
        assert_eq!(filled_cells(&lines[1 + i]), LEGEND_BAR_CELLS as usize);
    }
    reset_test_config(&mut w);
    assert!(feed_input(&mut w, b"B"));
    let lines = legend(&w);
    assert_eq!(filled_cells(&lines[1]), 5);
    assert!(contains(&lines[1], b"0.25"));
}

#[test]
fn test_bar_spans_the_whole_travel() {
    let mut w = enter!("test_bar_spans_the_whole_travel");
    reset_test_config(&mut w);
    assert_eq!(LEGEND_BAR_CELLS, 12);
    for n in 0..=LEGEND_BAR_CELLS {
        let c = &mut w.sim.config;
        c.boundary_notch = n;
        c.separation_notch = n;
        c.alignment_notch = n;
        c.vision_notch = n;
        c.pace_notch = n;
        w.sim.apply_notches();
        let c = &w.sim.config;
        assert!((c.pace - (0.2 + 0.2 * f64::from(n))).abs() < 1e-12);
        if n == 0 {
            assert_eq!(c.pace, PACE_FLOOR);
            assert_eq!(c.boundary, BOUNDARY_MIN);
            assert_eq!(c.separation, SEPARATION_MIN);
            assert_eq!(c.alignment, ALIGNMENT_MIN);
            assert_eq!(c.vision_radius, MIN_VISION_RADIUS);
        }
        if n == LEGEND_BAR_CELLS {
            assert!((c.boundary - BOUNDARY_MAX).abs() < 1e-12);
            assert!((c.alignment - ALIGNMENT_MAX).abs() < 1e-12);
            assert_eq!(c.vision_radius, MAX_VISION_RADIUS);
        }
        assert!(c.vision_cells * SPATIAL_CELL_SIZE >= c.vision_radius);
        assert!((c.vision_cells - 1) * SPATIAL_CELL_SIZE < c.vision_radius);
        assert!(c.vision_cells <= MAX_VISION_CELLS);
    }
    reset_test_config(&mut w);
    let c = &w.sim.config;
    assert!((c.boundary - DEFAULT_BOUNDARY_W).abs() < 1e-12);
    assert!((c.separation - DEFAULT_SEPARATION_W).abs() < 1e-12);
    assert!((c.alignment - DEFAULT_ALIGNMENT_W).abs() < 1e-12);
    assert_eq!(c.pace, 1.0);
    assert_eq!(c.boundary_notch, DEFAULT_NOTCH);
    assert_eq!(c.alignment_notch, DEFAULT_NOTCH);
    assert_eq!(c.vision_radius, DEFAULT_VISION_RADIUS);
}

#[test]
fn test_one_keypress_is_one_cell() {
    let mut w = enter!("test_one_keypress_is_one_cell");
    let sliders = [
        (1, b'B', b'b'),
        (2, b'S', b's'),
        (3, b'A', b'a'),
        (4, b'T', b't'),
        (5, b'P', b'p'),
        (6, b'V', b'v'),
    ];
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    for (row, raise, lower) in sliders {
        reset_test_config(&mut w);
        for _ in 0..=LEGEND_BAR_CELLS {
            assert!(feed_input(&mut w, &[lower]));
        }
        assert_eq!(filled_cells(&legend(&w)[row]), 0);
        for expected in 1..=LEGEND_BAR_CELLS as usize {
            assert!(feed_input(&mut w, &[raise]));
            assert_eq!(filled_cells(&legend(&w)[row]), expected);
        }
        assert!(feed_input(&mut w, &[raise]));
        assert_eq!(filled_cells(&legend(&w)[row]), LEGEND_BAR_CELLS as usize);
        for expected in (0..LEGEND_BAR_CELLS as usize).rev() {
            assert!(feed_input(&mut w, &[lower]));
            assert_eq!(filled_cells(&legend(&w)[row]), expected);
        }
        assert!(feed_input(&mut w, &[lower]));
        assert_eq!(filled_cells(&legend(&w)[row]), 0);
    }
}

#[test]
fn test_weights_stop_at_their_bounds() {
    let mut w = enter!("test_weights_stop_at_their_bounds");
    type Value = fn(&Config) -> f64;
    let weights: [(u8, u8, f64, f64, Value); 4] = [
        (b'B', b'b', BOUNDARY_MAX, BOUNDARY_MIN, |c| c.boundary),
        (b'S', b's', SEPARATION_MAX, SEPARATION_MIN, |c| c.separation),
        (b'A', b'a', ALIGNMENT_MAX, ALIGNMENT_MIN, |c| c.alignment),
        (b'V', b'v', PACE_CEILING, PACE_FLOOR, |c| c.pace),
    ];
    for (raise, lower, ceiling, floor, value) in weights {
        reset_test_config(&mut w);
        assert!(feed_input(&mut w, &[raise; INPUT_BUFFER_SIZE]));
        assert!((value(&w.sim.config) - ceiling).abs() < 1e-12);
        assert!(feed_input(&mut w, &[lower; INPUT_BUFFER_SIZE]));
        assert!((value(&w.sim.config) - floor).abs() < 1e-12);
    }
}

#[test]
fn test_legend_repels_towards_the_nearer_way_out() {
    let mut w = enter!("test_legend_repels_towards_the_nearer_way_out");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);
    let (lw, lh, m) =
        (f64::from(sim.screen.legend_width), f64::from(sim.screen.legend_height), sim.config.speed);

    let right = bird(lw + m - 1.0, lh / 2.0, 0.0);
    assert_eq!(sim.boundary_vector(&right).x, LEGEND_PUSH);
    assert_eq!(sim.boundary_vector(&right).y, 0.0);
    let below = bird(lw / 2.0, lh + m - 1.0, 0.0);
    assert_eq!(sim.boundary_vector(&below).y, LEGEND_PUSH);
    assert_eq!(sim.boundary_vector(&below).x, 0.0);
    let corner = bird(1.0, 1.0, 0.0);
    assert!(lh < lw);
    assert_eq!(sim.boundary_vector(&corner).y, LEGEND_PUSH);
    assert_eq!(sim.boundary_vector(&corner).x, 0.0);

    let clear = bird(lw + m + 1.0, lh + m + 1.0, 0.0);
    let force = sim.boundary_vector(&clear);
    assert!(force.x > 0.0 && force.x < EDGE_FIRM);
    assert!(force.y > 0.0 && force.y < EDGE_FIRM);
    assert!(clear.x < f64::from(sim.screen.turn_x) && clear.y < f64::from(sim.screen.turn_y));

    let middle = bird(f64::from(sim.screen.width) / 2.0, f64::from(sim.screen.height) / 2.0, 0.0);
    let force = sim.boundary_vector(&middle);
    assert!(force.x == 0.0 && force.y == 0.0);

    sim.legend_enabled = false;
    sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);
    assert_eq!(sim.screen.legend_width, 0);
    let force = sim.boundary_vector(&bird(1.0, 1.0, 0.0));
    assert!(force.x > EDGE_FIRM * 0.9 && force.x <= EDGE_FIRM);
    assert!(force.y > EDGE_FIRM * 0.9 && force.y <= EDGE_FIRM);
}

#[test]
fn test_legend_push_overrules_the_flock() {
    let mut w = enter!("test_legend_push_overrules_the_flock");
    const COUNT: usize = 50;
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);
    sim.config.birds = COUNT as i32;
    let first = bird(
        f64::from(sim.screen.legend_width) + 2.0,
        f64::from(sim.screen.legend_height) / 2.0,
        0.0,
    );
    let mut birds = vec![bird(first.x - 4.0, first.y - 4.0, PI); COUNT];
    birds[0] = first;
    let mut grid = grid_for(sim, COUNT);
    build(&mut grid, &birds);
    assert!(sim.flock_direction(&birds, &grid, 0).cos() > 0.999);
    sim.screen.legend_width = 0;
    sim.screen.legend_height = 0;
    assert!(sim.flock_direction(&birds, &grid, 0).cos() < 0.0);
}

#[test]
fn test_no_bird_ever_reaches_the_panel() {
    let mut w = enter!("test_no_bird_ever_reaches_the_panel");
    const FRAMES: usize = 400;
    const DIRECTIONS: i32 = 16;
    let turns = [0, 1, DEFAULT_TURNING_NOTCH, LEGEND_BAR_CELLS];
    let lengths = [1.0, 2.0];
    for turn in turns {
        for length in lengths {
            reset_test_config(&mut w);
            let sim = &mut w.sim;
            sim.config.turning_notch = turn;
            sim.set_frame_seconds(length / f64::from(FRAME_RATE));
            sim.apply_screen_size(200, 50, 200 * 8, 50 * 16);
            sim.config.birds = 1;
            let mut grid = grid_for(sim, 1);
            let margin = sim.config.speed;
            let (lw, lh) =
                (f64::from(sim.screen.legend_width), f64::from(sim.screen.legend_height));
            let mut x = 0.0;
            while x <= lw + margin + 40.0 {
                let mut y = 0.0;
                while y <= lh + margin + 40.0 {
                    for d in 0..DIRECTIONS {
                        let mut b = bird(x, y, 2.0 * PI * f64::from(d) / f64::from(DIRECTIONS));
                        if sim.legend_turn_zone(b.x, b.y) {
                            continue;
                        }
                        for _ in 0..FRAMES {
                            let snapshot = [b];
                            build(&mut grid, &snapshot);
                            let mut moved = [b];
                            sim.update_birds(&mut moved, &snapshot, &grid);
                            b = moved[0];
                            assert!(!sprite_overlaps_legend(sim, b.x, b.y));
                        }
                    }
                    y += 13.0;
                }
                x += 17.0;
            }
        }
    }
}

#[test]
fn test_birds_start_clear_of_the_panel() {
    let mut w = enter!("test_birds_start_clear_of_the_panel");
    const COUNT: usize = 512;
    let mut birds = vec![Bird::default(); COUNT];
    w.sim.rng.seed(20260912);
    for (columns, rows) in [(200, 50), (80, 24), (50, 15)] {
        reset_test_config(&mut w);
        let sim = &mut w.sim;
        sim.apply_screen_size(columns, rows, columns * 8, rows * 16);
        sim.config.birds = COUNT as i32;
        sim.initialize_birds(&mut birds);
        for b in &birds {
            assert!(!sim.legend_turn_zone(b.x, b.y));
            assert!(!sprite_overlaps_legend(sim, b.x, b.y));
        }
    }
}

#[test]
fn test_frame_carries_the_panel() {
    let mut w = enter!("test_frame_carries_the_panel");
    let b = Bird { x: 400.0, y: 300.0, direction: 0.0, frame: 3, ..Bird::default() };
    reset_test_config(&mut w);
    w.sim.config.birds = 1;
    w.renderer.legend_drawn = false;
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    let mut graphics = KittyGraphics::new(1).expect("graphics");
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    let buffer = graphics.buffer();
    let placement = find(buffer, b"a=p").expect("placement");
    let panel = find(buffer, "\x1b[1;1H\u{256d}".as_bytes()).expect("panel");
    let sync_end = find(buffer, b"\x1b[?2026l").expect("sync end");
    assert!(panel > placement);
    assert!(panel < sync_end);
    assert!(w.renderer.legend_drawn);
    for row in 1..=LEGEND_ROWS {
        assert!(contains(buffer, format!("\x1b[{row};1H").as_bytes()));
    }
    graphics.clear();
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(contains(graphics.buffer(), "\u{256d}".as_bytes()));
    assert!(!contains(graphics.buffer(), b"\x1b[K"));
}

#[test]
fn test_panel_switches_off_cleanly() {
    let mut w = enter!("test_panel_switches_off_cleanly");
    let b = Bird { x: 400.0, y: 300.0, direction: 0.0, frame: 3, ..Bird::default() };
    reset_test_config(&mut w);
    w.sim.config.birds = 1;
    w.renderer.legend_drawn = false;
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    let mut graphics = KittyGraphics::new(1).expect("graphics");
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(w.renderer.legend_drawn);

    graphics.clear();
    w.sim.apply_screen_size(30, 24, 30 * 8, 24 * 16);
    assert_eq!(w.sim.screen.legend_width, 0);
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(!w.renderer.legend_drawn);
    assert!(!contains(graphics.buffer(), b"\x1b[2J"));
    assert!(!contains(graphics.buffer(), b"\x1b[3J"));
    for row in 1..=LEGEND_ROWS {
        assert!(contains(graphics.buffer(), format!("\x1b[{row};1H\x1b[K").as_bytes()));
    }

    graphics.clear();
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(!contains(graphics.buffer(), b"\x1b[K"));

    graphics.clear();
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    assert!(w.renderer.legend_drawn);
    assert!(contains(graphics.buffer(), "\u{256d}".as_bytes()));
}

#[test]
fn test_no_legend_leaves_the_corner_to_the_flock() {
    let mut w = enter!("test_no_legend_leaves_the_corner_to_the_flock");
    let b = Bird { x: 400.0, y: 300.0, direction: 0.0, frame: 3, ..Bird::default() };
    reset_test_config(&mut w);
    w.sim.config.birds = 1;
    w.renderer.legend_drawn = false;
    w.sim.legend_enabled = false;
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    assert_eq!(w.sim.screen.legend_width, 0);
    assert_eq!(w.sim.screen.rows, 24);
    assert_eq!(w.sim.screen.height, 24 * 16);
    let force = w.sim.boundary_vector(&bird(1.0, 1.0, 0.0));
    assert!(force.x > 0.0 && force.x <= EDGE_FIRM);
    assert!(force.y > 0.0 && force.y <= EDGE_FIRM);
    let mut graphics = KittyGraphics::new(1).expect("graphics");
    w.renderer.queue_render_frame(&mut graphics, &w.sim, &[b]).expect("frame");
    let buffer = graphics.buffer();
    assert!(!contains(buffer, "\u{256d}".as_bytes()));
    assert!(!contains(buffer, b"\x1b[K"));
    assert!(!contains(buffer, b"\x1b[2J"));
    assert!(contains(buffer, b"a=p"));
}

#[test]
fn test_the_speed_slider_flies_the_same_path_faster() {
    let mut w = enter!("test_the_speed_slider_flies_the_same_path_faster");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.apply_screen_size(200, 60, 1600, 960);
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    let speed = f64::from(DEFAULT_SPEED);
    assert_eq!(sim.config.pace, 1.0);
    assert!(sim.config.speed == speed && sim.config.base_speed == speed);
    let bird_radius = sim.config.speed / sim.turn_limit();
    let hawk_radius = sim.hawk_turning_radius();
    let (mut shortest, mut longest) = (0.0, 0.0);
    for n in 0..=LEGEND_BAR_CELLS {
        sim.config.pace_notch = n;
        sim.apply_notches();
        assert_eq!(sim.config.base_speed, speed);
        assert!((sim.config.speed - speed * sim.config.pace).abs() < 1e-9);
        assert!((sim.config.speed / sim.turn_limit() - bird_radius).abs() < 1e-9);
        assert!((sim.hawk_turning_radius() - hawk_radius).abs() < 1e-9);
        if n == 0 {
            shortest = sim.config.speed;
        }
        if n == LEGEND_BAR_CELLS {
            longest = sim.config.speed;
        }
    }
    assert!(shortest < speed / 4.0 && longest > speed * 2.5);

    for n in [0, LEGEND_BAR_CELLS] {
        let mut leaving = [bird(800.0, 500.0, 0.0)];
        sim.config.pace_notch = n;
        sim.apply_notches();
        sim.config.birds = 1;
        sim.fly_away(&mut leaving);
        assert!(leaving[0].y == 500.0 - speed && leaving[0].x == 800.0);
        assert!((leaving[0].direction - 3.0 * PI / 2.0).abs() < 1e-12);
    }
    sim.config.birds = 800;

    sim.apply_screen_size(40, 14, 320, 224);
    sim.config.pace_notch = DEFAULT_NOTCH;
    sim.apply_notches();
    let capped = sim.config.speed;
    sim.config.pace_notch = LEGEND_BAR_CELLS;
    sim.apply_notches();
    assert!(sim.config.speed > capped * 2.5);

    reset_test_config(&mut w);
    assert!(feed_input(&mut w, b"V"));
    assert!(
        w.sim.config.pace_notch == DEFAULT_NOTCH + 1 && (w.sim.config.pace - 1.2).abs() < 1e-12
    );
    assert!(feed_input(&mut w, b"vv"));
    assert_eq!(w.sim.config.pace_notch, DEFAULT_NOTCH - 1);
    assert!(feed_input(&mut w, b"vvvvvvvvvvvvvvvv"));
    assert!(w.sim.config.pace_notch == 0 && w.sim.config.pace == PACE_FLOOR);
    assert!(feed_input(&mut w, b"0"));
    assert_eq!(w.sim.config.pace, DEFAULT_PACE);
    assert!(feed_input(&mut w, b"VVV\t"));
    assert_eq!(w.sim.config.pace_notch, DEFAULT_PACE_NOTCH + 3);
}

#[test]
fn test_the_default_size_follows_the_renderer() {
    let mut w = enter!("test_the_default_size_follows_the_renderer");
    let sim = &mut w.sim;
    for mode in [
        RenderMode::Unset,
        RenderMode::Kitty,
        RenderMode::Braille,
        RenderMode::Sextants,
        RenderMode::Blocks,
    ] {
        sim.render_mode = mode;
        sim.config.bird_size = 0;
        sim.settle_the_bird_size();
        assert!(sim.config.bird_size == DEFAULT_BIRD_SIZE && DEFAULT_BIRD_SIZE == 30);
        assert_eq!(sim.hawk_sprite_size(), 2 * DEFAULT_BIRD_SIZE);
    }
    sim.render_mode = RenderMode::Braille;
    sim.config.bird_size = 60;
    sim.settle_the_bird_size();
    assert_eq!(sim.config.bird_size, 60);
    sim.config.bird_size = MAX_BIRD_SIZE;
    assert_eq!(sim.hawk_sprite_size(), 2 * MAX_BIRD_SIZE);
}

#[test]
fn test_the_speed_is_a_flag() {
    let mut w = enter!("test_the_speed_is_a_flag");
    reset_test_config(&mut w);
    let mut program = rbirds::app::Program {
        sim: w.sim.clone(),
        settings: w.settings.clone(),
        renderer: Default::default(),
    };
    let mut stdout = CStdout::captured();
    assert!(
        read_options(
            &mut program,
            &args(&["cbirds", "--preset", "storm", "--speed", "9"]),
            &mut stdout
        )
        .is_ok()
    );
    let c = &program.sim.config;
    assert!(c.pace_notch == 9 && (c.pace - 2.0).abs() < 1e-12);
    assert_eq!(c.boundary_notch, PRESETS[2].notch[0]);
    assert_eq!(c.separation_notch, PRESETS[2].notch[1]);
}

#[test]
fn test_a_hawk_holds_a_chase_for_a_distance() {
    let mut w = enter!("test_a_hawk_holds_a_chase_for_a_distance");
    let birds = [bird(1500.0, 700.0, 0.0)];
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = false;
    sim.apply_screen_size(200, 50, 1600, 800);
    sim.config.birds = 1;
    sim.config.hawks = 1;
    for notch in [DEFAULT_NOTCH, 9] {
        sim.config.pace_notch = notch;
        sim.apply_notches();
        sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
        sim.hawks[0] = Hawk { x: 300.0, y: 300.0, prey: 0, commitment: 0.5, ..Hawk::default() };
        sim.hunt(&birds);
        assert_eq!(sim.hawks[0].prey, 0);
        assert!(
            (sim.hawks[0].commitment - (0.5 - sim.config.pace / f64::from(FRAME_RATE))).abs()
                < 1e-12
        );
        sim.hawks[0] = Hawk { x: 300.0, y: 300.0, prey: -1, passing: 0.2, ..Hawk::default() };
        sim.hunt(&birds);
        assert!(
            (sim.hawks[0].passing - (0.2 - sim.config.pace / f64::from(FRAME_RATE))).abs() < 1e-12
        );
    }
}

/// How many birds are outside the frame over a run (the C's
/// `share_off_the_screen`), starting from a reset each time as the C does.
fn share_off_the_screen(
    w: &mut World,
    pace_notch: i32,
    columns: i32,
    rows: i32,
    panel: bool,
) -> f64 {
    const BIRDS: usize = 300;
    const SECONDS: i32 = 12;
    reset_test_config(w);
    let sim = &mut w.sim;
    sim.config.birds = BIRDS as i32;
    sim.config.pace_notch = pace_notch;
    sim.legend_enabled = panel;
    sim.apply_notches();
    sim.apply_screen_size(columns, rows, columns * 8, rows * 16);
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    let mut grid = grid_for(sim, BIRDS);
    let mut birds = vec![Bird::default(); BIRDS];
    let mut snapshot = vec![Bird::default(); BIRDS];
    sim.rng.seed(7);
    sim.initialize_birds(&mut birds);
    let (mut outside, mut counted) = (0_i64, 0_i64);
    for frame in 0..SECONDS * FRAME_RATE {
        snapshot.copy_from_slice(&birds);
        build(&mut grid, &snapshot);
        sim.fly(&mut birds, &mut snapshot, &mut grid);
        assert_eq!(sim.frame_seconds, 1.0 / f64::from(FRAME_RATE));
        if frame < 2 * FRAME_RATE {
            continue;
        }
        for b in &birds {
            counted += 1;
            if b.x < 0.0
                || b.y < 0.0
                || b.x >= f64::from(sim.screen.width)
                || b.y >= f64::from(sim.screen.height)
            {
                outside += 1;
            }
            assert!(!sprite_overlaps_legend(sim, b.x, b.y));
        }
    }
    w.sim.legend_enabled = true;
    reset_test_config(w);
    outside as f64 / counted as f64
}

#[test]
fn test_a_fast_flock_is_flown_in_steps() {
    let mut w = enter!("test_a_fast_flock_is_flown_in_steps");
    reset_test_config(&mut w);
    let sim = &mut w.sim;
    sim.legend_enabled = false;
    sim.apply_screen_size(200, 60, 1600, 960);
    sim.config.birds = 1;
    sim.config.pace_notch = LEGEND_BAR_CELLS;
    sim.apply_notches();
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    let frame_step = sim.config.speed;
    let mut grid = grid_for(sim, 1);
    let mut b = [bird(800.0, 480.0, 0.0)];
    let mut snapshot = b;
    build(&mut grid, &snapshot);
    sim.fly(&mut b, &mut snapshot, &mut grid);
    assert!((b[0].x - (800.0 + frame_step)).abs() < 1e-9 && (b[0].y - 480.0).abs() < 1e-9);
    assert_eq!(sim.config.speed, frame_step);
    sim.legend_enabled = true;

    for (columns, rows, panel) in [(200, 50, false), (80, 24, false), (76, 22, true)] {
        let shipped = share_off_the_screen(&mut w, DEFAULT_NOTCH, columns, rows, panel);
        let slow = share_off_the_screen(&mut w, 0, columns, rows, panel);
        let fast = share_off_the_screen(&mut w, LEGEND_BAR_CELLS, columns, rows, panel);
        assert!(fast <= shipped * 1.5 + 0.005);
        assert!(slow <= shipped * 1.5 + 0.005);
    }
}

#[test]
fn test_the_avoidance_slider_needs_two_flocks() {
    let mut w = enter!("test_the_avoidance_slider_needs_two_flocks");
    reset_test_config(&mut w);
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    assert_eq!(w.sim.legend_rows(), LEGEND_ROWS);
    assert_eq!(w.sim.screen.legend_height, LEGEND_ROWS * w.sim.screen.cell_height);
    for line in legend(&w) {
        assert!(!contains(&line, b"avoidance"));
    }
    assert!(feed_input(&mut w, b"GGg"));
    assert_eq!(w.sim.config.avoid_notch, DEFAULT_NOTCH);

    w.sim.config.flocks = 3;
    w.sim.apply_screen_size(80, 24, 80 * 8, 24 * 16);
    assert_eq!(w.sim.legend_rows(), LEGEND_MAX_ROWS);
    assert_eq!(w.sim.screen.legend_height, LEGEND_MAX_ROWS * w.sim.screen.cell_height);
    let lines = legend(&w);
    for line in &lines {
        assert_eq!(legend_cells(line), LEGEND_COLUMNS as usize);
    }
    assert!(contains(&lines[7], b"avoidance") && contains(&lines[7], b"g/G"));
    assert!(
        contains(&lines[7], "1.00\u{d7}".as_bytes())
            && filled_cells(&lines[7]) == DEFAULT_NOTCH as usize
    );
    let rows = LEGEND_MAX_ROWS as usize;
    assert!(contains(&lines[rows - 3], b"fps"));
    assert!(contains(&lines[rows - 2], b"quit"));
    assert!(lines[rows - 1].starts_with("\u{2570}".as_bytes()));

    for expected in DEFAULT_NOTCH + 1..=LEGEND_BAR_CELLS {
        assert!(feed_input(&mut w, b"G"));
        assert_eq!(filled_cells(&legend(&w)[7]), expected as usize);
    }
    assert!(feed_input(&mut w, b"G"));
    assert_eq!(w.sim.config.avoid_notch, LEGEND_BAR_CELLS);
    assert!(contains(&legend(&w)[7], "3.00\u{d7}".as_bytes()));
    assert!(feed_input(&mut w, b"gggggggggggggggg"));
    assert_eq!(w.sim.config.avoid_notch, 0);
    let lines = legend(&w);
    assert!(contains(&lines[7], "0.00\u{d7}".as_bytes()) && filled_cells(&lines[7]) == 0);
    assert!(feed_input(&mut w, b"0"));
    assert_eq!(w.sim.config.avoid_notch, DEFAULT_NOTCH);

    let sim = &mut w.sim;
    let (mut room, mut weight, mut kinship) = (-1.0, -1.0, 2.0);
    for n in 0..=LEGEND_BAR_CELLS {
        sim.config.avoid_notch = n;
        sim.apply_notches();
        let c = &sim.config;
        assert!(c.avoid_room >= room && c.avoid_weight >= weight);
        assert!(c.avoid_kinship < kinship || c.avoid_kinship == 0.0);
        (room, weight, kinship) = (c.avoid_room, c.avoid_weight, c.avoid_kinship);
        if n < DEFAULT_NOTCH {
            assert!(c.avoid_kinship == 2.0_f64.powi(-n) && room == 0.0 && weight == 0.0);
        } else {
            assert_eq!(c.avoid_kinship, 0.0);
        }
    }
    assert!(sim.config.avoid_room == 2.0 && sim.config.avoid_weight == AVOID_WEIGHT_MAX);
    sim.config.avoid_notch = DEFAULT_NOTCH;
    sim.apply_notches();
    assert!(
        sim.config.avoid_room == 0.0
            && sim.config.avoid_weight == 0.0
            && sim.config.avoid_kinship == 0.0
    );
    assert_eq!(sim.flock_room(), 0.0);
    sim.apply_screen_size(200, 60, 1600, 960);
    sim.config.avoid_notch = 8;
    sim.apply_notches();
    assert_eq!(sim.flock_room(), 2.0 * f64::from(FLOCK_LEASH));
    sim.config.avoid_notch = 0;
    sim.apply_notches();
    assert_eq!(sim.flock_pace(2), 1.0);
}

#[test]
fn test_the_avoidance_is_a_flag() {
    let mut w = enter!("test_the_avoidance_is_a_flag");
    reset_test_config(&mut w);
    let mut program = rbirds::app::Program {
        sim: w.sim.clone(),
        settings: w.settings.clone(),
        renderer: Default::default(),
    };
    let mut stdout = CStdout::captured();
    assert!(
        read_options(
            &mut program,
            &args(&["cbirds", "--flocks", "3", "--avoidance", "10"]),
            &mut stdout
        )
        .is_ok()
    );
    let c = &program.sim.config;
    assert!(c.flocks == 3 && c.avoid_notch == 10);
    assert!(c.avoid_weight > 0.0 && c.avoid_room > 1.0);
}

struct FlocksApart {
    contact: f64,
    gap: f64,
    outside: f64,
}

/// The C's `three_flocks_at`.
fn three_flocks_at(
    w: &mut World,
    avoid_notch: i32,
    columns: i32,
    rows: i32,
    panel: bool,
) -> FlocksApart {
    const BIRDS: usize = 300;
    let seconds = 20;
    reset_test_config(w);
    let sim = &mut w.sim;
    sim.config.birds = BIRDS as i32;
    sim.config.flocks = 3;
    sim.config.avoid_notch = avoid_notch;
    sim.legend_enabled = panel;
    sim.apply_notches();
    sim.apply_screen_size(columns, rows, columns * 8, rows * 16);
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    sim.flock_home_x = [0.0; 3];
    sim.flock_home_y = [0.0; 3];
    let mut grid = grid_for(sim, BIRDS);
    let mut birds = vec![Bird::default(); BIRDS];
    let mut snapshot = vec![Bird::default(); BIRDS];
    sim.rng.seed(5);
    sim.initialize_birds(&mut birds);
    let (mut contact, mut sampled, mut outside, mut counted) = (0_i64, 0_i64, 0_i64, 0_i64);
    let (mut gap, mut gaps) = (0.0, 0);
    for frame in 0..seconds * FRAME_RATE {
        snapshot.copy_from_slice(&birds);
        build(&mut grid, &snapshot);
        sim.fly(&mut birds, &mut snapshot, &mut grid);
        if frame < 5 * FRAME_RATE {
            continue;
        }
        let mut least = -1.0_f64;
        for f in 0..3 {
            for g in f + 1..3 {
                let dx = sim.flock_center_x[f] - sim.flock_center_x[g];
                let dy = sim.flock_center_y[f] - sim.flock_center_y[g];
                let distance = (dx * dx + dy * dy).sqrt();
                if least < 0.0 || distance < least {
                    least = distance;
                }
            }
        }
        gap += least;
        gaps += 1;
        for b in &birds {
            counted += 1;
            if b.x < 0.0
                || b.y < 0.0
                || b.x >= f64::from(sim.screen.width)
                || b.y >= f64::from(sim.screen.height)
            {
                outside += 1;
            }
            assert!(!sprite_overlaps_legend(sim, b.x, b.y));
        }
        if frame % 20 != 0 {
            continue;
        }
        for i in (0..BIRDS).step_by(3) {
            sampled += 1;
            for j in 0..BIRDS {
                if birds[j].flock == birds[i].flock {
                    continue;
                }
                let dx = birds[i].x - birds[j].x;
                let dy = birds[i].y - birds[j].y;
                if dx * dx + dy * dy < f64::from(sim.config.vision_radius_squared) {
                    contact += 1;
                    break;
                }
            }
        }
    }
    w.sim.legend_enabled = true;
    reset_test_config(w);
    FlocksApart {
        contact: contact as f64 / sampled as f64,
        gap: gap / f64::from(gaps),
        outside: outside as f64 / counted as f64,
    }
}

#[test]
fn test_flocks_avoid_each_other_as_much_as_asked() {
    let mut w = enter!("test_flocks_avoid_each_other_as_much_as_asked");
    let mingle = three_flocks_at(&mut w, 0, 200, 50, false);
    let shipped = three_flocks_at(&mut w, DEFAULT_NOTCH, 200, 50, false);
    let shun = three_flocks_at(&mut w, LEGEND_BAR_CELLS, 200, 50, false);
    assert!(mingle.contact > shipped.contact && shipped.contact > shun.contact);
    assert!(shun.contact < 0.02);
    assert!(mingle.gap < shipped.gap && shipped.gap < shun.gap);
    assert!(shun.outside <= shipped.outside * 3.0 + 0.005);
    let small_shipped =
        three_flocks_at(&mut w, DEFAULT_NOTCH, LEGEND_MIN_COLS, LEGEND_MIN_ROWS, true);
    let small_shun =
        three_flocks_at(&mut w, LEGEND_BAR_CELLS, LEGEND_MIN_COLS, LEGEND_MIN_ROWS, true);
    three_flocks_at(&mut w, 0, LEGEND_MIN_COLS, LEGEND_MIN_ROWS, true);
    assert!(small_shun.outside <= small_shipped.outside * 1.5 + 0.005);
}
