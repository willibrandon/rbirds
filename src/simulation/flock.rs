//! The flock: where birds start, what pushes them, how each one reads its
//! neighbours, and one step of every bird. Translated from cbirds `boids.c`
//! (`place_one_bird` through `update_birds`).

use std::f64::consts::PI;

use super::{Bird, Sim, Vector, direction_frame, normalized_angle, trig_lookup, turn_towards};
use crate::config::*;
use crate::fp::{self, mul_add};
use crate::spatial_grid::SpatialGrid;

impl Sim {
    /// `wind_vector`: straight down in the rain, nothing otherwise.
    pub fn wind_vector(&self) -> Vector {
        let mut force = Vector::default();
        if !self.rain {
            return force;
        }
        force.y = 1.0;
        force
    }

    /// `flock_pace`: the first flock at full speed, the last at seven eighths.
    pub fn flock_pace(&self, flock: i32) -> f64 {
        if self.config.flocks <= 1 {
            return 1.0;
        }
        1.0 - (1.0 - self.config.avoid_kinship) * 0.125 * f64::from(flock)
            / f64::from(self.config.flocks - 1)
    }

    /// `shade_for_flock`: the ramp's ends first, then the space between.
    pub fn shade_for_flock(&self, flock: i32) -> i32 {
        let shades = self.palette_shades();
        if shades <= 1 || self.config.flocks <= 1 {
            return 0;
        }
        let spread = flock * (shades - 1) / (self.config.flocks - 1);
        if spread >= shades { shades - 1 } else { spread }
    }

    /// `place_one_bird`: spread over the region no turn band covers, rejected
    /// rather than clamped out of the panel's zone, with a bounded fallback.
    pub fn place_one_bird(&mut self, bird: &mut Bird, index: i32) {
        *bird = Bird::default();
        let screen = self.screen;
        let mut min_x = f64::from(screen.turn_x);
        let mut max_x = f64::from(screen.width - screen.turn_x);
        let mut min_y = f64::from(screen.turn_y);
        let mut max_y = f64::from(screen.height - screen.turn_bottom);
        if max_x <= min_x {
            min_x = f64::from(screen.width) / 2.0;
            max_x = min_x;
        }
        if max_y <= min_y {
            min_y = f64::from(screen.height) / 2.0;
            max_y = min_y;
        }

        bird.flock = index % self.config.flocks;
        if self.config.flocks > 1 {
            let band = (max_x - min_x) / f64::from(self.config.flocks);
            // fma: boids.c:1774:15
            min_x = mul_add(band, f64::from(bird.flock), min_x);
            max_x = min_x + band;
            // A bird added with + joins its flock where it is flying.
            let flock = bird.flock as usize;
            if self.flock_home_x[flock] != 0.0 || self.flock_home_y[flock] != 0.0 {
                min_x = self.flock_home_x[flock] - f64::from(FLOCK_LEASH) / 2.0;
                max_x = min_x + f64::from(FLOCK_LEASH);
                min_y = self.flock_home_y[flock] - f64::from(FLOCK_LEASH) / 2.0;
                max_y = min_y + f64::from(FLOCK_LEASH);
            }
        }

        let mut attempt = 0;
        loop {
            // fma: boids.c:1792:25
            bird.x = mul_add(max_x - min_x, self.rng.random_unit(), min_x);
            // fma: boids.c:1793:25
            bird.y = mul_add(max_y - min_y, self.rng.random_unit(), min_y);
            if !self.legend_turn_zone(bird.x, bird.y) {
                break;
            }
            if attempt + 1 >= SPAWN_ATTEMPTS {
                bird.y = f64::from(screen.legend_height) + self.config.speed + 1.0;
                if bird.y > max_y {
                    bird.x = f64::from(screen.legend_width) + self.config.speed + 1.0;
                }
                break;
            }
            attempt += 1;
        }
        bird.direction = 2.0 * PI * self.rng.random_unit();
        bird.frame = direction_frame(bird.direction);
        // A plane of its own, drawn only when there are two, and a place in
        // the beat of its own.
        bird.layer = if self.deep_look && self.rng.random_unit() < FAR_SHARE { 1 } else { 0 };
        bird.wing = (self.rng.random_unit() * f64::from(WING_CYCLE)) as i32 % WING_CYCLE;
        bird.wing_clock = self.rng.random_unit();
        bird.gliding = 0.0;
        bird.shade = if self.config.flocks > 1 {
            self.shade_for_flock(bird.flock)
        } else {
            let shades = self.palette_shades();
            (self.rng.random_unit() * f64::from(shades)) as i32 % shades
        };
    }

