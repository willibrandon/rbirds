//! Hawks: a predator is not a boid. It has no neighbours, obeys none of the
//! three rules and is kept out of the grid; it chases a bird and every bird
//! flees it. Translated from cbirds `boids.c` (`place_one_hawk` through
//! `hawk_vector`).

use std::f64::consts::PI;

use super::{Bird, Hawk, Sim, Vector, direction_frame, normalized_angle, turn_towards};
use crate::config::*;
use crate::fp::{self, mul_add};

impl Sim {
    /// `place_one_hawk`: one hawk, so a summoned one leaves the rest alone.
    pub fn place_one_hawk(&mut self, i: usize) {
        let x =
            f64::from(self.screen.width) * (i as f64 + 1.0) / (f64::from(self.config.hawks) + 1.0);
        let y = f64::from(self.screen.height) * if !i.is_multiple_of(2) { 0.75 } else { 0.25 };
        let direction = 2.0 * PI * self.rng.random_unit();
        self.hawks[i] = Hawk {
            x,
            y,
            direction,
            frame: direction_frame(direction),
            prey: -1,
            commitment: 0.0,
            passing: 0.0,
            wing: 0,
            wing_clock: 0.0,
        };
    }

    /// `place_hawks`.
    pub fn place_hawks(&mut self) {
        for i in 0..self.config.hawks as usize {
            self.place_one_hawk(i);
        }
    }

    /// `nearest_bird`: by brute force, in its own sky, optionally passing over
    /// birds another hawk has chosen, and no nearer than a floor.
    pub fn nearest_bird(
        &self,
        birds: &[Bird],
        x: f64,
        y: f64,
        own: usize,
        unclaimed: bool,
        no_nearer_than: f64,
    ) -> i32 {
        let mut best = -1;
        let mut best_distance = 0.0_f64;
        let floor_squared = no_nearer_than * no_nearer_than;
        for (b, bird) in birds[..self.config.birds as usize].iter().enumerate() {
            if bird.layer > 0 {
                continue;
            }
            if unclaimed {
                let taken = (0..self.config.hawks as usize)
                    .any(|h| h != own && self.hawks[h].prey == b as i32);
                if taken {
                    continue;
                }
            }
            let dx = bird.x - x;
            let dy = bird.y - y;
            // fma: boids.c:1419:35
            let distance = mul_add(dx, dx, dy * dy);
            if distance < floor_squared {
                continue;
            }
            if best < 0 || distance < best_distance {
                best_distance = distance;
                best = b as i32;
            }
        }
        best
    }

    /// `distance_to_bird`.
    pub fn distance_to_bird(birds: &[Bird], hawk: &Hawk, bird: i32) -> f64 {
        let bird = &birds[bird as usize];
        let dx = bird.x - hawk.x;
        let dy = bird.y - hawk.y;
        // fma: boids.c:1431:25
        mul_add(dx, dx, dy * dy).sqrt()
    }

    /// `reach_along_the_step`: the nearest the hawk passes its bird over this
    /// frame's whole travel.
    pub fn reach_along_the_step(&self, birds: &[Bird], hawk: &Hawk, bird: i32) -> f64 {
        let step = self.config.speed * HAWK_DIVE_SPEED;
        let target = &birds[bird as usize];
        let mut dx = target.x - hawk.x;
        let mut dy = target.y - hawk.y;
        // fma: boids.c:1441:46
        let mut along = mul_add(dx, fp::cos(hawk.direction), dy * fp::sin(hawk.direction));
        if along < 0.0 {
            along = 0.0;
        }
        if along > step {
            along = step;
        }
        // fma: boids.c:1444:29
        let near_x = mul_add(along, fp::cos(hawk.direction), hawk.x);
        // fma: boids.c:1445:29
        let near_y = mul_add(along, fp::sin(hawk.direction), hawk.y);
        dx = target.x - near_x;
        dy = target.y - near_y;
        // fma: boids.c:1448:25
        mul_add(dx, dx, dy * dy).sqrt()
    }

