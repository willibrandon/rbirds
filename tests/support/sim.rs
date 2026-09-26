//! The Rust half of the simulation oracle: runs the script language of
//! `tools/oracle/sim_oracle.c` through the Rust port and prints the same
//! "rbirds-sim-trace 1" format, so the two transcripts can be compared line
//! for line. Every command calls the port's own entry points — `record_step`,
//! `bench_step`, `LiveLoop::frame` — never a reimplementation.

use std::fmt::Write as _;

use rbirds::live::{Frame, LiveLoop, account_frame};
use rbirds::platform::{Timespec, WinSize};
use rbirds::render::Renderer;
use rbirds::render::kitty::{KittyError, KittyGraphics};
use rbirds::simulation::{Bird, RenderMode, Sim};
use rbirds::spatial_grid::SpatialGrid;

/// The port's state as the C oracle's globals hold it.
pub struct World {
    pub sim: Sim,
    pub renderer: Renderer,
    pub birds: Vec<Bird>,
    pub snapshot: Vec<Bird>,
    pub allocated: usize,
    pub grid: Option<SpatialGrid>,
    pub graphics: Option<KittyGraphics>,
    pub live: LiveLoop,
    out: String,
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn from_bits(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex bits"))
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    if hex == "-" {
        return Vec::new();
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() && out.len() < (1 << 16) {
        let pair = std::str::from_utf8(&bytes[i..i + 2]).expect("ascii");
        out.push(u8::from_str_radix(pair, 16).unwrap_or(0));
        i += 2;
    }
    out
}

fn kitty_code(result: Result<(), KittyError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(KittyError::Argument) => 1,
        Err(KittyError::Memory) => 2,
        Err(KittyError::Io(_)) => 3,
        Err(KittyError::Again) => 4,
    }
}

fn grid_code(result: Result<(), rbirds::spatial_grid::GridError>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(rbirds::spatial_grid::GridError::Argument) => 1,
        Err(rbirds::spatial_grid::GridError::Memory) => 2,
    }
}