    /// `initialize_birds`.
    pub fn initialize_birds(&mut self, birds: &mut [Bird]) {
        for i in 0..self.config.birds {
            self.place_one_bird(&mut birds[i as usize], i);
        }
    }

    /// `resize_the_flock`: both arrays change or neither does. The flying
    /// birds carry on and only the new ones are placed. `false`, with both
    /// arrays untouched, when either cannot be allocated.
    pub fn resize_the_flock(
        &mut self,
        birds: &mut Vec<Bird>,
        snapshot: &mut Vec<Bird>,
        from: i32,
        to: i32,
    ) -> bool {
        let length = to as usize;
        let mut grown: Vec<Bird> = Vec::new();
        let mut grown_snapshot: Vec<Bird> = Vec::new();
        if grown.try_reserve_exact(length).is_err()
            || grown_snapshot.try_reserve_exact(length).is_err()
        {
            return false;
        }
        let kept = from.min(to) as usize;
        grown.extend_from_slice(&birds[..kept]);
        grown.resize(length, Bird::default());
        for i in from..to {
            self.place_one_bird(&mut grown[i as usize], i);
        }
        // The C snapshot is uninitialized until the next frame copies into it.
        grown_snapshot.resize(length, Bird::default());
        *birds = grown;
        *snapshot = grown_snapshot;
        true
    }

    /// `pointer_vector`: falls off with distance, one at the pointer.
    pub fn pointer_vector(&self, bird: &Bird) -> Vector {
        let mut force = Vector::default();
        if !self.mouse.present {
            return force;
        }
        let dx = bird.x - self.mouse.x;
        let dy = bird.y - self.mouse.y;
        // fma: boids.c:1857:30
        let squared = mul_add(dx, dx, dy * dy);
        let reach = f64::from(MOUSE_REACH);
        if squared >= reach * reach || squared < 1e-9 {
            return force;
        }
        let distance = squared.sqrt();
        let strength = (reach - distance) / reach;
        force.x = strength * dx / distance;
        force.y = strength * dy / distance;
        force
    }

    /// `edge_push`: gently through the band, steeply past the screen's edge.
    pub fn edge_push(&self, past: f64, band: f64) -> f64 {
        if past <= 0.0 {
            return 0.0;
        }
        let depth = past / band;
        if depth <= 1.0 {
            return EDGE_FIRM * depth * depth;
        }
        let boundary = self.config.boundary;
        // fma: boids.c:1890:41
        EDGE_FIRM * mul_add(depth - 1.0, ESCAPE_PENALTY, boundary) / boundary
    }

    /// `boundary_vector`: the panel first, then no walls in the rain, then
    /// the four bands.
    pub fn boundary_vector(&self, bird: &Bird) -> Vector {
        let mut boundary = Vector::default();
        if self.legend_repels(bird, &mut boundary) {
            return boundary;
        }
        if self.rain {
            return boundary;
        }
        let s = &self.screen;
        let turn_x = f64::from(s.turn_x);
        let turn_y = f64::from(s.turn_y);
        let turn_bottom = f64::from(s.turn_bottom);
        if bird.x < turn_x {
            boundary.x = self.edge_push(turn_x - bird.x, turn_x);
        } else if bird.x > f64::from(s.width - s.turn_x) {
            boundary.x = -self.edge_push(bird.x - f64::from(s.width - s.turn_x), turn_x);
        }
        if bird.y < turn_y {
            boundary.y = self.edge_push(turn_y - bird.y, turn_y);
        } else if bird.y > f64::from(s.height - s.turn_bottom) {
            boundary.y = -self.edge_push(bird.y - f64::from(s.height - s.turn_bottom), turn_bottom);
        }
        boundary
    }

    /// `flock_room`: two leashes, or nearly the shorter side when that is less.
    pub fn flock_room(&self) -> f64 {
        let room = 2.0 * f64::from(FLOCK_LEASH) * self.config.avoid_room;
        let shorter = f64::from(if self.screen.width < self.screen.height {
            self.screen.width
        } else {
            self.screen.height
        });
        let fits = 0.9 * shorter;
        if room < fits { room } else { fits }
    }

