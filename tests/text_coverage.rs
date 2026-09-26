//! Sparse composition must produce exactly what the full canvas reader sees,
//! including old pixels being erased and non-integral terminal cell sizes.
use rbirds::render::Renderer;
use rbirds::render::cells::Cells;
use rbirds::render::compose::text_style;
use rbirds::render::kitty::KittyGraphics;
use rbirds::simulation::{Bird, RenderMode, Sim};

#[test]
fn sprite_coverage_matches_full_canvas_reading_through_moves_and_resizes() {
    for mode in [RenderMode::Braille, RenderMode::Sextants, RenderMode::Blocks] {
        let mut sim = Sim::new();
        sim.render_mode = mode;
        sim.config.birds = 3;
        sim.config.trails = true;
        sim.config.palette = 1;
        sim.settle_the_bird_size();
        let mut renderer = Renderer::default();
        renderer.prepare_text_renderer(&mut sim, None, b"rbirds").unwrap();
        let mut reference = Cells::new(renderer.cells.truecolor).unwrap();
        let mut graphics = KittyGraphics::new(1).unwrap();
        let mut birds = vec![Bird::default(); 3];
        for (cols, rows, width, height) in [(100, 30, 803, 483), (31, 15, 40, 24), (7, 5, 23, 22)] {
            sim.apply_screen_size(cols, rows, width, height);
            reference.resize(cols, rows).unwrap();
            for (x, y) in
                [(10., 10.), (-5., -5.), (width as f64 - 4., height as f64 - 4.), (10000., 10000.)]
            {
                birds[0].x = x;
                birds[0].y = y;
                birds[1].x = x + 10.;
                birds[1].y = y + 5.;
                birds[2].x = x - 10.;
                birds[2].y = y - 5.;
                birds[0].trail_held = 1;
                birds[0].trail_x[0] = 5.;
                birds[0].trail_y[0] = 8.;
                birds[0].trail_at = 1;
                graphics.clear();
                renderer.queue_render_frame(&mut graphics, &sim, &birds).unwrap();
                reference.read(
                    text_style(mode),
                    &renderer.canvas,
                    sim.screen.cell_width,
                    sim.screen.cell_height,
                );
                reference.emit().unwrap();
                assert_eq!(
                    renderer.cells.before, reference.before,
                    "{mode:?}, {cols}x{rows}, {x},{y}"
                );
                assert_eq!(renderer.cells.text, reference.text);
            }
        }
    }
}
