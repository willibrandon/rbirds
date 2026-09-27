//! A paused live frame may be reused only while its displayed state is unchanged.
use rbirds::live::{Frame, LiveLoop};
use rbirds::platform::{Timespec, WinSize};
use rbirds::render::{Renderer, kitty::KittyGraphics};
use rbirds::simulation::{Bird, RenderMode, Sim};
use rbirds::spatial_grid::SpatialGrid;

struct Playback {
    sim: Sim,
    renderer: Renderer,
    live: LiveLoop,
    birds: Vec<Bird>,
    snapshot: Vec<Bird>,
    grid: SpatialGrid,
    graphics: KittyGraphics,
}

impl Playback {
    fn new(mode: RenderMode, reuse: bool, raster: bool) -> Self {
        let mut sim = Sim::new();
        sim.render_mode = mode;
        sim.config.birds = 24;
        sim.config.palette = 1;
        sim.config.hawks = 2;
        sim.config.flocks = 2;
        sim.config.trails = true;
        sim.deep_look = true;
        sim.legend_enabled = true;
        sim.hawk_sets_built = true;
        sim.settle_the_bird_size();
        sim.apply_notches();
        sim.apply_screen_size(100, 30, 800, 480);
        sim.rng.seed(42);
        let mut renderer = Renderer::default();
        renderer.incremental_legend = true;
        renderer.kitty_raster = raster.then(rbirds::render::compose::KittyRaster::default);
        if mode == RenderMode::Kitty && !raster {
            sim.rasterise_sprites(&mut renderer.sprites, None, b"rbirds").unwrap();
        } else {
            renderer.prepare_text_renderer(&mut sim, None, b"rbirds").unwrap();
        }
        let mut birds = vec![Bird::default(); 24];
        sim.initialize_birds(&mut birds);
        sim.place_hawks();
        let snapshot = birds.clone();
        let mut live = LiveLoop::new(Timespec::default(), 24);
        live.reuse_paused_frame = reuse;
        Self {
            sim,
            renderer,
            live,
            birds,
            snapshot,
            grid: SpatialGrid::new(12).unwrap(),
            graphics: KittyGraphics::new(1).unwrap(),
        }
    }

    fn tick(&mut self, millis: i64, keys: Option<&[u8]>, window: WinSize) -> Frame {
        self.graphics.clear();
        self.live
            .frame(
                &mut self.sim,
                &mut self.renderer,
                &mut self.graphics,
                &mut self.birds,
                &mut self.snapshot,
                &mut self.grid,
                keys,
                Timespec { tv_sec: millis / 1000, tv_nsec: (millis % 1000) * 1_000_000 },
                window,
            )
            .unwrap()
    }
}

#[test]
fn paused_reuse_preserves_controls_pixels_simulation_and_outro_in_every_renderer() {
    for (mode, raster) in [
        (RenderMode::Braille, false),
        (RenderMode::Blocks, false),
        (RenderMode::Sextants, false),
        (RenderMode::Kitty, false),
        (RenderMode::Kitty, true),
        (RenderMode::Sixel, false),
    ] {
        let mut full = Playback::new(mode, false, raster);
        let mut reuse = Playback::new(mode, true, raster);
        let mut window = WinSize { col: 100, row: 30, xpixel: 800, ypixel: 480 };
        let steps: &[(i64, Option<&[u8]>, Frame)] = &[
            (16, None, Frame::Drawn),
            (32, Some(b" "), Frame::Drawn),
            (48, None, Frame::Idle),
            (64, Some(b"."), Frame::Drawn),
            (80, None, Frame::Idle),
            (96, Some(b"e+k"), Frame::Drawn),
            (112, None, Frame::Idle),
            (128, Some(b"h"), Frame::Drawn),
            (144, None, Frame::Drawn), // resize below
            (160, Some(b"\x1b[<0;"), Frame::Idle),
            (176, Some(b"10;3M"), Frame::Idle),
            (192, None, Frame::Idle),
            (208, Some(b"h"), Frame::Drawn),
            (65_000, None, Frame::Drawn), // visible autopilot slider
            (65_016, None, Frame::Idle),
            (69_100, None, Frame::Drawn), // next autopilot change
            (69_116, Some(b" "), Frame::Drawn),
            (69_132, None, Frame::Drawn),
            (69_148, Some(b" "), Frame::Drawn),
            (69_164, None, Frame::Drawn), // input consumed while flushing
            (69_180, None, Frame::Idle),
            // Bare CSI finals also feed the Konami recognizer. Completing it
            // can reposition hawks even when all four are already present.
            (
                69_188,
                Some(b"\x1b[A\x1b[A\x1b[B\x1b[B\x1b[D\x1b[C\x1b[D\x1b[C\x1b[b\x1b[a"),
                Frame::Drawn,
            ),
            (69_192, None, Frame::Idle),
            (69_196, Some(b"q"), Frame::Drawn),
            (69_212, None, Frame::Drawn),
            (70_000, None, Frame::Over),
        ];
        for (index, &(millis, keys, expected)) in steps.iter().enumerate() {
            if index == 8 {
                window = WinSize { col: 120, row: 32, xpixel: 960, ypixel: 512 };
            }
            if index == 19 {
                // This input has the same clock value as the preceding tick.
                // A timestamp alone cannot detect it as a new change.
                for playback in [&mut full, &mut reuse] {
                    assert!(playback.sim.handle_input(&mut playback.live.parser, Some(b"k")));
                }
            }
            let reference = full.tick(millis, keys, window);
            let actual = reuse.tick(millis, keys, window);
            assert_eq!(actual, expected, "{mode:?} step {index}");
            assert_eq!(reference, if expected == Frame::Over { Frame::Over } else { Frame::Drawn });
            assert_eq!(reuse.birds, full.birds, "{mode:?} step {index}");
            assert_eq!(
                format!("{:?}", reuse.sim),
                format!("{:?}", full.sim),
                "{mode:?} step {index}"
            );
            assert_eq!(reuse.renderer.canvas, full.renderer.canvas, "{mode:?} step {index}");
            if expected == Frame::Idle {
                assert!(reuse.graphics.is_empty(), "{mode:?} step {index}");
            } else {
                // Repainting unchanged frames advances the raster's image-ID
                // bank; skipping them does not. Compare equivalent plane IDs.
                let normalize = |bytes: &[u8]| {
                    if raster {
                        String::from_utf8_lossy(bytes)
                            .replace(",i=3,", ",i=1,")
                            .replace(",i=4,", ",i=2,")
                            .into_bytes()
                    } else {
                        bytes.to_vec()
                    }
                };
                assert_eq!(
                    normalize(reuse.graphics.buffer()),
                    normalize(full.graphics.buffer()),
                    "{mode:?} step {index}"
                );
            }
        }
    }
}