    /// `measure_flocks`: every flock's centre, leash and home, off the same
    /// snapshot every bird reads. The loops index several per-flock arrays at
    /// once, in the C's order.
    #[allow(clippy::needless_range_loop)]
    pub fn measure_flocks(&mut self, birds: &[Bird]) {
        const FLOCKS: usize = MAX_FLOCKS as usize;
        let mut counted = [0_i32; FLOCKS];
        for f in 0..FLOCKS {
            self.flock_center_x[f] = 0.0;
            self.flock_center_y[f] = 0.0;
            self.flock_home_x[f] = 0.0;
            self.flock_home_y[f] = 0.0;
        }
        if self.config.flocks <= 1 {
            return;
        }
        for bird in &birds[..self.config.birds as usize] {
            let flock = bird.flock;
            if !(0..MAX_FLOCKS).contains(&flock) {
                continue;
            }
            let flock = flock as usize;
            self.flock_center_x[flock] += bird.x;
            self.flock_center_y[flock] += bird.y;
            counted[flock] += 1;
        }
        for f in 0..FLOCKS {
            if counted[f] > 0 {
                self.flock_center_x[f] /= f64::from(counted[f]);
                self.flock_center_y[f] /= f64::from(counted[f]);
            }
            let width = LEASH_PER_ROOT_BIRD * f64::from(counted[f]).sqrt();
            self.flock_leash[f] =
                if width > f64::from(FLOCK_LEASH) { width } else { f64::from(FLOCK_LEASH) };
        }

        let room = self.flock_room();
        let flocks = (self.config.flocks as usize).min(FLOCKS);
        for f in 0..flocks {
            self.flock_home_x[f] = self.flock_center_x[f];
            self.flock_home_y[f] = self.flock_center_y[f];
            if counted[f] == 0 {
                continue;
            }
            let mut shove_x = 0.0_f64;
            let mut shove_y = 0.0_f64;
            let mut crowding = 0;
            for g in 0..flocks {
                if g == f || counted[g] == 0 {
                    continue;
                }
                let mut dx = self.flock_center_x[f] - self.flock_center_x[g];
                let mut dy = self.flock_center_y[f] - self.flock_center_y[g];
                // fma: boids.c:1959:44
                let mut distance = mul_add(dx, dx, dy * dy).sqrt();
                if distance >= room {
                    continue;
                }
                if distance < 1e-9 {
                    // Exactly on top of one another: one direction per flock.
                    let angle = 2.0 * PI * f as f64 / f64::from(self.config.flocks);
                    let (sine, cosine) = fp::sin_cos(angle);
                    dx = cosine;
                    dy = sine;
                    distance = 1.0;
                }
                shove_x += (room - distance) * dx / distance;
                shove_y += (room - distance) * dy / distance;
                crowding += 1;
            }
            // The sum of the shoves, never further than one room's worth.
            if crowding > 0 {
                // fma: boids.c:1980:51
                let shove = mul_add(shove_x, shove_x, shove_y * shove_y).sqrt();
                if shove > room {
                    shove_x = shove_x * room / shove;
                    shove_y = shove_y * room / shove;
                }
                self.flock_home_x[f] += shove_x;
                self.flock_home_y[f] += shove_y;
            }
            // And home is somewhere a flock can actually be.
            if self.flock_home_x[f] < 0.0 {
                self.flock_home_x[f] = 0.0;
            }
            if self.flock_home_y[f] < 0.0 {
                self.flock_home_y[f] = 0.0;
            }
            if self.flock_home_x[f] > f64::from(self.screen.width) {
                self.flock_home_x[f] = f64::from(self.screen.width);
            }
            if self.flock_home_y[f] > f64::from(self.screen.height) {
                self.flock_home_y[f] = f64::from(self.screen.height);
            }
        }
    }

