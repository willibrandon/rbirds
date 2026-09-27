use rbirds::render::Renderer;
use rbirds::render::kitty::KittyGraphics;
use rbirds::simulation::{RenderMode, Sim};

#[derive(Debug, PartialEq, Eq)]
struct Screen {
    cols: usize,
    rows: usize,
    cursor: (usize, usize),
    cells: Vec<char>,
}

impl Screen {
    fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows, cursor: (0, 0), cells: vec![' '; cols * rows] }
    }

    // The panel's protocol is cursor positioning, erase-to-end-of-line and
    // single-cell Unicode text. Compare the retained terminal state, including
    // the cursor, rather than requiring identical update commands.
    fn apply(&mut self, bytes: &[u8]) {
        let mut chars = std::str::from_utf8(bytes).unwrap().chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                assert_eq!(chars.next(), Some('['));
                let mut parameters = String::new();
                while chars.peek().is_some_and(|c| c.is_ascii_digit() || *c == ';') {
                    parameters.push(chars.next().unwrap());
                }
                match chars.next().unwrap() {
                    'H' => {
                        let values: Vec<usize> =
                            parameters.split(';').map(|s| s.parse().unwrap()).collect();
                        self.cursor = (values[1] - 1, values[0] - 1);
                    }
                    'K' => {
                        assert!(parameters.is_empty());
                        for col in self.cursor.0..self.cols {
                            self.cells[self.cursor.1 * self.cols + col] = ' ';
                        }
                    }
                    other => panic!("unexpected panel control {other}"),
                }
            } else {
                assert!(self.cursor.0 < self.cols && self.cursor.1 < self.rows);
                self.cells[self.cursor.1 * self.cols + self.cursor.0] = c;
                self.cursor.0 += 1;
            }
        }
    }
}

fn queued(renderer: &mut Renderer, sim: &Sim) -> Vec<u8> {
    let mut graphics = KittyGraphics::new(1).unwrap();
    renderer.queue_legend(&mut graphics, sim).unwrap();
    graphics.buffer().to_vec()
}

#[test]
fn incremental_panel_preserves_text_and_cursor_through_changes_and_resize() {
    for mode in [RenderMode::Braille, RenderMode::Sextants, RenderMode::Blocks, RenderMode::Kitty] {
        let mut sim = Sim::new();
        sim.render_mode = mode;
        sim.legend_enabled = true;
        sim.apply_screen_size(100, 30, 800, 480);
        let mut full = Renderer::default();
        let mut delta = Renderer::default();
        delta.incremental_legend = true;
        let mut expected = Screen::new(100, 30);
        let mut actual = Screen::new(100, 30);
        for step in 0..14 {
            match step {
                2 => {
                    full.stats.frame_ms = 2.3;
                    full.stats.bytes = 4096.0;
                    full.stats.rate = 60.0;
                    delta.stats = full.stats;
                }
                3 => {
                    sim.config.pace_notch += 1;
                    sim.config.turning_notch += 1;
                    sim.apply_notches();
                }
                4 => sim.config.flocks = 3,
                5 => sim.config.flocks = 1,
                6 => sim.legend_enabled = false,
                8 => sim.legend_enabled = true,
                9 => {
                    sim.apply_screen_size(80, 25, 800, 500);
                    expected = Screen::new(80, 25);
                    actual = Screen::new(80, 25);
                }
                10 => {
                    full.legend_drawn = false;
                    delta.legend_drawn = false;
                    expected = Screen::new(80, 25);
                    actual = Screen::new(80, 25);
                }
                11 => delta.incremental_legend = false,
                12 => delta.incremental_legend = true,
                _ => {}
            }
            sim.measure_legend();
            // Kitty placements can leave the cursor anywhere before the panel.
            expected.cursor = (60, 20);
            actual.cursor = (60, 20);
            let reference = queued(&mut full, &sim);
            let update = queued(&mut delta, &sim);
            expected.apply(&reference);
            actual.apply(&update);
            assert_eq!(actual, expected, "{mode:?}, step {step}");
            if step == 1 || step == 2 {
                assert!(update.len() < reference.len() / 2);
            }
        }
    }
}

#[test]
fn sixel_repaints_the_panel_after_every_raster() {
    let mut sim = Sim::new();
    sim.render_mode = RenderMode::Sixel;
    sim.legend_enabled = true;
    sim.apply_screen_size(100, 30, 800, 480);
    let mut full = Renderer::default();
    let mut delta = Renderer::default();
    delta.incremental_legend = true;
    for _ in 0..3 {
        assert_eq!(queued(&mut delta, &sim), queued(&mut full, &sim));
    }
}
