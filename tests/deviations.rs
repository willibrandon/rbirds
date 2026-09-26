//! Regression tests for the proposed deviations in docs/DEVIATIONS.md: the
//! places where the reference's behavior is undefined and the port's safe
//! behavior is fixed here so it cannot drift.

use rbirds::image::Image;
use rbirds::render::Renderer;
use rbirds::render::compose::compose_onto;
use rbirds::render::kitty::KittyGraphics;
use rbirds::simulation::{Bird, RenderMode, Sim};

fn flock(sim: &mut Sim, count: usize) -> Vec<Bird> {
    sim.settle_the_bird_size();
    sim.config.palette = 1;
    sim.apply_screen_size(100, 30, 800, 480);
    sim.config.birds = count as i32;
    sim.rng.seed(4);
    let mut birds = vec![Bird::default(); count];
    sim.initialize_birds(&mut birds);
    birds
}

/// D-001: `+` then `q` in one blocked write leaves `config.birds` above the
/// arrays when the snapshot is composed. The C reads past its array; the
/// port draws the birds that exist, exactly as it would with the count
/// matching them.
#[test]
fn d001_a_count_above_the_arrays_draws_the_birds_that_exist() {
    let mut sim = Sim::new();
    let birds = flock(&mut sim, 40);
    let mut frames = rbirds::sprites::empty_catalogue();
    sim.rasterise_sprites(&mut frames, None, b"rbirds").expect("sprites");

    let mut expected = Image::alloc(800, 480).expect("canvas");
    compose_onto(&sim, &mut expected, &frames, &birds, true);

    sim.config.birds = 51; // `+` on 40 birds: 40 / 4 + 1 more.
    let mut actual = Image::alloc(800, 480).expect("canvas");
    compose_onto(&sim, &mut actual, &frames, &birds, true);
    assert_eq!(actual, expected);

    // The Kitty placements likewise.
    sim.render_mode = RenderMode::Kitty;
    let mut renderer = Renderer::default();
    let mut grown = KittyGraphics::new(1).expect("graphics");
    renderer.queue_render_frame(&mut grown, &sim, &birds).expect("frame");
    sim.config.birds = 40;
    let mut matching = KittyGraphics::new(1).expect("graphics");
    renderer.queue_render_frame(&mut matching, &sim, &birds).expect("frame");
    assert_eq!(grown.buffer(), matching.buffer());
}