    /// `leash_vector`: nothing within the flock's width of home, then a pull,
    /// letting go (cubed) as strangers become kin.
    pub fn leash_vector(&self, bird: &Bird) -> Vector {
        let mut pull = Vector::default();
        if self.config.flocks <= 1 || !(0..MAX_FLOCKS).contains(&bird.flock) {
            return pull;
        }
        let flock = bird.flock as usize;
        let dx = self.flock_home_x[flock] - bird.x;
        let dy = self.flock_home_y[flock] - bird.y;
        // fma: boids.c:2005:36
        let distance = mul_add(dx, dx, dy * dy).sqrt();
        let width = if self.flock_leash[flock] > 0.0 {
            self.flock_leash[flock]
        } else {
            f64::from(FLOCK_LEASH)
        };
        if distance <= width || distance < 1e-9 {
            return pull;
        }
        let mut strength = (distance - width) / width;
        if strength > 1.0 {
            strength = 1.0;
        }
        let apart = 1.0 - self.config.avoid_kinship;
        strength *= apart * apart * apart;
        pull.x = strength * dx / distance;
        pull.y = strength * dy / distance;
        pull
    }

    /// `flock_direction`: the heading one bird wants, from its neighbours in
    /// the snapshot through the grid, in the grid's own order.
    pub fn flock_direction(&self, birds: &[Bird], grid: &SpatialGrid, target_index: usize) -> f64 {
        let target = &birds[target_index];
        // Writing overrules flocking while it lasts.
        if let Some((want_x, want_y)) = self.formation.target_of(target_index as i32) {
            let to_x = want_x - target.x;
            let to_y = want_y - target.y;
            // fma: boids.c:2030:25
            if mul_add(to_x, to_x, to_y * to_y) > 1e-9 {
                return normalized_angle(to_y, to_x);
            }
            return target.direction;
        }
        let config = &self.config;
        let mut separation = Vector::default();
        let mut alignment = Vector::default();
        let mut cohesion = Vector::default();
        let mut wary = Vector::default();
        let boundary = self.boundary_vector(target);
        let leash = self.leash_vector(target);
        let pointer = self.pointer_vector(target);
        let hawk = self.hawk_vector(target);
        let wind = self.wind_vector();
        let mut neighbors = 0;
        let mut strangers = 0;
        // Counted in kinship: a whole bird for its own flock.
        let mut kin = 0.0_f64;
        let (center_x, center_y) = grid.cell_for_position(target.x, target.y);
        let mut min_x = center_x - config.vision_cells;
        let mut max_x = center_x + config.vision_cells;
        let mut min_y = center_y - config.vision_cells;
        let mut max_y = center_y + config.vision_cells;
        if min_x < 0 {
            min_x = 0;
        }
        if min_y < 0 {
            min_y = 0;
        }
        if max_x >= grid.columns {
            max_x = grid.columns - 1;
        }
        if max_y >= grid.rows {
            max_y = grid.rows - 1;
        }
        let radius_squared = f64::from(config.vision_radius_squared);

        for cell_y in min_y..=max_y {
            for cell_x in min_x..=max_x {
                let cell = (cell_y * grid.columns + cell_x) as usize;
                for &i in grid.cell_items(cell) {
                    let i = i as usize;
                    if i == target_index {
                        continue;
                    }
                    let other = &birds[i];
                    let dx = target.x - other.x;
                    let dy = target.y - other.y;
                    // fma: boids.c:2060:29
                    if mul_add(dx, dx, dy * dy) >= radius_squared {
                        continue;
                    }
                    // The far layer is another sky.
                    if other.layer != target.layer {
                        continue;
                    }
                    // Separation is physical; alignment and cohesion are social.
                    separation.x += dx;
                    separation.y += dy;
                    neighbors += 1;
                    if other.flock != target.flock {
                        if config.avoid_kinship > 0.0 {
                            let heading = trig_lookup(other.direction);
                            let kinship = config.avoid_kinship;
                            // fma: boids.c:2076:37
                            alignment.x = mul_add(kinship, f64::from(heading.cosine), alignment.x);
                            // fma: boids.c:2077:37
                            alignment.y = mul_add(kinship, f64::from(heading.sine), alignment.y);
                            // fma: boids.c:2078:36
                            cohesion.x = mul_add(kinship, other.x, cohesion.x);
                            // fma: boids.c:2079:36
                            cohesion.y = mul_add(kinship, other.y, cohesion.y);
                            kin += kinship;
                        }
                        // Away from a stranger, hardest when it is nearest.
                        if config.avoid_weight > 0.0 {
                            // fma: boids.c:2086:56
                            let distance = mul_add(dx, dx, dy * dy).sqrt();
                            if distance > 1e-9 {
                                let strength = 1.0 - distance / f64::from(config.vision_radius);
                                wary.x += strength * dx / distance;
                                wary.y += strength * dy / distance;
                                strangers += 1;
                            }
                        }
                        continue;
                    }
                    let heading = trig_lookup(other.direction);
                    alignment.x += f64::from(heading.cosine);
                    alignment.y += f64::from(heading.sine);
                    cohesion.x += other.x;
                    cohesion.y += other.y;
                    kin += 1.0;
                }
            }
        }
        if neighbors != 0 {
            if kin != 0.0 {
                alignment.x /= kin;
                alignment.y /= kin;
                cohesion.x = cohesion.x / kin - target.x;
                cohesion.y = cohesion.y / kin - target.y;
            }
            if strangers != 0 {
                wary.x /= f64::from(strangers);
                wary.y /= f64::from(strangers);
            }
            // fma: boids.c:2116:53
            let mut x = mul_add(separation.x, config.separation, alignment.x * config.alignment);
            // fma: boids.c:2116:86
            x = mul_add(cohesion.x, COHESION_W, x);
            // fma: boids.c:2117:44
            x = mul_add(boundary.x, config.boundary, x);
            // fma: boids.c:2117:75
            x = mul_add(leash.x, LEASH_WEIGHT, x);
            // fma: boids.c:2117:100
            x = mul_add(pointer.x, MOUSE_WEIGHT, x);
            // fma: boids.c:2118:45
            x = mul_add(hawk.x, HAWK_WEIGHT, x);
            // fma: boids.c:2118:68
            x = mul_add(wind.x, WIND_WEIGHT, x);
            // fma: boids.c:2118:91
            x = mul_add(wary.x, config.avoid_weight, x);
            // fma: boids.c:2120:53
            let mut y = mul_add(separation.y, config.separation, alignment.y * config.alignment);
            // fma: boids.c:2120:86
            y = mul_add(cohesion.y, COHESION_W, y);
            // fma: boids.c:2121:44
            y = mul_add(boundary.y, config.boundary, y);
            // fma: boids.c:2121:75
            y = mul_add(leash.y, LEASH_WEIGHT, y);
            // fma: boids.c:2121:100
            y = mul_add(pointer.y, MOUSE_WEIGHT, y);
            // fma: boids.c:2122:45
            y = mul_add(hawk.y, HAWK_WEIGHT, y);
            // fma: boids.c:2122:68
            y = mul_add(wind.y, WIND_WEIGHT, y);
            // fma: boids.c:2122:91
            y = mul_add(wary.y, config.avoid_weight, y);
            return if x == 0.0 && y == 0.0 { target.direction } else { normalized_angle(y, x) };
        }
        // fma: boids.c:2126:47
        let mut bx = mul_add(boundary.x, config.boundary, leash.x * LEASH_WEIGHT);
        // fma: boids.c:2126:72
        bx = mul_add(pointer.x, MOUSE_WEIGHT, bx);
        // fma: boids.c:2126:99
        bx = mul_add(hawk.x, HAWK_WEIGHT, bx);
        // fma: boids.c:2127:39
        bx = mul_add(wind.x, WIND_WEIGHT, bx);
        // fma: boids.c:2128:47
        let mut by = mul_add(boundary.y, config.boundary, leash.y * LEASH_WEIGHT);
        // fma: boids.c:2128:72
        by = mul_add(pointer.y, MOUSE_WEIGHT, by);
        // fma: boids.c:2128:99
        by = mul_add(hawk.y, HAWK_WEIGHT, by);
        // fma: boids.c:2129:39
        by = mul_add(wind.y, WIND_WEIGHT, by);
        if bx != 0.0 || by != 0.0 {
            let (sine, cosine) = fp::sin_cos(target.direction);
            let x = cosine + bx;
            let y = sine + by;
            if x != 0.0 || y != 0.0 {
                return normalized_angle(y, x);
            }
        }
        target.direction
    }

