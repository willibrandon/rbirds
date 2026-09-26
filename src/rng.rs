//! The flock's own random numbers (`seed_random`, `next_random`,
//! `random_unit` in cbirds `boids.c`).
//!
//! `--seed` promises the same flock for the same seed, and a C library's
//! `rand()` cannot keep that. This is glibc's generator written out: an
//! additive lagged Fibonacci over 31 words, seeded by a Park and Miller LCG and
//! run 310 steps before its first number. The words wrap as `uint32_t` does in
//! the C, which is the only arithmetic here that is meant to overflow.

#![forbid(unsafe_code)]

pub const RANDOM_WORDS: usize = 31;
pub const RANDOM_LAG: usize = 3;
pub const RANDOM_WARMUP: usize = 310;
pub const RANDOM_MAX: u32 = 2_147_483_647;

/// The generator's whole state: the words and the two indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    pub word: [u32; RANDOM_WORDS],
    pub front: usize,
    pub rear: usize,
}

impl Default for Rng {
    /// The C's zero-initialized static state, before any seed.
    fn default() -> Rng {
        Rng { word: [0; RANDOM_WORDS], front: 0, rear: 0 }
    }
}

impl Rng {
    /// A generator seeded as `seed_random(seed)` seeds one.
    pub fn seeded(seed: u32) -> Rng {
        let mut rng = Rng::default();
        rng.seed(seed);
        rng
    }

    /// `seed_random`: zero is one, as in glibc.
    pub fn seed(&mut self, seed: u32) {
        let seed = if seed == 0 { 1 } else { seed };
        // glibc holds the seed as a 32 bit signed word; the wrap is spelled
        // out rather than left to a conversion.
        let mut word: i64 =
            if seed > i32::MAX as u32 { i64::from(seed) - 4_294_967_296 } else { i64::from(seed) };
        self.word[0] = seed;
        for slot in self.word.iter_mut().skip(1) {
            // 16807 * word % 2147483647 by Schrage's method, as glibc computes
            // it; i64 division truncates toward zero exactly as C's does.
            let high = word / 127_773;
            let low = word % 127_773;
            word = 16_807 * low - 2_836 * high;
            if word < 0 {
                word += 2_147_483_647;
            }
            *slot = word as u32;
        }
        self.front = RANDOM_LAG;
        self.rear = 0;
        for _ in 0..RANDOM_WARMUP {
            self.next_random();
        }
    }

    /// `next_random`: the sum wraps, and the lowest bit, the least random
    /// one, is dropped.
    pub fn next_random(&mut self) -> u32 {
        let sum = self.word[self.front].wrapping_add(self.word[self.rear]);
        self.word[self.front] = sum;
        self.front = (self.front + 1) % RANDOM_WORDS;
        self.rear = (self.rear + 1) % RANDOM_WORDS;
        sum >> 1
    }

    /// `random_unit`: zero to one, both included.
    pub fn random_unit(&mut self) -> f64 {
        f64::from(self.next_random()) / f64::from(RANDOM_MAX)
    }
}
