//! Decode the generated protocol independently, asserting the raster rather
//! than comparing the encoder with another copy of its implementation.
use rbirds::image::Image;
use rbirds::render::kitty::{KittyError, KittyGraphics};
use rbirds::render::sixel::Sixel;
use rbirds::terminal::{graphics_window, has_sixel, parse_cell_size};

fn number(bytes: &[u8], at: &mut usize) -> usize {
    let start = *at;
    while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
        *at += 1;
    }
    assert!(*at > start, "missing number at {start}");
    std::str::from_utf8(&bytes[start..*at]).unwrap().parse().unwrap()
}

fn decode(bytes: &[u8]) -> (usize, usize, Vec<[u8; 3]>) {
    assert!(bytes.starts_with(b"\x1bP0;1q\"1;1;"));
    assert!(bytes.ends_with(b"\x1b\\"));
    let mut at = b"\x1bP0;1q\"1;1;".len();
    let width = number(bytes, &mut at);
    assert_eq!(bytes[at], b';');
    at += 1;
    let height = number(bytes, &mut at);
    let mut pixels = vec![None; width * height];
    let mut colours = [[0; 3]; 256];
    let (mut x, mut y, mut colour) = (0, 0, 0);
    while at < bytes.len() - 2 {
        let command = bytes[at];
        at += 1;
        match command {
            b'#' => {
                colour = number(bytes, &mut at);
                if bytes[at] == b';' {
                    at += 1;
                    assert_eq!(number(bytes, &mut at), 2);
                    for channel in &mut colours[colour] {
                        assert_eq!(bytes[at], b';');
                        at += 1;
                        *channel = ((number(bytes, &mut at) * 255 + 50) / 100) as u8;
                    }
                }
            }
            b'$' => x = 0,
            b'-' => {
                x = 0;
                y += 6;
            }
            _ => {
                let (count, bits) = if command == b'!' {
                    let count = number(bytes, &mut at);
                    let bits = bytes[at] - b'?';
                    at += 1;
                    (count, bits)
                } else {
                    assert!((b'?'..=b'~').contains(&command));
                    (1, command - b'?')
                };
                for column in x..x + count {
                    assert!(column < width);
                    for bit in 0..6 {
                        if bits & (1 << bit) != 0 {
                            assert!(y + bit < height, "paint past the final partial band");
                            pixels[(y + bit) * width + column] = Some(colours[colour]);
                        }
                    }
                }
                x += count;
            }
        }
    }
    (
        width,
        height,
        pixels.into_iter().map(|p| p.expect("every raster pixel must be repainted")).collect(),
    )
}

#[test]
fn colours_runs_and_partial_bands_decode_to_the_expected_raster() {
    let mut image = Image::alloc(19, 13).unwrap();
    let colours = [[255, 0, 0, 255], [0, 102, 153, 255], [255, 255, 255, 255]];
    let mut expected = Vec::new();
    for y in 0..13 {
        for x in 0..19 {
            let c = colours[if x < 12 { y / 6 } else { (y + x) % 3 }];
            image.pixels[(y * 19 + x) * 4..(y * 19 + x + 1) * 4].copy_from_slice(&c);
            expected.push([c[0], c[1], c[2]]);
        }
    }
    let mut encoder = Sixel::default();
    let bytes = encoder.encode(&image).unwrap();
    assert!(bytes.contains(&b'!'), "long runs must be compressed");
    assert_eq!(decode(bytes), (19, 13, expected));
}

#[test]
fn alpha_is_flattened_and_a_second_frame_erases_old_birds() {
    let mut image = Image::alloc(1, 1).unwrap();
    let mut encoder = Sixel::default();
    image.pixels.copy_from_slice(&[255, 0, 0, 128]);
    assert_eq!(decode(encoder.encode(&image).unwrap()).2, [[153, 0, 0]]);
    image.pixels.copy_from_slice(&[255, 0, 0, 255]);
    assert_eq!(decode(encoder.encode(&image).unwrap()).2, [[255, 0, 0]]);
    image.pixels.copy_from_slice(&[255, 0, 0, 0]);
    assert_eq!(decode(encoder.encode(&image).unwrap()).2, [[18, 18, 23]]);
    let resized = Image::alloc(3, 7).unwrap();
    assert_eq!(decode(encoder.encode(&resized).unwrap()), (3, 7, vec![[18, 18, 23]; 21]));
}

#[test]
fn malformed_images_are_rejected_without_queuing_a_partial_frame() {
    let mut encoder = Sixel::default();
    let mut output = KittyGraphics::new(1).unwrap();
    output.write_raw(b"previous").unwrap();
    for image in [
        Image::empty(),
        Image { width: 2, height: 1, pixels: vec![0; 4] },
        Image { width: -1, height: 1, pixels: vec![] },
    ] {
        assert_eq!(encoder.queue(&mut output, &image), Err(KittyError::Argument));
        assert_eq!(output.buffer(), b"previous");
    }
}