    /// `shade_for`: one flock by heading, folded at the half turn; more than
    /// one by flock.
    pub fn shade_for(&self, bird: &Bird) -> i32 {
        self.shade_with_heading(bird, None)
    }

    fn shade_with_heading(&self, bird: &Bird, heading: Option<(f64, f64)>) -> i32 {
        let shades = self.palette_shades();
        if shades <= 1 {
            return 0;
        }
        if self.config.flocks > 1 {
            return self.shade_for_flock(bird.flock);
        }
        let (sine, cosine) = heading.unwrap_or_else(|| fp::sin_cos(bird.direction));
        let turns = normalized_angle(sine, cosine) / (2.0 * PI);
        let folded = if turns < 0.5 { turns * 2.0 } else { (1.0 - turns) * 2.0 };
        let shade = (folded * f64::from(shades)) as i32;
        if shade >= shades { shades - 1 } else { shade }
    }

    /// `wrap_position`: off one edge and back on the other.
    pub fn wrap_position(&self, bird: &mut Bird) {
        let width = f64::from(self.screen.width);
        let height = f64::from(self.screen.height);
        if bird.x < 0.0 {
            bird.x += width;
        }
        if bird.x >= width {
            bird.x -= width;
        }
        if bird.y < 0.0 {
            bird.y += height;
        }
        if bird.y >= height {
            bird.y -= height;
        }
    }

