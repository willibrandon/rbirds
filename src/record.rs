//! Recording, headless: a GIF of the composed frames, or an asciinema cast of
//! the braille renderer's escape text, both on the recording's own clock so a
//! seed gives the same file however long a frame takes to make. Translated
//! from cbirds `boids.c` (`record_delay_for`, `write_json_string`,
//! `run_cast_recording`, `run_recording`).

#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::os::unix::ffi::OsStrExt;

use crate::app::{EXIT_FAILURE, EXIT_SUCCESS, Settings};
use crate::cfmt;
use crate::config::*;
use crate::fp::mul_add;
use crate::image::gif::{GifError, GifWriter};
use crate::image::{Image, PngError};
use crate::platform;
use crate::render::Renderer;
use crate::render::cells::{Cells, CellsStyle};
use crate::render::compose::{compose_onto, text_style};
use crate::simulation::{Bird, RenderMode, Sim};
use crate::spatial_grid::SpatialGrid;
use crate::sprites::{PICTURE_GROUND, SpriteError, empty_catalogue};
use crate::stdio::{CStdout, cat, eprint};

/// `record_delay_for`: hundredths a frame, chosen by the error in the rate,
/// not by rounding the delay.
pub fn record_delay_for(fps: i32) -> i32 {
    let mut best = 100 / MAX_RECORD_FPS;
    let mut error = -1.0_f64;
    for delay in 100 / MAX_RECORD_FPS..=100 {
        let mut mistake = 100.0 / f64::from(delay) - f64::from(fps);
        if mistake < 0.0 {
            mistake = -mistake;
        }
        if error < 0.0 || mistake < error {
            error = mistake;
            best = delay;
        }
    }
    best
}

/// `write_json_string`'s escaping: quote and backslash escaped, control
/// characters spelled out, everything else, braille included, as it is.
pub fn json_escape_into(out: &mut Vec<u8>, text: &[u8]) {
    for &c in text {
        if c == b'"' || c == b'\\' {
            out.push(b'\\');
            out.push(c);
        } else if c < 0x20 {
            out.extend_from_slice(format!("\\u{c:04x}").as_bytes());
        } else {
            out.push(c);
        }
    }
}

/// `write_json_string`.
pub fn json_string(text: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    json_escape_into(&mut out, text);
    out.push(b'"');
    out
}

/// The C file a recording writes through: buffered, with the first failure
/// kept for `fclose` to report, as stdio keeps its error flag.
struct CFile {
    writer: BufWriter<File>,
    error: Option<std::io::Error>,
}

impl CFile {
    fn create(path: &OsStr) -> std::io::Result<CFile> {
        Ok(CFile { writer: BufWriter::new(File::create(path)?), error: None })
    }

    fn put(&mut self, bytes: &[u8]) {
        if self.error.is_none()
            && let Err(error) = self.writer.write_all(bytes)
        {
            self.error = Some(error);
        }
    }

    /// `fclose(out) == 0`, and the errno it would leave.
    fn close(mut self) -> Result<(), std::io::Error> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        self.writer.flush()?;
        let file = self.writer.into_inner().map_err(|e| e.into_error())?;
        drop(file);
        Ok(())
    }
}

fn os_error_text(error: &std::io::Error) -> Vec<u8> {
    platform::strerror(error.raw_os_error().unwrap_or(platform::EIO))
}

/// `name_length > 5 && strcmp(path + name_length - 5, ".cast") == 0`.
pub fn names_a_cast(path: &OsStr) -> bool {
    let name = path.as_bytes();
    // A C string ends at its first NUL; argv cannot contain one.
    name.len() > 5 && name.ends_with(b".cast")
}

/// The loop body every recording shares: the clock, the intro's release,
/// the autopilot, and one frame of flight.
pub fn record_step(
    sim: &mut Sim,
    birds: &mut [Bird],
    snapshot: &mut [Bird],
    grid: &mut SpatialGrid,
    frame: i32,
    fps: f64,
) {
    sim.clock.frame = i64::from(frame);
    sim.clock.seconds = f64::from(frame) / fps;
    sim.release_the_formation_if_due();
    sim.maybe_drift();
    let _ = sim.snapshot_and_build(birds, snapshot, grid);
    sim.fly(birds, snapshot, grid);
}

