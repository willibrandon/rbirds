use rbirds::config::ROTATION_FRAMES;
use rbirds::render::{Renderer, atlas::Atlas, compose::upload_sprite_sets, kitty::KittyGraphics};
use rbirds::simulation::{Bird, RenderMode, Sim};
fn main() {
    let mut sim = Sim::new();
    sim.render_mode = RenderMode::Kitty;
    sim.config.birds = 240;
    sim.config.palette = 1;
    sim.config.trails = true;
    sim.config.hawks = 4;
    sim.config.bird_size = std::env::args().nth(2).unwrap().parse().unwrap();
    sim.apply_screen_size(100, 32, 1400, 800);
    let mut renderer = Renderer::default();
    sim.rasterise_sprites(&mut renderer.sprites, None, b"rbirds").unwrap();
    let mut birds = vec![Bird::default(); 240];
    for (i, bird) in birds.iter_mut().enumerate() {
        bird.x = (i % 20 * 60 + 10) as f64;
        bird.y = (i / 20 * 50 + 10) as f64;
        bird.frame = (i % 60) as i32;
        bird.shade = (i % sim.palette_shades() as usize) as i32;
        bird.wing = (i % 4) as i32;
        bird.layer = (i % 3 == 0) as i32;
        bird.trail_held = 3;
        bird.trail_at = 0;
        bird.trail_x = [bird.x + 1., bird.x + 3., bird.x + 5.];
        bird.trail_y = [bird.y + 1., bird.y + 3., bird.y + 5.];
    }
    for (i, hawk) in sim.hawks.iter_mut().enumerate() {
        hawk.x = 200. + i as f64 * 150.;
        hawk.y = 100.;
        hawk.frame = i as i32 * 15;
        hawk.wing = i as i32;
    }
    let mut graphics = KittyGraphics::new(1).unwrap();
    if std::env::args().nth(1).unwrap() == "atlas" {
        renderer.atlas = Some(
            Atlas::upload(
                &mut graphics,
                &renderer.sprites[..(sim.sprite_set_count() * ROTATION_FRAMES) as usize],
            )
            .unwrap(),
        );
    } else {
        upload_sprite_sets(&sim, &mut graphics, &renderer.sprites).unwrap();
    }
    renderer.queue_render_frame(&mut graphics, &sim, &birds).unwrap();
    graphics.flush().unwrap();
}