    /// `beat_wings`: WING_HZ beats a second whatever the frame rate, and now
    /// and then a glide after a completed beat.
    pub fn beat_wings(&mut self, bird: &mut Bird) {
        if bird.gliding > 0.0 {
            bird.gliding -= self.frame_seconds;
            if bird.gliding < 0.0 {
                bird.gliding = 0.0;
            }
            bird.wing = 0;
            return;
        }
        // fma: boids.c:2187:22
        bird.wing_clock =
            mul_add(WING_HZ * f64::from(WING_CYCLE), self.frame_seconds, bird.wing_clock);
        while bird.wing_clock >= 1.0 {
            bird.wing_clock -= 1.0;
            bird.wing = (bird.wing + 1) % WING_CYCLE;
            if bird.wing == 0 && self.rng.random_unit() < GLIDE_CHANCE {
                // fma: boids.c:2193:35
                let seconds = mul_add(
                    GLIDE_SECONDS_MAX - GLIDE_SECONDS_MIN,
                    self.rng.random_unit(),
                    GLIDE_SECONDS_MIN,
                );
                bird.gliding = seconds;
            }
        }
    }

    /// `update_birds`: every bird from the snapshot.
    pub fn update_birds(&mut self, birds: &mut [Bird], snapshot: &[Bird], grid: &SpatialGrid) {
        self.measure_flocks(snapshot);
        for i in 0..self.config.birds as usize {
            let mut direction = self.flock_direction(snapshot, grid, i);
            // Banking is for flocking: a bird writing a letter, and a bird in
            // the panel's turn zone, turn at once.
            if self.formation.target_of(i as i32).is_none()
                && !self.legend_turn_zone(snapshot[i].x, snapshot[i].y)
            {
                direction = turn_towards(snapshot[i].direction, direction, self.turn_limit());
            }
            let bird = &mut birds[i];
            bird.direction = direction;
            // Never past the target: the last step is the distance left.
            let mut step = self.config.speed * self.flock_pace(snapshot[i].flock);
            if snapshot[i].layer > 0 {
                step *= FAR_PACE;
            }
            if let Some((want_x, want_y)) = self.formation.target_of(i as i32) {
                let dx = want_x - bird.x;
                let dy = want_y - bird.y;
                // fma: boids.c:2220:45
                let remaining = mul_add(dx, dx, dy * dy).sqrt();
                if remaining < step {
                    step = remaining;
                }
            }
            // The platform routine returns both values. Reuse that exact pair
            // for movement and shade instead of calculating it four times.
            let (sine, cosine) = fp::sin_cos(direction);
            // fma: boids.c:2223:20
            bird.x = mul_add(step, cosine, bird.x);
            // fma: boids.c:2224:20
            bird.y = mul_add(step, sine, bird.y);
            if self.rain {
                self.wrap_position(bird);
            }
            bird.shade = self.shade_with_heading(bird, Some((sine, cosine)));
            self.beat_wings(bird);
            if self.config.trails && i as i32 % TRAIL_EVERY == 0 {
                // Where it was, not where it is: a tail behind, never under.
                let at = bird.trail_at as usize;
                bird.trail_x[at] = snapshot[i].x;
                bird.trail_y[at] = snapshot[i].y;
                bird.trail_at = (bird.trail_at + 1) % TRAIL_LENGTH;
                if bird.trail_held < TRAIL_LENGTH {
                    bird.trail_held += 1;
                }
            }
        }
    }
}
