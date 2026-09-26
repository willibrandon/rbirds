//! The live run: the terminal taken, the flock drawn at sixty frames a
//! second on the monotonic clock, keys read between frames and while output
//! is blocked, and a snapshot written once the terminal is back. Translated
//! from the live half of cbirds `boids.c` `main` and `write_snapshot`.
//!
//! The loop body is [`LiveLoop::frame`], which takes the keys read, the
//! frame's timestamp and the window size as arguments rather than reading
//! them itself, so the same body the terminal runs can be driven with
//! injected time and input against the reference.

#![forbid(unsafe_code)]

use crate::platform::OsStrExt;
use std::ffi::OsStr;
use std::time::{Duration, Instant};

use crate::app::{EXIT_FAILURE, EXIT_SUCCESS, Program};
use crate::config::*;
use crate::image::{Image, PngError, png};
use crate::input::InputParser;
use crate::platform::{self, STDIN_FILENO, Timespec, WinSize};
use crate::render::Renderer;
use crate::render::compose::{compose_onto, text_style, upload_sprite_sets};
use crate::render::kitty::{KittyError, KittyGraphics};
use crate::simulation::{Bird, RenderMode, Sim};
use crate::spatial_grid::{SpatialGrid, status_string};
use crate::sprites::{PICTURE_GROUND, SpriteError, empty_catalogue, free_sprites};
use crate::stdio::{CStdout, cat, eprint};
use crate::terminal::{self, Terminal};
use crate::timing::{FramePacer, FrameProfile, Trace};

/// `handle_input`'s read: at most INPUT_BUFFER_SIZE bytes, whatever is there.
pub fn read_keys(sim: &mut Sim, parser: &mut InputParser) -> bool {
    if platform::exit_requested() {
        return false;
    }
    let mut input = [0_u8; INPUT_BUFFER_SIZE];
    match platform::read(STDIN_FILENO, &mut input) {
        Ok(length) => sim.handle_input(parser, Some(&input[..length])),
        Err(_) => sim.handle_input(parser, None),
    }
}

/// `elapsed_seconds`.
pub fn elapsed_seconds(start: &Timespec, end: &Timespec) -> f64 {
    (end.tv_sec - start.tv_sec) as f64 + (end.tv_nsec - start.tv_nsec) as f64 / 1e9
}

/// `elapsed_microseconds`.
pub fn elapsed_microseconds(start: &Timespec, end: &Timespec) -> i64 {
    (end.tv_sec - start.tv_sec) * 1_000_000 + (end.tv_nsec - start.tv_nsec) / 1000
}

/// The per-second averages the panel shows, over the frame just drawn.
pub fn account_frame(renderer: &mut Renderer, seconds: f64, frame_microseconds: i64, bytes: usize) {
    let stats = &mut renderer.stats;
    stats.window_ms += frame_microseconds as f64 / 1000.0;
    stats.window_bytes += bytes as f64;
    stats.counted += 1;
    if seconds - stats.window_started >= 1.0 {
        let span = seconds - stats.window_started;
        stats.frame_ms = stats.window_ms / stats.counted as f64;
        stats.bytes = stats.window_bytes / stats.counted as f64;
        stats.rate = stats.counted as f64 / span;
        stats.window_started = seconds;
        stats.window_ms = 0.0;
        stats.window_bytes = 0.0;
        stats.counted = 0;
    }
}

/// `write_snapshot`: a picture of what was on the screen. `Err` carries the
/// sprite loader's own fatal message, which ends the run as in the C.
pub fn write_snapshot(
    sim: &mut Sim,
    renderer: &Renderer,
    path: &OsStr,
    birds: &[Bird],
    sprite_path: Option<&OsStr>,
    program: &[u8],
) -> Result<bool, Vec<u8>> {
    let canvas: Result<Image, PngError> = if sim.drawing_with_text() {
        renderer
            .cells
            .paint(
                text_style(sim.render_mode),
                sim.screen.cell_width,
                sim.screen.cell_height,
                PICTURE_GROUND,
            )
            .map_err(|_| PngError::Memory)
    } else {
        let mut frames = empty_catalogue();
        match sim.rasterise_sprites(&mut frames, sprite_path, program) {
            Err(SpriteError::Fatal(message)) => return Err(message),
            Err(SpriteError::Png(error)) => Err(error),
            Ok(()) => Image::alloc(sim.screen.width, sim.screen.height).map(|mut canvas| {
                compose_onto(sim, &mut canvas, &frames, birds, true);
                canvas
            }),
        }
    };
    let Ok(encoded) = canvas.and_then(|canvas| png::encode(&canvas)) else { return Ok(false) };
    // A full disk may only say so when the file is closed.
    Ok(platform::write_file(path, &encoded).is_ok())
}

