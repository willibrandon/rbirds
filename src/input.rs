//! Keys, the pointer, the Konami code and the autopilot. Translated from
//! cbirds `boids.c` (`read_decimal`, `read_mouse_report`, the autopilot,
//! `konami_note`, `handle_input`).
//!
//! The parser's state persists between reads, as the C's function statics
//! do, so an escape sequence split across two reads is still one sequence.

#![forbid(unsafe_code)]

use crate::config::*;
use crate::simulation::{KONAMI, KONAMI_LENGTH, Sim};

/// Where `handle_input` is in an escape sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputState {
    #[default]
    Normal,
    Escape,
    Sequence,
}

/// `handle_input`'s statics: the state and the sequence body so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputParser {
    pub state: InputState,
    pub sequence: [u8; 32],
    pub sequence_length: usize,
    /// Rendering inputs, including keys consumed while output is blocked.
    /// Pointer reports do not change a paused image.
    pub(crate) revision: u64,
    filter_graphics: bool,
    graphics_string: bool,
    graphics_escape: bool,
}

impl InputParser {
    /// Carry an incomplete startup APC into the live parser. This is enabled
    /// only for the extra local iTerm query; ordinary input retains C behavior.
    #[cfg(target_os = "macos")]
    pub(crate) fn ignore_kitty_replies(&mut self, partial: &[u8]) {
        self.filter_graphics = true;
        self.graphics_string = false;
        self.graphics_escape = false;
        self.state = InputState::Normal;
        for &key in partial {
            if !self.skip_graphics_byte(key) {
                self.state = if key == 0x1b { InputState::Escape } else { InputState::Normal };
            }
        }
    }

    fn skip_graphics_byte(&mut self, key: u8) -> bool {
        if !self.filter_graphics {
            return false;
        }
        if self.graphics_string {
            if self.graphics_escape {
                self.graphics_escape = false;
                if key == b'\\' {
                    self.graphics_string = false;
                } else if key == 0x1b {
                    self.graphics_escape = true;
                } else {
                    // A new escape sequence abandons an unterminated APC.
                    self.graphics_string = false;
                    self.state = InputState::Escape;
                    return false;
                }
            } else if key == 0x1b {
                self.graphics_escape = true;
            } else if key == 0x18 || key == 0x1a {
                self.graphics_string = false;
            }
            return true;
        }
        if self.state == InputState::Escape && key == b'_' {
            self.state = InputState::Normal;
            self.graphics_string = true;
            return true;
        }
        false
    }
}

/// `read_decimal`: the decimal digits at `text[*at..]`, overflow refused.
/// On success `*at` moves past them.
pub fn read_decimal(text: &[u8], at: &mut usize) -> Option<i32> {
    let mut position = *at;
    let digit_at = |p: usize| text.get(p).copied().filter(u8::is_ascii_digit);
    digit_at(position)?;
    let mut result: i32 = 0;
    while let Some(c) = digit_at(position) {
        let digit = i32::from(c - b'0');
        if result > (i32::MAX - digit) / 10 {
            return None;
        }
        result = result * 10 + digit;
        position += 1;
    }
    *at = position;
    Some(result)
}

impl Sim {
    /// `read_mouse_report`: `<button;column;row`, one based, as mode 1006
    /// sends it; the pointer is the middle of the cell it names. `sequence`
    /// is the C string of the sequence body.
    pub fn read_mouse_report(&mut self, sequence: &[u8]) {
        if sequence.first() != Some(&b'<') {
            return;
        }
        let mut at = 1;
        let Some(_button) = read_decimal(sequence, &mut at) else { return };
        if sequence.get(at) != Some(&b';') {
            return;
        }
        at += 1;
        let Some(column) = read_decimal(sequence, &mut at) else { return };
        if sequence.get(at) != Some(&b';') {
            return;
        }
        at += 1;
        let Some(row) = read_decimal(sequence, &mut at) else { return };
        if column < 1 || row < 1 {
            return;
        }
        self.mouse.x = (f64::from(column) - 0.5) * f64::from(self.screen.cell_width);
        self.mouse.y = (f64::from(row) - 0.5) * f64::from(self.screen.cell_height);
        self.mouse.present = true;
    }

    /// `drift_a_slider`: one notch of one of the four flocking sliders,
    /// turned back at the ends.
    pub fn drift_a_slider(&mut self) {
        let which = (self.rng.random_unit() * 4.0) as i32 % 4;
        let mut step = if self.rng.random_unit() < 0.5 { -1 } else { 1 };
        let notch = match which {
            0 => &mut self.config.boundary_notch,
            1 => &mut self.config.separation_notch,
            2 => &mut self.config.alignment_notch,
            _ => &mut self.config.vision_notch,
        };
        if *notch + step < 0 || *notch + step > LEGEND_BAR_CELLS {
            step = -step;
        }
        *notch += step;
        self.apply_notches();
    }