fn allocate_flock(count: i32) -> Option<(Vec<Bird>, Vec<Bird>)> {
    let length = count.max(0) as usize;
    let mut birds = Vec::new();
    let mut snapshot = Vec::new();
    if birds.try_reserve_exact(length).is_err() || snapshot.try_reserve_exact(length).is_err() {
        return None;
    }
    birds.resize(length, Bird::default());
    snapshot.resize(length, Bird::default());
    Some((birds, snapshot))
}

fn sprite_failure(error: SpriteError, program: &[u8]) -> i32 {
    match error {
        SpriteError::Fatal(message) => eprint(&message),
        SpriteError::Png(_) => {
            eprint(&cat(&[program, b": cannot build the sprites to record with\n"]));
        }
    }
    EXIT_FAILURE
}

/// `run_cast_recording`: the recording as text, one line of braille escape
/// text a frame.
pub fn run_cast_recording(
    sim: &mut Sim,
    renderer: &mut Renderer,
    settings: &Settings,
    stdout: &mut CStdout,
) -> i32 {
    let program = settings.program_name.as_slice();
    let Some(record_path) = settings.record_path.as_deref() else { return EXIT_FAILURE };
    let total = settings.record_fps * settings.record_seconds;
    sim.settle_the_palette_without_a_terminal();
    sim.set_frame_seconds(1.0 / f64::from(settings.record_fps));
    sim.legend_enabled = false;
    sim.render_mode = RenderMode::Braille;
    sim.settle_the_bird_size();
    sim.apply_screen_size(
        settings.record_columns,
        settings.record_rows,
        settings.record_columns * DEFAULT_CELL_WIDTH,
        settings.record_rows * DEFAULT_CELL_HEIGHT,
    );
    let Ok(mut grid) = SpatialGrid::new(SPATIAL_CELL_SIZE) else { return EXIT_FAILURE };
    if grid.prepare(sim.screen.width, sim.screen.height, sim.config.birds).is_err() {
        return EXIT_FAILURE;
    }
    let sprite_path = settings.sprite_path.as_deref();
    if let Err(error) = renderer.prepare_text_renderer(sim, sprite_path, program) {
        return sprite_failure(error, program);
    }
    if !renderer.text_renderer_fits_the_screen(sim) {
        return sprite_failure(SpriteError::Png(PngError::Memory), program);
    }
    // A cast is played in somebody else's terminal: 24 bit colour.
    renderer.cells.truecolor = true;

    let Some((mut birds, mut snapshot)) = allocate_flock(sim.config.birds) else {
        return EXIT_FAILURE;
    };
    let mut out = match CFile::create(record_path) {
        Ok(out) => out,
        Err(error) => {
            eprint(&cat(&[
                program,
                b": ",
                record_path.as_bytes(),
                b": ",
                &os_error_text(&error),
                b"\n",
            ]));
            return EXIT_FAILURE;
        }
    };
    out.put(
        format!(
            "{{\"version\": 2, \"width\": {}, \"height\": {}, \"timestamp\": {}, \
             \"title\": \"rbirds\", \"env\": {{\"TERM\": \"xterm-256color\", \"SHELL\": \
             \"/bin/sh\"}}}}\n",
            sim.screen.cols,
            sim.screen.rows,
            platform::time_now()
        )
        .as_bytes(),
    );
    sim.rng.seed(if settings.requested_seed >= 0 { settings.requested_seed as u32 } else { 1 });
    sim.initialize_birds(&mut birds);
    sim.place_hawks();
    sim.begin_the_intro();

    // Hidden cursor and a clean slate first; the pen put back at the end.
    out.put(b"[0, \"o\", ");
    out.put(&json_string(b"\x1b[?25l\x1b[2J"));
    out.put(b"]\n");

    let mut bytes: i64 = 0;
    let mut line = Vec::new();
    for frame in 0..total {
        record_step(
            sim,
            &mut birds,
            &mut snapshot,
            &mut grid,
            frame,
            f64::from(settings.record_fps),
        );
        compose_onto(sim, &mut renderer.canvas, &renderer.sprites, &birds, false);
        renderer.cells.read(
            CellsStyle::Braille,
            &renderer.canvas,
            sim.screen.cell_width,
            sim.screen.cell_height,
        );
        if renderer.cells.emit().is_err() {
            break;
        }
        // Inside a synchronized update, for the players that honour it.
        line.clear();
        line.push(b'[');
        line.extend_from_slice(cfmt::fixed(sim.clock.seconds, 4).as_bytes());
        line.extend_from_slice(b", \"o\", \"\\u001b[?2026h");
        json_escape_into(&mut line, &renderer.cells.text);
        line.extend_from_slice(b"\\u001b[?2026l\"]\n");
        out.put(&line);
        bytes += renderer.cells.text.len() as i64;
    }
    out.put(b"[");
    out.put(cfmt::fixed(f64::from(total) / f64::from(settings.record_fps), 4).as_bytes());
    out.put(b", \"o\", ");
    out.put(&json_string(b"\x1b[0m\x1b[?25h"));
    out.put(b"]\n");
    let closed = out.close();
    // A clip shorter than the intro would leave it writing.
    sim.formation.clear();

    renderer.cells = Cells::default();
    renderer.canvas.free();
    crate::sprites::free_sprites(&mut renderer.sprites);
    if let Err(error) = closed {
        eprint(&cat(&[
            program,
            b": ",
            record_path.as_bytes(),
            b": ",
            &os_error_text(&error),
            b"\n",
        ]));
        return EXIT_FAILURE;
    }
    let mut summary = record_path.as_bytes().to_vec();
    summary.extend_from_slice(
        format!(
            ": {} frames, {}x{} cells, {} fps, {}s, {} KB of braille\n",
            total,
            sim.screen.cols,
            sim.screen.rows,
            settings.record_fps,
            cfmt::fixed(f64::from(total) / f64::from(settings.record_fps), 1),
            cfmt::fixed(bytes as f64 / 1024.0, 1)
        )
        .as_bytes(),
    );
    stdout.print(&summary);
    EXIT_SUCCESS
}