/// Where the loop body left the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// Queued in the output buffer, to be flushed.
    Drawn,
    /// The flight out is over: leave the loop without drawing.
    Over,
}

/// The loop's own state (`live_birds`, `leaving`, the clocks, the parser).
#[derive(Clone, Debug)]
pub struct LiveLoop {
    pub parser: InputParser,
    pub started: Timespec,
    pub previous_frame: Timespec,
    pub live_birds: i32,
    /// Seconds left of the flight out.
    pub leaving: f64,
}

impl LiveLoop {
    pub fn new(started: Timespec, live_birds: i32) -> LiveLoop {
        LiveLoop {
            parser: InputParser::default(),
            started,
            previous_frame: started,
            live_birds,
            leaving: 0.0,
        }
    }

    /// One pass of the C main loop from `handle_input` to the queued frame.
    /// `keys` is what the read returned (`None` for nothing or a failure),
    /// `frame_start` the monotonic clock just after it, and `window` what
    /// TIOCGWINSZ reports (zero where it fails). `Err` is the exit code, the
    /// message already written.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        sim: &mut Sim,
        renderer: &mut Renderer,
        graphics: &mut KittyGraphics,
        birds: &mut Vec<Bird>,
        snapshot: &mut Vec<Bird>,
        grid: &mut SpatialGrid,
        keys: Option<&[u8]>,
        frame_start: Timespec,
        window: WinSize,
    ) -> Result<Frame, i32> {
        if !sim.handle_input(&mut self.parser, keys) && self.leaving <= 0.0 {
            // Asked to quit: fly off the top first.
            self.leaving = f64::from(OUTRO_FRAMES_AT_SIXTY) / f64::from(FRAME_RATE);
            sim.formation.clear();
        }
        sim.set_frame_seconds(elapsed_seconds(&self.previous_frame, &frame_start));
        self.previous_frame = frame_start;
        if self.leaving > 0.0 {
            self.leaving -= sim.frame_seconds;
            if self.leaving <= 0.0 {
                return Ok(Frame::Over);
            }
        }
        sim.clock.frame += 1;
        sim.clock.seconds = elapsed_seconds(&self.started, &frame_start);
        // Writing lets go when its hold is up.
        sim.release_the_formation_if_due();
        sim.maybe_drift();
        terminal::apply_window_size(sim, window);
        if let Err(error) = grid.prepare(sim.screen.width, sim.screen.height, sim.config.birds) {
            return Err(grid_failure("Cannot resize spatial grid", error));
        }
        if sim.population_changed {
            sim.population_changed = false;
            if sim.resize_the_flock(birds, snapshot, self.live_birds, sim.config.birds) {
                self.live_birds = sim.config.birds;
            } else {
                // Keep what we have rather than lose it.
                sim.config.birds = self.live_birds;
            }
            if let Err(error) = grid.prepare(sim.screen.width, sim.screen.height, sim.config.birds)
            {
                return Err(grid_failure("Cannot resize spatial grid", error));
            }
        }
        if let Err(error) = sim.snapshot_and_build(birds, snapshot, grid) {
            return Err(grid_failure("Cannot build spatial grid", error));
        }
        if self.leaving > 0.0 {
            sim.fly_away(birds);
        } else {
            sim.advance(birds, snapshot, grid);
        }
        if let Some(profile) = &mut renderer.profile {
            profile.updated();
        }
        let rendered = renderer.queue_render_frame(graphics, sim, birds);
        if let Some(profile) = &mut renderer.profile {
            profile.rendered();
        }
        if let Err(error) = rendered {
            let renderer = if sim.render_mode == RenderMode::Sixel { "Sixel" } else { "Kitty" };
            return Err(fail(
                format!("Cannot render {renderer} graphics: {}\n", kitty_status(error)).as_bytes(),
            ));
        }
        Ok(Frame::Drawn)
    }
}

/// The errno an OS error carries, as `perror` would report it.
fn errno_of(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(platform::EIO)
}

fn fail(message: &[u8]) -> i32 {
    eprint(message);
    EXIT_FAILURE
}

fn grid_failure(what: &str, error: crate::spatial_grid::GridError) -> i32 {
    fail(format!("{what}: {}\n", status_string(Err(error))).as_bytes())
}

fn kitty_status(error: KittyError) -> &'static str {
    crate::render::kitty::kitty_graphics_status_string(Err(error))
}

