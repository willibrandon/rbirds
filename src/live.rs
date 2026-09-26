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

use crate::app::{EXIT_FAILURE, EXIT_SUCCESS, Program, frame_delay_after};
use crate::config::*;
use crate::image::{Image, PngError, png};
use crate::input::InputParser;
use crate::platform::{self, PollFd, STDIN_FILENO, Timespec, WinSize};
use crate::render::Renderer;
use crate::render::compose::{compose_onto, text_style, upload_sprite_sets};
use crate::render::kitty::{KittyError, KittyGraphics};
use crate::simulation::{Bird, RenderMode, Sim};
use crate::spatial_grid::{SpatialGrid, status_string};
use crate::sprites::{PICTURE_GROUND, SpriteError, empty_catalogue, free_sprites};
use crate::stdio::{CStdout, cat, eprint};
use crate::terminal::{self, Terminal};

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

/// Big-flock mode's check that the terminal has caught up (docs/DEVIATIONS.md
/// D-006). A frame ends with a device status request, and the next frame is
/// worked out while the terminal reads this one but is not written until the
/// terminal has answered. However much a frame holds and however slowly the
/// terminal reads, it is never more than a frame behind, so frames arrive as
/// fast as it can take them and no faster. The answers are taken out of the
/// input before the keys are read from it.
#[derive(Clone, Debug)]
pub struct Pacing {
    /// Whether the terminal answered at startup. Without that, frames go out
    /// as they do in cbirds.
    pub on: bool,
    /// Whether frames wait for answers: until one takes longer than
    /// ANSWER_MILLISECONDS, after which the terminal is taken not to answer
    /// any more, and frames go out as in cbirds. Answers are still taken
    /// out of the input.
    waits: bool,
    /// Requests counted and not yet answered.
    unanswered: u32,
    /// When the last request had been written.
    sent_at: Timespec,
    /// How much of an answer the input read so far ended with.
    held: usize,
}

/// Device status report: "are you there?"
pub const STATUS_REQUEST: &[u8] = b"\x1b[5n";
/// "Yes", sent once everything before the request has been read.
pub const STATUS_ANSWER: &[u8] = b"\x1b[0n";
/// How long to wait for an answer before sending the next frame anyway.
const ANSWER_MILLISECONDS: i64 = 1000;
/// Room for the keys of one read of INPUT_BUFFER_SIZE bytes and the start of
/// an answer held from the read before, which may turn out to be keys.
pub const KEYS_ROOM: usize = INPUT_BUFFER_SIZE + STATUS_ANSWER.len() - 1;

impl Pacing {
    /// Pacing that is `on` or not.
    pub fn new(on: bool) -> Pacing {
        Pacing { on, waits: on, unanswered: 0, sent_at: Timespec::default(), held: 0 }
    }

    /// Sorts one read's `input`, at most INPUT_BUFFER_SIZE bytes, into
    /// answers, which are counted off, and keys, which go into `keys`;
    /// returns how many keys. An answer split across reads is still one
    /// answer, and bytes that start one and then don't finish it are keys
    /// after all, so no byte read is lost.
    pub fn keys_from(&mut self, input: &[u8], keys: &mut [u8; KEYS_ROOM]) -> usize {
        assert!(input.len() <= INPUT_BUFFER_SIZE, "one read at a time");
        let mut length = 0;
        for &byte in input {
            if byte == STATUS_ANSWER[self.held] {
                self.held += 1;
                if self.held == STATUS_ANSWER.len() {
                    self.held = 0;
                    self.unanswered = self.unanswered.saturating_sub(1);
                    platform::mark_answer_outstanding(self.unanswered > 0);
                }
                continue;
            }
            for &started in &STATUS_ANSWER[..self.held] {
                keys[length] = started;
                length += 1;
            }
            self.held = 0;
            if byte == STATUS_ANSWER[0] {
                self.held = 1;
            } else {
                keys[length] = byte;
                length += 1;
            }
        }
        length
    }

    /// Counts a request about to go out, before anything that writes it can
    /// also read its answer.
    pub fn requested(&mut self) {
        self.unanswered = self.unanswered.saturating_add(1);
        platform::mark_answer_outstanding(true);
    }

    /// The frame and its request have been written: the wait for the answer
    /// starts now.
    pub fn sent(&mut self, at: Timespec) {
        self.sent_at = at;
    }

    /// Whether a request is still unanswered.
    pub fn waiting(&self) -> bool {
        self.unanswered > 0
    }