    /// `choose_prey`: pick, or keep; trade up only for a bird a clear quarter
    /// closer, and only once the commitment is spent.
    pub fn choose_prey(&mut self, i: usize, birds: &[Bird]) {
        if self.hawks[i].prey >= self.config.birds {
            self.hawks[i].prey = -1;
        }
        if self.hawks[i].prey >= 0 && self.hawks[i].commitment > 0.0 {
            return;
        }
        if self.hawks[i].prey >= 0
            && Sim::distance_to_bird(birds, &self.hawks[i], self.hawks[i].prey)
                > f64::from(HAWK_GIVE_UP)
        {
            self.hawks[i].prey = -1;
        }

        let (x, y) = (self.hawks[i].x, self.hawks[i].y);
        let mut candidate = self.nearest_bird(birds, x, y, i, true, f64::from(HAWK_STALK));
        if candidate < 0 {
            candidate = self.nearest_bird(birds, x, y, i, true, 0.0);
        }
        if candidate < 0 {
            candidate = self.nearest_bird(birds, x, y, i, false, 0.0);
        }
        if candidate < 0 {
            return;
        }
        let hawk = &self.hawks[i];
        if hawk.prey >= 0
            && Sim::distance_to_bird(birds, hawk, candidate)
                > 0.75 * Sim::distance_to_bird(birds, hawk, hawk.prey)
        {
            return;
        }
        self.hawks[i].prey = candidate;
        self.hawks[i].commitment = f64::from(HAWK_COMMITMENT_FRAMES) / f64::from(FRAME_RATE);
    }

    /// `hawk_turn_limit`: per elapsed flight time, and never so lazy that it
    /// cannot turn inside a third of the screen.
    pub fn hawk_turn_limit(&self) -> f64 {
        let mut limit = HAWK_TURN * f64::from(FRAME_RATE) * self.flight_seconds();
        let shorter = f64::from(if self.screen.width < self.screen.height {
            self.screen.width
        } else {
            self.screen.height
        });
        if shorter > 0.0 {
            let needed = self.config.speed * HAWK_DIVE_SPEED / (shorter / 3.0);
            if needed > limit {
                limit = needed;
            }
        }
        if limit > PI { PI } else { limit }
    }

    /// `hawk_turning_radius`.
    pub fn hawk_turning_radius(&self) -> f64 {
        let limit = self.hawk_turn_limit();
        if limit > 0.0 { self.config.speed * HAWK_DIVE_SPEED / limit } else { 0.0 }
    }

    /// `hawk_wall_band`: a whole diameter and a half ahead.
    pub fn hawk_wall_band(&self) -> f64 {
        self.hawk_turning_radius() * 3.0
    }

    /// `hawk_wall_vector`: a wall, and the panel, seen a turning circle ahead.
    pub fn hawk_wall_vector(&self, hawk: &Hawk) -> Vector {
        let mut wall = Vector::default();
        let band = self.hawk_wall_band();
        let width = f64::from(self.screen.width);
        let height = f64::from(self.screen.height);
        let band_x = if band < width / 3.0 { band } else { width / 3.0 };
        let band_y = if band < height / 3.0 { band } else { height / 3.0 };
        if band_x >= 1.0 {
            if hawk.x < band_x {
                wall.x = (band_x - hawk.x) / band_x;
            } else if hawk.x > width - band_x {
                wall.x = -(hawk.x - (width - band_x)) / band_x;
            }
        }
        if band_y >= 1.0 {
            if hawk.y < band_y {
                wall.y = (band_y - hawk.y) / band_y;
            } else if hawk.y > height - band_y {
                wall.y = -(hawk.y - (height - band_y)) / band_y;
            }
        }
        let legend_width = f64::from(self.screen.legend_width);
        let legend_height = f64::from(self.screen.legend_height);
        if self.screen.legend_width > 0
            && hawk.x < legend_width + band_x
            && hawk.y < legend_height + band_y
        {
            let out_right = legend_width + band_x - hawk.x;
            let out_below = legend_height + band_y - hawk.y;
            if out_right / band_x < out_below / band_y {
                wall.x += out_right / band_x;
            } else {
                wall.y += out_below / band_y;
            }
        }
        wall
    }