/// Writes the queued frame, reading keys whenever the terminal pushes back.
/// `Ok(false)` is a `q` read while blocked, which leaves without the outro.
fn flush_frame(
    sim: &mut Sim,
    parser: &mut InputParser,
    graphics: &mut KittyGraphics,
    name: &[u8],
) -> Result<bool, i32> {
    let mut running = true;
    while running && !graphics.is_empty() {
        match graphics.flush_nonblocking() {
            Ok(()) => {}
            Err(KittyError::Again) => {
                if let Err(error) = terminal::wait_for_terminal_io() {
                    return Err(fail(&platform::perror_message(
                        b"Cannot wait for terminal output",
                        errno_of(&error),
                    )));
                }
                running = read_keys(sim, parser);
            }
            Err(KittyError::Io(errno)) => {
                // Whatever the renderer; a reader that went away is the
                // usual reason.
                return Err(fail(&cat(&[
                    name,
                    b": cannot write to the terminal: ",
                    &platform::strerror(errno),
                    b"\n",
                ])));
            }
            Err(error) => {
                return Err(fail(&cat(&[
                    name,
                    b": cannot write to the terminal: ",
                    kitty_status(error).as_bytes(),
                    b"\n",
                ])));
            }
        }
    }
    Ok(running)
}

/// The live half of `main`. The terminal is restored when this returns,
/// whichever way it returns.
pub fn run(program: &mut Program, _stdout: &mut CStdout) -> i32 {
    let Program { sim, settings, renderer } = program;
    match run_live(sim, renderer, settings) {
        Ok(code) | Err(code) => code,
    }
}