    /// Reads until every request is answered, handling the keys as they are
    /// read, as keys read while output is blocked are. `false` is a q, which
    /// leaves at once as it does then, or a request to exit. Stops waiting,
    /// for good, once an answer is ANSWER_MILLISECONDS late, and for now when
    /// the terminal has gone.
    pub fn wait(&mut self, sim: &mut Sim, parser: &mut InputParser) -> bool {
        while self.waits && self.unanswered > 0 {
            if platform::exit_requested() {
                return false;
            }
            let spent = elapsed_microseconds(&self.sent_at, &platform::monotonic_now()) / 1000;
            if spent >= ANSWER_MILLISECONDS {
                self.waits = false;
                return true;
            }
            let mut ready = [PollFd::new(STDIN_FILENO, platform::POLLIN)];
            match platform::poll(&mut ready, (ANSWER_MILLISECONDS - spent) as i32) {
                Err(error) if error.raw_os_error() == Some(platform::EINTR) => continue,
                Err(_) => return true,
                Ok(_) => {}
            }
            let mut input = [0_u8; INPUT_BUFFER_SIZE];
            let Ok(got) = platform::read(STDIN_FILENO, &mut input) else { return true };
            if got == 0 && ready[0].revents & !platform::POLLIN != 0 {
                // Hung up: the frame's write will say so.
                return true;
            }
            let mut keys = [0_u8; KEYS_ROOM];
            let length = self.keys_from(&input[..got], &mut keys);
            if !sim.handle_input(parser, Some(&keys[..length])) {
                return false;
            }
        }
        true
    }
}

/// `read_keys` with the terminal's answers taken out first.
fn read_keys_paced(sim: &mut Sim, parser: &mut InputParser, pacing: &mut Pacing) -> bool {
    if platform::exit_requested() {
        return false;
    }
    let mut input = [0_u8; INPUT_BUFFER_SIZE];
    let Ok(got) = platform::read(STDIN_FILENO, &mut input) else {
        return sim.handle_input(parser, None);
    };
    let mut keys = [0_u8; KEYS_ROOM];
    let length = pacing.keys_from(&input[..got], &mut keys);
    sim.handle_input(parser, Some(&keys[..length]))
}

/// Sleeps until `microseconds` after `from`, a millisecond at a time. One
/// long sleep can end several milliseconds late: macOS lets a timer fire up
/// to about a third of its length after it was due, so the remainder of a
/// fast frame, 14 ms, would end 5 to 7 ms late and the next frame with it.
/// Slices of a millisecond end within half a millisecond of the time.
pub fn sleep_until(from: &Timespec, microseconds: i64) {
    loop {
        if platform::exit_requested() {
            return;
        }
        let left = microseconds - elapsed_microseconds(from, &platform::monotonic_now());
        if left <= 0 {
            return;
        }
        let _ = platform::nanosleep(&Timespec { tv_sec: 0, tv_nsec: left.min(1000) * 1000 });
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
        let rendered = if self.leaving > 0.0 {
            sim.fly_away(birds);
            renderer.queue_render_frame(graphics, sim, birds)
        } else {
            sim.advance(birds, snapshot, grid);
            renderer.queue_render_frame(graphics, sim, birds)
        };
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
    pacing: &mut Pacing,
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
                running = if pacing.on {
                    read_keys_paced(sim, parser, pacing)
                } else {
                    read_keys(sim, parser)
                };
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
    // A big flock is paced by the terminal, if it answers.
    let mut pacing = Pacing::new(sim.big_flock && terminal::answers_status());

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
    let mut input = [0_u8; INPUT_BUFFER_SIZE];
    let mut paced_keys = [0_u8; KEYS_ROOM];
    loop {
        if platform::exit_requested() {
            return Ok(130);
        }
        let keys = platform::read(STDIN_FILENO, &mut input).ok();
        let frame_start = platform::monotonic_now();
        let window = terminal::graphics_window(
            platform::window_size_or_zero(platform::STDOUT_FILENO),
            sixel_cell,
        );
        let keys = match keys {
            Some(length) if pacing.on => {
                let length = pacing.keys_from(&input[..length], &mut paced_keys);
                Some(&paced_keys[..length])
            }
            keys => keys.map(|length| &input[..length]),
        };
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
        if pacing.on {
            // The request goes after the frame; the frame waits for the
            // answer to the one before it. A q while it waits leaves at once.
            if let Err(error) = graphics.write_raw(STATUS_REQUEST) {
                return Err(fail(
                    format!("Cannot queue terminal output: {}\n", kitty_status(error)).as_bytes(),
                ));
            }
            if !pacing.wait(sim, &mut live.parser) {
                break;
            }
            // Counted before the write, which may read the answer itself.
            pacing.requested();
        }
        if !flush_frame(sim, &mut live.parser, &mut graphics, &name, &mut pacing)? {
            break;
        }
        if pacing.on {
            pacing.sent(platform::monotonic_now());
        }
        if settings.frame_limit > 0 && sim.clock.frame >= i64::from(settings.frame_limit) {
            break;
        }
        let frame_end = platform::monotonic_now();
        account_frame(
            renderer,
            sim.clock.seconds,
            elapsed_microseconds(&frame_start, &frame_end),
            frame_bytes,
        );
        let remaining =
            frame_delay_after(settings.unlock_fps, elapsed_microseconds(&frame_start, &frame_end));
        if remaining > 0 {
            sleep_until(&frame_end, remaining);
        }
    }
    // A Ctrl event may also interrupt a blocked frame flush, which exits the
    // loop before its next iteration can observe the cancellation flag.
    if platform::exit_requested() {
        return Ok(130);
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