    /// `flying_itself`: a minute to be sure nobody is watching.
    pub fn flying_itself(&self) -> bool {
        self.clock.seconds - self.last_key_at >= f64::from(IDLE_SECONDS)
    }

    /// `maybe_drift`: a notch every AUTOPILOT_PERIOD seconds once idle.
    pub fn maybe_drift(&mut self) {
        if !self.flying_itself() {
            return;
        }
        if self.clock.seconds - self.last_drift_at < f64::from(AUTOPILOT_PERIOD) {
            return;
        }
        self.last_drift_at = self.clock.seconds;
        self.drift_a_slider();
    }

    /// `konami_note`: the last ten keys, compared as a whole ring, so a
    /// stutter does not throw the sequence away. Four hawks for the code.
    pub fn konami_note(&mut self, key: u8) {
        let konami = &mut self.konami;
        konami.seen[konami.at as usize % KONAMI_LENGTH] = key;
        konami.at += 1;
        if (konami.at as usize) < KONAMI_LENGTH {
            return;
        }
        let start = konami.at as usize;
        if KONAMI
            .iter()
            .enumerate()
            .any(|(i, &key)| konami.seen[(start + i) % KONAMI_LENGTH] != key)
        {
            return;
        }
        konami.at = 0;
        konami.seen = [0; KONAMI_LENGTH];
        self.config.hawks = MAX_HAWKS;
        self.place_hawks();
    }