    /// `hawk_spacing`: a nudge away from any hawk closer than HAWK_SPACING,
    /// unanswerable at contact.
    pub fn hawk_spacing(&self, own: usize) -> Vector {
        let mut apart = Vector::default();
        let spacing = f64::from(HAWK_SPACING);
        for i in 0..self.config.hawks as usize {
            if i == own {
                continue;
            }
            let dx = self.hawks[own].x - self.hawks[i].x;
            let dy = self.hawks[own].y - self.hawks[i].y;
            // fma: boids.c:1557:34
            let squared = mul_add(dx, dx, dy * dy);
            if squared >= spacing * spacing || squared < 1e-9 {
                continue;
            }
            let distance = squared.sqrt();
            let strength = spacing / distance - 1.0;
            apart.x += strength * dx / distance;
            apart.y += strength * dy / distance;
        }
        apart
    }

    /// `hunt`: the chase, for every hawk, off the snapshot.
    pub fn hunt(&mut self, birds: &[Bird]) {
        for i in 0..self.config.hawks as usize {
            let flight_seconds = self.flight_seconds();
            {
                let hawk = &mut self.hawks[i];
                if hawk.commitment > 0.0 {
                    hawk.commitment -= flight_seconds;
                    if hawk.commitment < 0.0 {
                        hawk.commitment = 0.0;
                    }
                }
            }

            if self.hawks[i].passing > 0.0 {
                let hawk = &mut self.hawks[i];
                hawk.passing -= flight_seconds;
                if hawk.passing < 0.0 {
                    hawk.passing = 0.0;
                }
                hawk.prey = -1;
            } else {
                // Two silhouettes, and no wider, tested over the whole step.
                let arrived = f64::from(self.config.bird_size * 2);
                let prey = self.hawks[i].prey;
                let struck = prey >= 0
                    && prey < self.config.birds
                    && self.reach_along_the_step(birds, &self.hawks[i], prey) < arrived;
                if struck {
                    let hawk = &mut self.hawks[i];
                    hawk.prey = -1;
                    hawk.commitment = 0.0;
                    hawk.passing = f64::from(HAWK_PASS_FRAMES) / f64::from(FRAME_RATE);
                } else {
                    self.choose_prey(i, birds);
                }
            }

            let mut pace = HAWK_SPEED;
            let apart = self.hawk_spacing(i);
            let wall = self.hawk_wall_vector(&self.hawks[i]);
            // fma: boids.c:1624:46
            let mut want_x = mul_add(apart.x, HAWK_APART, wall.x * HAWK_WALL);
            // fma: boids.c:1625:46
            let mut want_y = mul_add(apart.y, HAWK_APART, wall.y * HAWK_WALL);
            let frame_seconds = self.frame_seconds;
            if self.hawks[i].prey >= 0 {
                let prey = birds[self.hawks[i].prey as usize];
                let gap = Sim::distance_to_bird(birds, &self.hawks[i], self.hawks[i].prey);
                let hawk = &mut self.hawks[i];
                if gap < HAWK_DIVE {
                    pace = HAWK_DIVE_SPEED;
                }
                // It soars until the dive, and then it beats.
                if gap < HAWK_DIVE || hawk.passing > 0.0 {
                    // fma: boids.c:1632:34
                    hawk.wing_clock =
                        mul_add(WING_HZ * f64::from(WING_CYCLE), frame_seconds, hawk.wing_clock);
                    while hawk.wing_clock >= 1.0 {
                        hawk.wing_clock -= 1.0;
                        hawk.wing = (hawk.wing + 1) % WING_CYCLE;
                    }
                } else {
                    hawk.wing = 0;
                }
                // Aim where the bird will be, never further than three steps.
                let mut lead = gap / pace;
                if lead > HAWK_LEAD_DISTANCE {
                    lead = HAWK_LEAD_DISTANCE;
                }
                // fma: boids.c:1647:35
                let to_x = mul_add(fp::cos(prey.direction), lead, prey.x) - hawk.x;
                // fma: boids.c:1648:35
                let to_y = mul_add(fp::sin(prey.direction), lead, prey.y) - hawk.y;
                // fma: boids.c:1649:45
                let reach = mul_add(to_x, to_x, to_y * to_y).sqrt();
                if reach > 1e-9 {
                    want_x += to_x / reach;
                    want_y += to_y / reach;
                }
            }
            // fma: boids.c:1655:29
            if mul_add(want_x, want_x, want_y * want_y) > 1e-9 {
                let limit = self.hawk_turn_limit();
                let hawk = &mut self.hawks[i];
                hawk.direction =
                    turn_towards(hawk.direction, normalized_angle(want_y, want_x), limit);
            }

            let speed = self.config.speed;
            let margin = f64::from(self.hawk_draw_offset());
            let width = self.screen.width;
            let height = self.screen.height;
            let hawk = &mut self.hawks[i];
            // fma: boids.c:1659:17
            hawk.x = mul_add(speed * pace, fp::cos(hawk.direction), hawk.x);
            // fma: boids.c:1660:17
            hawk.y = mul_add(speed * pace, fp::sin(hawk.direction), hawk.y);

            // Turned back at the walls, half its silhouette in.
            let mut last_x = f64::from(width - 1) - margin;
            let mut last_y = f64::from(height - 1) - margin;
            if last_x < margin {
                last_x = margin;
            }
            if last_y < margin {
                last_y = margin;
            }
            if hawk.x < margin || hawk.x > last_x {
                hawk.x = if hawk.x < margin { margin } else { last_x };
                hawk.direction =
                    normalized_angle(fp::sin(hawk.direction), -fp::cos(hawk.direction));
                hawk.prey = -1;
                hawk.commitment = 0.0;
                hawk.passing = 0.0;
            }
            if hawk.y < margin || hawk.y > last_y {
                hawk.y = if hawk.y < margin { margin } else { last_y };
                hawk.direction =
                    normalized_angle(-fp::sin(hawk.direction), fp::cos(hawk.direction));
                hawk.prey = -1;
                hawk.commitment = 0.0;
                hawk.passing = 0.0;
            }
            hawk.frame = direction_frame(hawk.direction);
        }
    }