fn run_live(
    sim: &mut Sim,
    renderer: &mut Renderer,
    settings: &crate::app::Settings,
) -> Result<i32, i32> {
    let name = settings.program_name.clone();
    let sprite_path = settings.sprite_path.as_deref();
    let mut trace = Trace::from_environment()
        .map_err(|error| fail(format!("Cannot start live trace: {error}\n").as_bytes()))?;
    platform::install_signal_handlers();

    // The terminal is asked its questions before anything is built for it.
    let mut terminal = Terminal::enter().map_err(|error| {
        fail(&platform::perror_message(b"Can't enable raw mode", errno_of(&error)))
    })?;
    sim.render_mode = sim.live_render_mode();
    sim.settle_the_bird_size();
    if sim.palette_follows_the_theme() && !terminal::learn_the_theme(&mut sim.theme) {
        sim.config.palette = crate::palette::fallback_palette();
    }
    let sixel_cell = if sim.render_mode == RenderMode::Sixel {
        Some(
            terminal::prepare_sixel()
                .map_err(|error| fail(format!("Cannot enable Sixel: {error}\n").as_bytes()))?,
        )
    } else {
        None
    };

    let mut grid = SpatialGrid::new(SPATIAL_CELL_SIZE)
        .map_err(|error| grid_failure("Cannot initialize spatial grid", error))?;
    // A named seed makes a run repeatable.
    let seed = if settings.requested_seed >= 0 {
        settings.requested_seed as u32
    } else {
        platform::time_now() as u32
    };
    sim.rng.seed(seed);
    terminal::apply_window_size(
        sim,
        terminal::graphics_window(
            platform::window_size_or_zero(platform::STDOUT_FILENO),
            sixel_cell,
        ),
    );
    grid.prepare(sim.screen.width, sim.screen.height, sim.config.birds)
        .map_err(|error| grid_failure("Cannot prepare spatial grid", error))?;
    // The sprites, once, as pixels.
    let built = if sim.drawing_with_text() {
        renderer.prepare_text_renderer(sim, sprite_path, &name)
    } else {
        sim.rasterise_sprites(&mut renderer.sprites, sprite_path, &name)
    };
    match built {
        Ok(()) => {}
        Err(SpriteError::Fatal(message)) => return Err(fail(&message)),
        Err(SpriteError::Png(_)) => {
            return Err(fail(&cat(&[&name, b": cannot build the sprites to draw with\n"])));
        }
    }
    sim.set_frame_seconds(1.0 / f64::from(FRAME_RATE));
    sim.hawk_sets_built = true;

    let count = sim.config.birds.max(0) as usize;
    let mut birds: Vec<Bird> = Vec::new();
    let mut snapshot: Vec<Bird> = Vec::new();
    if birds.try_reserve_exact(count).is_err() || snapshot.try_reserve_exact(count).is_err() {
        return Err(fail(&platform::perror_message(b"Out of memory", platform::ENOMEM)));
    }
    birds.resize(count, Bird::default());
    snapshot.resize(count, Bird::default());
    let mut graphics = KittyGraphics::new(platform::STDOUT_FILENO).map_err(|error| {
        fail(format!("Cannot initialize Kitty graphics: {}\n", kitty_status(error)).as_bytes())
    })?;

    terminal.enter_alt_screen();
    terminal::write_all(b"\x1b[J");
    terminal::apply_window_size(
        sim,
        terminal::graphics_window(
            platform::window_size_or_zero(platform::STDOUT_FILENO),
            sixel_cell,
        ),
    );
    sim.initialize_birds(&mut birds);
    sim.place_hawks();
    sim.begin_the_intro();
    if sim.render_mode == RenderMode::Kitty {
        terminal.mark_sprites_uploaded();
        let uploaded = upload_sprite_sets(sim, &mut graphics, &renderer.sprites);
        free_sprites(&mut renderer.sprites);
        if let Err(error) = uploaded {
            return Err(fail(
                format!("Cannot upload Kitty graphics: {}\n", kitty_status(error)).as_bytes(),
            ));
        }
    }

    let mut live = LiveLoop::new(platform::monotonic_now(), sim.config.birds);
    if let Some(trace) = &mut trace {
        trace
            .begin(
                sim.render_mode.name(),
                [sim.screen.cols, sim.screen.rows, sim.screen.width, sim.screen.height],
            )
            .map_err(|error| fail(format!("Cannot start live trace: {error}\n").as_bytes()))?;
    }
    let mut pacer = FramePacer::new(Instant::now());
    let mut sleeper = platform::FrameSleeper::new();
    let mut input = [0_u8; INPUT_BUFFER_SIZE];
    loop {
        if platform::exit_requested() {
            return Ok(130);
        }
        renderer.profile = trace.as_ref().map(|_| {
            let mut profile = FrameProfile::new();
            if !settings.unlock_fps {
                profile.target = pacer.target();
            }
            profile
        });
        let keys = platform::read(STDIN_FILENO, &mut input).ok();
        let frame_start = platform::monotonic_now();
        let window = terminal::graphics_window(
            platform::window_size_or_zero(platform::STDOUT_FILENO),
            sixel_cell,
        );
        let keys = keys.map(|length| &input[..length]);
        // The clock is read after the keys, as the C reads it; the window a
        // moment later, which no step in between depends on.
        let drawn = live.frame(
            sim,
            renderer,
            &mut graphics,
            &mut birds,
            &mut snapshot,
            &mut grid,
            keys,
            frame_start,
            window,
        )?;
        if drawn == Frame::Over {
            break;
        }
        let frame_bytes = graphics.len();
        if !flush_frame(sim, &mut live.parser, &mut graphics, &name)? {
            break;
        }
        let flushed = Instant::now();
        let last = settings.frame_limit > 0 && sim.clock.frame >= i64::from(settings.frame_limit);
        let frame_end = platform::monotonic_now();
        account_frame(
            renderer,
            sim.clock.seconds,
            elapsed_microseconds(&frame_start, &frame_end),
            frame_bytes,
        );
        let delay =
            if settings.unlock_fps || last { Duration::ZERO } else { pacer.delay_after(flushed) };
        if let Some(trace) = &mut trace {
            trace.record(
                renderer.profile.unwrap(),
                flushed,
                delay,
                frame_bytes,
                keys.map_or(0, <[u8]>::len),
            );
        }
        if last {
            break;
        }
        sleeper.sleep(delay);
    }
    // A Ctrl event may also interrupt a blocked frame flush, which exits the
    // loop before its next iteration can observe the cancellation flag.
    if platform::exit_requested() {
        return Ok(130);
    }
    if let Some(trace) = trace {
        terminal.restore();
        trace
            .finish()
            .map_err(|error| fail(format!("Cannot finish live trace: {error}\n").as_bytes()))?;
    }
    // A snapshot asked for and not written is a failed run.
    let mut outcome = EXIT_SUCCESS;
    if let Some(path) = settings.snapshot_path.as_deref() {
        terminal.restore();
        match write_snapshot(sim, renderer, path, &birds, sprite_path, &name) {
            Ok(true) => eprint(&cat(&[&name, b": wrote ", path.as_bytes(), b"\n"])),
            Ok(false) => {
                eprint(&cat(&[&name, b": could not write ", path.as_bytes(), b"\n"]));
                outcome = EXIT_FAILURE;
            }
            Err(message) => return Err(fail(&message)),
        }
    }
    Ok(outcome)
}