#[test]
fn frame_queue_composes_birds_repaints_after_panel_removal_and_resizes() {
    use rbirds::render::Renderer;
    use rbirds::simulation::{Bird, RenderMode, Sim};
    let mut sim = Sim::new();
    sim.render_mode = RenderMode::Sixel;
    sim.config.birds = 1;
    sim.config.palette = 1;
    sim.legend_enabled = true;
    sim.settle_the_bird_size();
    sim.apply_screen_size(80, 24, 800, 480);
    let mut renderer = Renderer::default();
    // A one-pixel red sprite makes the composition's expected raster exact,
    // independent of orientation/wing catalogue indexing and rasterization.
    for sprite in &mut renderer.sprites {
        *sprite = Image { width: 1, height: 1, pixels: vec![255, 0, 0, 255] };
    }
    let mut bird = Bird { x: 700.0, y: 460.0, ..Bird::default() };
    let mut output = KittyGraphics::new(1).unwrap();
    for step in 0..3 {
        if step == 1 {
            sim.legend_enabled = false;
            sim.apply_screen_size(80, 24, 800, 480);
            bird.x = 3.0;
            bird.y = 4.0;
        } else if step == 2 {
            sim.apply_screen_size(40, 14, 400, 280);
        }
        output.clear();
        renderer.queue_render_frame(&mut output, &sim, &[bird]).unwrap();
        let bytes = output.buffer();
        assert!(bytes.starts_with(b"\x1b[?2026h\x1b[2J\x1b[H"));
        assert!(bytes.ends_with(b"\x1b[?2026l"));
        let start = bytes.windows(2).position(|s| s == b"\x1bP").unwrap();
        let end = start + bytes[start..].windows(2).position(|s| s == b"\x1b\\").unwrap() + 2;
        let (width, height) = (sim.screen.width as usize, sim.screen.height as usize);
        let mut expected = vec![[18, 18, 23]; width * height];
        expected[bird.y as usize * width + bird.x as usize] = [255, 0, 0];
        assert_eq!(decode(&bytes[start..end]), (width, height, expected));
        assert_eq!(renderer.legend_drawn, step == 0);
    }
}

#[test]
fn terminal_capabilities_and_virtual_cell_sizes_are_parsed_precisely() {
    assert!(has_sixel(b"\x1b[?61;4;6;22c"));
    for reply in [&b"\x1b[?64;6;22c"[..], b"\x1b[?61;14c", b"\x1b[?61;4", b"garbage"] {
        assert!(!has_sixel(reply), "{reply:?}");
    }
    assert_eq!(parse_cell_size(b"\x1b[6;20;10t"), Some((10, 20)));
    for reply in [
        &b"\x1b[6;0;10t"[..],
        b"\x1b[6;20;0t",
        b"\x1b[6;20;65536t",
        b"\x1b[4;20;10t",
        b"\x1b[6;20;10",
        b"\x1b[6;20;10;5t",
    ] {
        assert_eq!(parse_cell_size(reply), None);
    }
    let size = rbirds::platform::WinSize { row: 24, col: 80, xpixel: 1234, ypixel: 5678 };
    assert_eq!(graphics_window(size, None), size);
    let virtual_size = graphics_window(size, Some((10, 20)));
    assert_eq!((virtual_size.xpixel, virtual_size.ypixel), (800, 480));
}

#[test]
fn sixel_is_an_explicit_renderer_and_the_default_stays_braille() {
    use rbirds::simulation::{RenderMode, Sim};
    let mut program = rbirds::app::Program::new();
    let mut stdout = rbirds::stdio::CStdout::captured();
    let argv = ["rbirds", "--render", "sixel"].map(std::ffi::OsString::from);
    assert_eq!(rbirds::app::read_options(&mut program, &argv, &mut stdout), Ok(()));
    assert_eq!(program.sim.render_mode, RenderMode::Sixel);
    assert_eq!(Sim::new().live_render_mode(), RenderMode::Braille);
}

/// Big-flock mode writes the bands on several threads (docs/DEVIATIONS.md
/// D-006): the same bytes as one thread, for any count, any height (a short
/// last band, or a single band) and an encoder whose storage was last used
/// for a different image.
#[test]
fn every_thread_count_writes_the_same_image() {
    use rbirds::render::compose::compose_onto;
    use rbirds::simulation::{Bird, Sim};
    let mut state = 0x2545_f491_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut images = Vec::new();
    for (width, height) in [(1, 1), (7, 5), (64, 6), (33, 13), (200, 61), (320, 180)] {
        let mut image = Image::alloc(width, height).unwrap();
        for pixel in image.pixels.chunks_exact_mut(4) {
            let r = next();
            // Mostly background, some one flat colour, the rest anything.
            match r % 4 {
                0 | 1 => pixel.copy_from_slice(&[18, 18, 23, 255]),
                2 => pixel.copy_from_slice(&[250, 160, 40, 255]),
                _ => pixel.copy_from_slice(&r.to_le_bytes()),
            }
        }
        images.push(image);
    }
    // And a frame of a flock, as queue_sixel_frame composes it.
    let mut sim = Sim::new();
    sim.config.birds = 3000;
    sim.config.palette = 1;
    sim.config.trails = true;
    sim.settle_the_bird_size();
    sim.apply_notches();
    sim.apply_screen_size(100, 30, 800, 480);
    let mut frames = rbirds::sprites::empty_catalogue();
    sim.rasterise_sprites(&mut frames, None, b"rbirds").unwrap();
    let mut birds = vec![Bird::default(); 3000];
    sim.rng.seed(5);
    sim.initialize_birds(&mut birds);
    let mut canvas = Image::alloc(800, 480).unwrap();
    compose_onto(&sim, &mut canvas, &frames, &birds, true);
    images.push(canvas);

    let serial: Vec<Vec<u8>> =
        images.iter().map(|image| Sixel::default().encode(image).unwrap().to_vec()).collect();
    for threads in [2, 3, 4, 7, 16] {
        let mut encoder = Sixel::default();
        // Largest first, then smaller and larger again, reusing the storage.
        for (image, expected) in images.iter().zip(&serial).rev().chain(images.iter().zip(&serial))
        {
            let bytes = encoder.encode_in_parallel(threads, image).unwrap();
            assert_eq!(bytes, &expected[..], "{}x{} on {threads}", image.width, image.height);
        }
    }
    assert_eq!(decode(&serial[3]).0, 33);
}