/// `run_recording`: a GIF, or a cast when the name says so.
pub fn run_recording(
    sim: &mut Sim,
    renderer: &mut Renderer,
    settings: &Settings,
    stdout: &mut CStdout,
) -> i32 {
    let program = settings.program_name.as_slice();
    let Some(record_path) = settings.record_path.as_deref() else { return EXIT_FAILURE };
    if names_a_cast(record_path) {
        return run_cast_recording(sim, renderer, settings, stdout);
    }
    sim.settle_the_palette_without_a_terminal();
    let delay = record_delay_for(settings.record_fps);
    // The rate a hundredth-of-a-second delay really gives.
    let actual_fps = 100.0 / f64::from(delay);
    // fma: boids.c:3683:51
    let total = mul_add(actual_fps, f64::from(settings.record_seconds), 0.5) as i32;
    sim.set_frame_seconds(1.0 / actual_fps);
    // A GIF has no panel in it, and keeps no corner for one.
    sim.legend_enabled = false;
    sim.apply_screen_size(
        settings.record_columns,
        settings.record_rows,
        settings.record_columns * DEFAULT_CELL_WIDTH,
        settings.record_rows * DEFAULT_CELL_HEIGHT,
    );
    let Ok(mut grid) = SpatialGrid::new(SPATIAL_CELL_SIZE) else { return EXIT_FAILURE };
    if grid.prepare(sim.screen.width, sim.screen.height, sim.config.birds).is_err() {
        return EXIT_FAILURE;
    }
    let mut frames = empty_catalogue();
    if let Err(error) = sim.rasterise_sprites(&mut frames, settings.sprite_path.as_deref(), program)
    {
        return sprite_failure(error, program);
    }
    let Ok(mut canvas) = Image::alloc(sim.screen.width, sim.screen.height) else {
        return EXIT_FAILURE;
    };
    // Under a text renderer the GIF is of the cells, painted as a terminal
    // shows them.
    let as_text = sim.drawing_with_text();
    let mut cells = Cells::default();
    if as_text {
        match Cells::new(true) {
            Ok(fresh) => cells = fresh,
            Err(_) => return EXIT_FAILURE,
        }
        if cells.resize(sim.screen.cols, sim.screen.rows).is_err() {
            return EXIT_FAILURE;
        }
    }

    let mut gif = match GifWriter::open(
        std::path::Path::new(record_path),
        sim.screen.width,
        sim.screen.height,
        delay,
    ) {
        Ok(gif) => gif,
        Err(error) => {
            eprint(&cat(&[
                program,
                b": ",
                record_path.as_bytes(),
                b": ",
                error.as_str().as_bytes(),
                b"\n",
            ]));
            return EXIT_FAILURE;
        }
    };

    let Some((mut birds, mut snapshot)) = allocate_flock(sim.config.birds) else {
        return EXIT_FAILURE;
    };
    sim.rng.seed(if settings.requested_seed >= 0 { settings.requested_seed as u32 } else { 1 });
    sim.initialize_birds(&mut birds);
    sim.place_hawks();
    sim.begin_the_intro();

    let mut gif_status: Result<(), GifError> = Ok(());
    for frame in 0..total {
        if gif_status.is_err() {
            break;
        }
        record_step(sim, &mut birds, &mut snapshot, &mut grid, frame, actual_fps);
        if as_text {
            compose_onto(sim, &mut canvas, &frames, &birds, false);
            let style = text_style(sim.render_mode);
            cells.read(style, &canvas, sim.screen.cell_width, sim.screen.cell_height);
            let painted = match cells.emit() {
                Ok(()) => cells.paint(
                    style,
                    sim.screen.cell_width,
                    sim.screen.cell_height,
                    PICTURE_GROUND,
                ),
                Err(_) => Err(crate::render::cells::CellsError::Memory),
            };
            match painted {
                Ok(painted) => gif_status = gif.add_frame(&painted),
                Err(_) => {
                    gif_status = Err(GifError::Memory);
                    break;
                }
            }
        } else {
            compose_onto(sim, &mut canvas, &frames, &birds, true);
            gif_status = gif.add_frame(&canvas);
        }
    }
    sim.formation.clear();

    let closed = gif.close();
    if gif_status.is_ok() {
        gif_status = closed.status;
    }
    if let Err(error) = gif_status {
        eprint(&cat(&[
            program,
            b": ",
            record_path.as_bytes(),
            b": ",
            error.as_str().as_bytes(),
            b"\n",
        ]));
        return EXIT_FAILURE;
    }
    let written = closed.frames;
    let mut summary = record_path.as_bytes().to_vec();
    summary.extend_from_slice(
        format!(
            ": {} frames, {}x{}, {} fps, {}s, {} KB\n",
            written,
            sim.screen.width,
            sim.screen.height,
            cfmt::general(actual_fps, 4),
            cfmt::fixed(f64::from(written) / actual_fps, 1),
            cfmt::fixed(closed.bytes as f64 / 1024.0, 1)
        )
        .as_bytes(),
    );
    stdout.print(&summary);
    if delay != record_delay_for(settings.record_fps)
        || (actual_fps + 0.5) as i32 != settings.record_fps
    {
        let rate = cfmt::general(actual_fps, 4);
        eprint(&cat(&[
            program,
            format!(
                ": asked for {} fps, recorded at {rate}. A GIF's delay between frames is\n\
                 whole hundredths of a second, so the only rates it has are 100/1, 100/2,\n\
                 100/3 and so on, and viewers clamp anything under two hundredths up to a\n\
                 tenth. {rate} is the nearest rate this format can actually carry.\n",
                settings.record_fps
            )
            .as_bytes(),
        ]));
    }
    EXIT_SUCCESS
}