fn fnv1a(text: &[u8]) -> u64 {
    let mut hash: u64 = 1469598103934665603;
    for &byte in text {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    hash
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

impl World {
    pub fn new() -> World {
        World {
            sim: Sim::new(),
            renderer: Renderer::default(),
            birds: Vec::new(),
            snapshot: Vec::new(),
            allocated: 0,
            grid: None,
            graphics: None,
            live: LiveLoop::new(Timespec { tv_sec: 0, tv_nsec: 0 }, 0),
            out: String::new(),
        }
    }

    fn graphics(&mut self) -> &mut KittyGraphics {
        self.graphics.get_or_insert_with(|| KittyGraphics::new(1).expect("graphics"))
    }

    /// The C oracle's `handle_keys`: one read of at most INPUT_BUFFER_SIZE.
    fn keys(&mut self, hex: &str) -> bool {
        let bytes = hex_bytes(hex);
        let taken = &bytes[..bytes.len().min(rbirds::config::INPUT_BUFFER_SIZE)];
        self.sim.handle_input(&mut self.live.parser, Some(taken))
    }

    fn emit_buffer(&mut self, label: &str, status: i32) {
        let buffer = self.graphics().buffer().to_vec();
        let _ = write!(self.out, "{label} {status} {} ", buffer.len());
        for byte in buffer {
            let _ = write!(self.out, "{byte:02x}");
        }
        self.out.push('\n');
    }

    fn set(&mut self, field: &str, value: i32) {
        let sim = &mut self.sim;
        let config = &mut sim.config;
        match field {
            "birds" => config.birds = value,
            "size" => config.bird_size = value,
            "palette" => config.palette = value,
            "flocks" => config.flocks = value,
            "trails" => config.trails = value != 0,
            "hawks" => config.hawks = value,
            "shape" => config.shape = value,
            "turning" => config.turning_notch = value,
            "boundary" => config.boundary_notch = value,
            "separation" => config.separation_notch = value,
            "alignment" => config.alignment_notch = value,
            "vision" => config.vision_notch = value,
            "pace" => config.pace_notch = value,
            "avoid" => config.avoid_notch = value,
            "legend" => sim.legend_enabled = value != 0,
            "render" => sim.render_mode = RenderMode::from_index(value),
            "deep" => sim.deep_look = value != 0,
            "rain" => sim.rain = value != 0,
            "paused" => sim.paused = value != 0,
            "hawk_sets" => sim.hawk_sets_built = value != 0,
            "preset_index" => sim.requested_preset = value,
            "truecolor" => self.renderer.cells.truecolor = value != 0,
            other => panic!("unknown field {other}"),
        }
    }

    pub fn dump(&mut self) -> String {
        let sim = &self.sim;
        let c = &sim.config;
        let s = &sim.screen;
        let mut out = String::new();
        out.push_str("rbirds-sim-trace 1\n");
        let _ = writeln!(
            out,
            "config birds={} size={} palette={} flocks={} trails={} hawks={} shape={} turning={}",
            c.birds,
            c.bird_size,
            c.palette,
            c.flocks,
            i32::from(c.trails),
            c.hawks,
            c.shape,
            c.turning_notch
        );
        let _ = writeln!(
            out,
            "config speed={} base={} pace={}",
            bits(c.speed),
            bits(c.base_speed),
            bits(c.pace)
        );
        let _ = writeln!(
            out,
            "config vision cells={} radius={} squared={}",
            c.vision_cells, c.vision_radius, c.vision_radius_squared
        );
        let _ = writeln!(
            out,
            "config weights separation={} alignment={} boundary={}",
            bits(c.separation),
            bits(c.alignment),
            bits(c.boundary)
        );
        let _ = writeln!(
            out,
            "config notches boundary={} separation={} alignment={} vision={} pace={} avoid={}",
            c.boundary_notch,
            c.separation_notch,
            c.alignment_notch,
            c.vision_notch,
            c.pace_notch,
            c.avoid_notch
        );
        let _ = writeln!(
            out,
            "config avoid kinship={} room={} weight={}",
            bits(c.avoid_kinship),
            bits(c.avoid_room),
            bits(c.avoid_weight)
        );
        let _ = writeln!(
            out,
            "screen {} {} {} {} {} {} {} {} {} {} {}",
            s.width,
            s.height,
            s.cols,
            s.rows,
            s.cell_width,
            s.cell_height,
            s.turn_x,
            s.turn_y,
            s.turn_bottom,
            s.legend_width,
            s.legend_height
        );
        let _ = writeln!(
            out,
            "state legend={} render={} deep={} rain={} paused={} step={} population={}",
            i32::from(sim.legend_enabled),
            sim.render_mode as i32,
            i32::from(sim.deep_look),
            i32::from(sim.rain),
            i32::from(sim.paused),
            i32::from(sim.step_once),
            i32::from(sim.population_changed)
        );
        let _ = writeln!(
            out,
            "state frame_seconds={} clock={},{}",
            bits(sim.frame_seconds),
            sim.clock.frame,
            bits(sim.clock.seconds)
        );
        let _ = writeln!(
            out,
            "state mouse={},{},{} last_key={} last_drift={}",
            i32::from(sim.mouse.present),
            bits(sim.mouse.x),
            bits(sim.mouse.y),
            bits(sim.last_key_at),
            bits(sim.last_drift_at)
        );
        let _ = write!(out, "state konami_at={} seen=", sim.konami.at);
        for byte in sim.konami.seen {
            let _ = write!(out, "{byte:02x}");
        }
        let _ = write!(
            out,
            " preset={} hawk_sets={} theme_known={} theme=",
            sim.requested_preset,
            i32::from(sim.hawk_sets_built),
            i32::from(sim.theme.known)
        );
        for tint in sim.theme.tints {
            let _ = write!(out, "{:02x}{:02x}{:02x}", tint[0], tint[1], tint[2]);
        }
        out.push('\n');
        let st = &self.renderer.stats;
        let _ = writeln!(
            out,
            "stats frame_ms={} bytes={} rate={} counted={} started={} window_ms={} \
             window_bytes={} legend_drawn={} text_legend={}",
            bits(st.frame_ms),
            bits(st.bytes),
            bits(st.rate),
            st.counted,
            bits(st.window_started),
            bits(st.window_ms),
            bits(st.window_bytes),
            i32::from(self.renderer.legend_drawn),
            i32::from(self.renderer.text_legend_was_drawn)
        );
        let _ = write!(out, "rng front={} rear={} words=", sim.rng.front, sim.rng.rear);
        for word in sim.rng.word {
            let _ = write!(out, "{word:08x}");
        }
        out.push('\n');
        let f = &sim.formation;
        let _ = writeln!(
            out,
            "formation count={} writing={} until={}",
            f.count,
            i32::from(f.writing),
            bits(f.until)
        );
        for i in 0..f.count as usize {
            let _ = writeln!(out, "target {i} {} {}", bits(f.x[i]), bits(f.y[i]));
        }
        for i in 0..3 {
            let _ = writeln!(
                out,
                "flock {i} center={},{} home={},{} leash={}",
                bits(sim.flock_center_x[i]),
                bits(sim.flock_center_y[i]),
                bits(sim.flock_home_x[i]),
                bits(sim.flock_home_y[i]),
                bits(sim.flock_leash[i])
            );
        }
        for (i, h) in sim.hawks.iter().enumerate() {
            let _ = writeln!(
                out,
                "hawk {i} {} {} {} frame={} prey={} commitment={} passing={} wing={} clock={}",
                bits(h.x),
                bits(h.y),
                bits(h.direction),
                h.frame,
                h.prey,
                bits(h.commitment),
                bits(h.passing),
                h.wing,
                bits(h.wing_clock)
            );
        }
        for (i, b) in self.birds[..self.allocated].iter().enumerate() {
            let _ = writeln!(
                out,
                "bird {i} {} {} {} frame={} shade={} flock={} layer={} wing={} clock={} glide={} \
                 trail={},{},{}/{},{},{} at={} held={}",
                bits(b.x),
                bits(b.y),
                bits(b.direction),
                b.frame,
                b.shade,
                b.flock,
                b.layer,
                b.wing,
                bits(b.wing_clock),
                bits(b.gliding),
                bits(b.trail_x[0]),
                bits(b.trail_x[1]),
                bits(b.trail_x[2]),
                bits(b.trail_y[0]),
                bits(b.trail_y[1]),
                bits(b.trail_y[2]),
                b.trail_at,
                b.trail_held
            );
        }
        if let Some(grid) = &self.grid {
            let _ = write!(
                out,
                "grid columns={} rows={} cells={} capacity={} offsets=",
                grid.columns, grid.rows, grid.cell_count, grid.item_capacity
            );
            let mut hash: u64 = 1469598103934665603;
            for i in 0..=grid.cell_count as usize {
                hash = (hash ^ u64::from(grid.offsets[i] as u32)).wrapping_mul(1099511628211);
            }
            let indexed = (sim.config.birds.max(0) as usize).min(grid.item_capacity as usize);
            for i in 0..indexed {
                hash = (hash ^ u64::from(grid.indices[i] as u32)).wrapping_mul(1099511628211);
            }
            let _ = writeln!(out, "{hash:016x}");
        }
        out
    }

    /// Runs one command line; the transcript accumulates in [`World::take`].
    pub fn command(&mut self, line: &str) {
        let word: Vec<&str> = line.split_whitespace().collect();
        if word.is_empty() || line.starts_with('#') {
            return;
        }
        let arg = |i: usize| word.get(i).copied().unwrap_or("");
        let int = |i: usize| arg(i).parse::<i64>().unwrap_or(0);
        match word[0] {
            "screen" => self.sim.apply_screen_size(
                int(1) as i32,
                int(2) as i32,
                int(3) as i32,
                int(4) as i32,
            ),
            "set" => self.set(arg(1), int(2) as i32),
            "notches" => self.sim.apply_notches(),
            "defaults" => self.sim.apply_preset_defaults(),
            "preset" => self.sim.apply_preset(int(1) as i32),
            "seconds" => self.sim.set_frame_seconds(from_bits(arg(1))),
            "seed" => self.sim.rng.seed(arg(1).parse::<u32>().unwrap_or(0)),
            "alloc" => {
                self.allocated = int(1) as usize;
                self.birds = vec![Bird::default(); self.allocated];
                self.snapshot = vec![Bird::default(); self.allocated];
            }
            "init" => self.sim.initialize_birds(&mut self.birds),
            "hawks" => self.sim.place_hawks(),
            "intro" => self.sim.begin_the_intro(),
            "grid" => {
                let (w, h, n) = (self.sim.screen.width, self.sim.screen.height, self.sim.config.birds);
                let grid = self
                    .grid
                    .get_or_insert_with(|| SpatialGrid::new(rbirds::config::SPATIAL_CELL_SIZE).unwrap());
                let code = grid_code(grid.prepare(w, h, n));
                let _ = writeln!(self.out, "grid {code}");
            }
            "record" => {
                let grid = self.grid.as_mut().expect("grid before record");
                rbirds::record::record_step(
                    &mut self.sim,
                    &mut self.birds,
                    &mut self.snapshot,
                    grid,
                    int(1) as i32,
                    from_bits(arg(2)),
                );
            }
            "bench" => {
                let grid = self.grid.as_mut().expect("grid before bench");
                rbirds::bench::bench_step(&mut self.sim, &mut self.birds, &mut self.snapshot, grid);
            }
            "fly" => {
                let grid = self.grid.as_mut().expect("grid before fly");
                let _ = self.sim.snapshot_and_build(&self.birds, &mut self.snapshot, grid);
                self.sim.fly(&mut self.birds, &mut self.snapshot, grid);
            }
            "clock" => {
                self.sim.clock.frame = int(1);
                self.sim.clock.seconds = from_bits(arg(2));
            }
            "keys" => {
                let result = self.keys(arg(1));
                let _ = writeln!(self.out, "keys {}", i32::from(result));
            }
            "theme" => {
                let accent = [int(1) as u8, int(2) as u8, int(3) as u8];
                let ground = [int(4) as u8, int(5) as u8, int(6) as u8];
                self.sim.theme.ramp_between(accent, ground);
                self.sim.theme.known = true;
            }
            "resize" => {
                let to = int(1) as i32;
                let from = self.sim.config.birds;
                self.sim.config.birds = to;
                assert!(self.sim.resize_the_flock(&mut self.birds, &mut self.snapshot, from, to));
                self.allocated = to as usize;
            }
            "sprites" => {
                let ok = if self.sim.drawing_with_text() {
                    self.renderer.prepare_text_renderer(&mut self.sim, None, b"rbirds").is_ok()
                } else {
                    self.sim.rasterise_sprites(&mut self.renderer.sprites, None, b"rbirds").is_ok()
                };
                let _ = writeln!(self.out, "sprites {}", i32::from(ok));
            }
            "render" => {
                self.graphics().clear();
                let mut graphics = self.graphics.take().expect("graphics");
                let status =
                    self.renderer.queue_render_frame(&mut graphics, &self.sim, &self.birds);
                self.graphics = Some(graphics);
                self.emit_buffer("render", kitty_code(status));
            }
            "live_begin" => {
                self.graphics();
                self.live = LiveLoop::new(
                    Timespec { tv_sec: int(1), tv_nsec: int(2) },
                    self.sim.config.birds,
                );
            }
            "live" => {
                self.graphics().clear();
                let bytes = hex_bytes(arg(1));
                let taken = &bytes[..bytes.len().min(rbirds::config::INPUT_BUFFER_SIZE)];
                let frame_start = Timespec { tv_sec: int(2), tv_nsec: int(3) };
                let window = WinSize {
                    ws_col: int(4) as u16,
                    ws_row: int(5) as u16,
                    ws_xpixel: int(6) as u16,
                    ws_ypixel: int(7) as u16,
                };
                let mut graphics = self.graphics.take().expect("graphics");
                let grid = self.grid.as_mut().expect("grid before live");
                let result = self.live.frame(
                    &mut self.sim,
                    &mut self.renderer,
                    &mut graphics,
                    &mut self.birds,
                    &mut self.snapshot,
                    grid,
                    Some(taken),
                    frame_start,
                    window,
                );
                self.graphics = Some(graphics);
                self.allocated = self.live.live_birds as usize;
                match result {
                    Ok(Frame::Over) => self.out.push_str("live over\n"),
                    Ok(Frame::Drawn) => self.emit_buffer("live", 0),
                    Err(code) => {
                        let _ = writeln!(self.out, "live failed {code}");
                    }
                }
            }
            "stats" => account_frame(
                &mut self.renderer,
                self.sim.clock.seconds,
                int(1),
                arg(2).parse::<usize>().unwrap_or(0),
            ),
            "dump" => {
                let dump = self.dump();
                self.out.push_str(&dump);
            }
            "digest" => {
                let dump = self.dump();
                let _ = writeln!(self.out, "digest {:016x}", fnv1a(dump.as_bytes()));
            }
            other => panic!("unknown command {other}"),
        }
    }

    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.out)
    }
}

/// Runs a whole script through the port.
pub fn run_rust(script: &str) -> String {
    let mut world = World::new();
    for line in script.lines() {
        world.command(line);
    }
    world.take()
}