    /// `hawk_reach`: never more than a third of the shorter side.
    pub fn hawk_reach(&self) -> f64 {
        let shorter = f64::from(if self.screen.width < self.screen.height {
            self.screen.width
        } else {
            self.screen.height
        });
        let fits = shorter / 3.0;
        let reach = f64::from(HAWK_REACH);
        if reach < fits { reach } else { fits }
    }

    /// `hawk_vector`: every bird flees every hawk in reach, partly sideways,
    /// on the side it is already turning.
    pub fn hawk_vector(&self, bird: &Bird) -> Vector {
        let mut force = Vector::default();
        if bird.layer > 0 {
            return force;
        }
        for hawk in &self.hawks[..self.config.hawks as usize] {
            let reach = self.hawk_reach();
            let dx = bird.x - hawk.x;
            let dy = bird.y - hawk.y;
            // fma: boids.c:1711:34
            let squared = mul_add(dx, dx, dy * dy);
            if squared >= reach * reach || squared < 1e-9 {
                continue;
            }
            let distance = squared.sqrt();
            let strength = (reach - distance) / reach;
            let away_x = dx / distance;
            let away_y = dy / distance;
            let mut side_x = -away_y;
            let mut side_y = away_x;
            // fma: boids.c:1718:43
            if mul_add(fp::cos(bird.direction), side_x, fp::sin(bird.direction) * side_y) < 0.0 {
                side_x = -side_x;
                side_y = -side_y;
            }
            // fma: boids.c:1722:17
            // fma: boids.c:1722:39
            force.x = mul_add(strength, mul_add(HAWK_SWIRL, side_x, away_x), force.x);
            // fma: boids.c:1723:17
            // fma: boids.c:1723:39
            force.y = mul_add(strength, mul_add(HAWK_SWIRL, side_y, away_y), force.y);
        }
        force
    }
}