    /// `handle_input` over the bytes one `read` returned (`None` when the
    /// read failed or found nothing, which changes nothing). `false` is `q`:
    /// the rest of that read is not looked at, as in the C.
    pub fn handle_input(&mut self, parser: &mut InputParser, input: Option<&[u8]>) -> bool {
        let Some(input) = input.filter(|bytes| !bytes.is_empty()) else { return true };
        if !parser.filter_graphics {
            self.last_key_at = self.clock.seconds;
        }
        for &key in input {
            if parser.skip_graphics_byte(key) {
                continue;
            }
            if parser.filter_graphics && key != 0x1b {
                self.last_key_at = self.clock.seconds;
            }
            if parser.state == InputState::Escape {
                if key == b'[' || key == b'O' {
                    parser.state = InputState::Sequence;
                    parser.sequence_length = 0;
                } else if key != 0x1b {
                    parser.state = InputState::Normal;
                }
                continue;
            }
            if parser.state == InputState::Sequence {
                // A final byte ends it; anything longer than the buffer is
                // not a report we know.
                if (0x40..=0x7e).contains(&key) {
                    parser.state = InputState::Normal;
                    parser.sequence[parser.sequence_length] = 0;
                    if key == b'M' || key == b'm' {
                        let body = parser.sequence;
                        let length = parser.sequence_length;
                        self.read_mouse_report(&body[..length]);
                    } else if parser.sequence_length == 0 {
                        // A bare arrow, not a modified one.
                        parser.revision = parser.revision.wrapping_add(1);
                        self.konami_note(key);
                    }
                    continue;
                }
                if parser.sequence_length + 1 < parser.sequence.len() {
                    parser.sequence[parser.sequence_length] = key;
                    parser.sequence_length += 1;
                }
                continue;
            }
            if key == 0x1b {
                parser.state = InputState::Escape;
                continue;
            }

            parser.revision = parser.revision.wrapping_add(1);

            if key == b'b' || key == b'a' {
                self.konami_note(key);
            }
            // Any key ends the intro; the pointer does not.
            self.formation.clear();
            let (notch, step): (&mut i32, i32) = match key {
                b'q' => return false,
                b' ' => {
                    self.paused = !self.paused;
                    continue;
                }
                b'.' => {
                    // One frame of motion, then still again.
                    self.step_once = true;
                    continue;
                }
                b'0' => {
                    self.apply_preset_defaults();
                    continue;
                }
                b'+' | b'=' => {
                    if self.config.birds < MAX_BIRDS {
                        self.config.birds += self.config.birds / 4 + 1;
                        if self.config.birds > MAX_BIRDS {
                            self.config.birds = MAX_BIRDS;
                        }
                        self.population_changed = true;
                    }
                    continue;
                }
                b'-' => {
                    if self.config.birds > 1 {
                        self.config.birds -= self.config.birds / 5 + 1;
                        if self.config.birds < 1 {
                            self.config.birds = 1;
                        }
                        self.population_changed = true;
                    }
                    continue;
                }
                b'h' => {
                    self.legend_enabled = !self.legend_enabled;
                    self.measure_legend();
                    self.update_turn_distances();
                    continue;
                }
                b'k' => {
                    if self.hawk_sets_built && self.config.hawks < MAX_HAWKS {
                        self.config.hawks += 1;
                        self.place_one_hawk((self.config.hawks - 1) as usize);
                    }
                    continue;
                }
                b'K' => {
                    if self.config.hawks > 0 {
                        self.config.hawks -= 1;
                    }
                    continue;
                }
                b'T' => {
                    if self.config.turning_notch < LEGEND_BAR_CELLS {
                        self.config.turning_notch += 1;
                    }
                    continue;
                }
                b't' => {
                    if self.config.turning_notch > 0 {
                        self.config.turning_notch -= 1;
                    }
                    continue;
                }
                b'e' => {
                    self.config.trails = !self.config.trails;
                    continue;
                }
                b'\t' => {
                    self.requested_preset = (self.requested_preset + 1) % PRESET_COUNT;
                    self.apply_preset(self.requested_preset);
                    continue;
                }
                b'B' => (&mut self.config.boundary_notch, 1),
                b'b' => (&mut self.config.boundary_notch, -1),
                b'S' => (&mut self.config.separation_notch, 1),
                b's' => (&mut self.config.separation_notch, -1),
                b'A' => (&mut self.config.alignment_notch, 1),
                b'a' => (&mut self.config.alignment_notch, -1),
                b'P' => (&mut self.config.vision_notch, 1),
                b'p' => (&mut self.config.vision_notch, -1),
                b'V' => (&mut self.config.pace_notch, 1),
                b'v' => (&mut self.config.pace_notch, -1),
                b'G' | b'g' => {
                    // Nothing to avoid with one flock, and no row to show it.
                    if self.config.flocks < 2 {
                        continue;
                    }
                    (&mut self.config.avoid_notch, if key == b'G' { 1 } else { -1 })
                }
                _ => continue,
            };
            *notch = (*notch + step).clamp(0, LEGEND_BAR_CELLS);
            self.apply_notches();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_refuse_overflow_and_empty() {
        let mut at = 0;
        assert_eq!(read_decimal(b"2147483647;", &mut at), Some(i32::MAX));
        assert_eq!(at, 10);
        let mut at = 0;
        assert_eq!(read_decimal(b"2147483648", &mut at), None);
        assert_eq!(at, 0);
        assert_eq!(read_decimal(b";1", &mut 0), None);
    }

    #[test]
    fn a_split_sequence_is_still_one_sequence() {
        let mut sim = Sim::new();
        sim.apply_screen_size(80, 24, 640, 384);
        let mut parser = InputParser::default();
        assert!(sim.handle_input(&mut parser, Some(b"\x1b[<0;1")));
        assert!(!sim.mouse.present);
        assert!(sim.handle_input(&mut parser, Some(b"0;2M")));
        assert!(sim.mouse.present);
        assert_eq!((sim.mouse.x, sim.mouse.y), (76.0, 24.0));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod kitty_reply_tests {
    use super::*;

    #[test]
    fn late_graphics_replies_are_not_keys_at_any_read_or_query_boundary() {
        let reply = b"\x1b_Gi=1919052146;EBADF: q h e K query failed\x1b\\";
        for split in 0..=reply.len() {
            let mut sim = Sim::new();
            sim.clock.seconds = 20.0;
            sim.last_key_at = 3.0;
            let before = sim.config.clone();
            let mut parser = InputParser::default();
            parser.ignore_kitty_replies(&reply[..split]);
            for byte in reply[split..].chunks(1) {
                assert!(sim.handle_input(&mut parser, Some(byte)), "split {split}");
            }
            assert_eq!(sim.config, before, "split {split}");
            assert!(!sim.paused);
            assert_eq!(sim.last_key_at, 3.0);
            assert_eq!(parser.revision, 0);
            assert_eq!(parser.state, InputState::Normal);
            assert!(sim.handle_input(&mut parser, Some(b"e")));
            assert_ne!(sim.config.trails, before.trails);
            assert_eq!(sim.last_key_at, 20.0);
            assert!(!sim.handle_input(&mut parser, Some(b"q")));
        }
    }

    #[test]
    fn another_escape_sequence_can_abandon_an_unterminated_graphics_reply() {
        let mut sim = Sim::new();
        let mut parser = InputParser::default();
        parser.ignore_kitty_replies(b"\x1b_Gi=1919052146;");
        assert!(sim.handle_input(&mut parser, Some(b"\x1b[C")));
        assert_eq!(parser.state, InputState::Normal);
        assert!(!sim.handle_input(&mut parser, Some(b"q")));
    }
}
