//! Decode the generated protocol independently, asserting the raster rather
//! than comparing the encoder with another copy of its implementation.
use rbirds::image::Image;
use rbirds::render::kitty::{KittyError, KittyGraphics};
use rbirds::render::sixel::Sixel;
use rbirds::terminal::{
    cell_size_from_window, graphics_window, has_sixel, is_iterm2, parse_cell_size,
};

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
    for erase in [false, true] {
        let mut sim = Sim::new();
        sim.render_mode = RenderMode::Sixel;
        sim.config.birds = 1;
        sim.config.palette = 1;
        sim.legend_enabled = true;
        sim.settle_the_bird_size();
        sim.apply_screen_size(80, 24, 800, 480);
        let mut renderer = Renderer::default();
        renderer.erase_sixel_before_frame = erase;
        // A one-pixel red sprite makes the composition's expected raster exact,
        // independent of orientation/wing catalogue indexing and rasterization.
        for sprite in &mut renderer.sprites {
            *sprite = Image { width: 1, height: 1, pixels: vec![255, 0, 0, 255] };
        }
        let mut bird = Bird { x: 700.0, y: 460.0, ..Bird::default() };
        let mut output = KittyGraphics::new(1).unwrap();
        for step in 0..4 {
            if step == 2 {
                sim.legend_enabled = false;
                sim.apply_screen_size(80, 24, 800, 480);
                bird.x = 3.0;
                bird.y = 4.0;
            } else if step == 3 {
                sim.apply_screen_size(40, 14, 400, 280);
            }
            output.clear();
            renderer.queue_render_frame(&mut output, &sim, &[bird]).unwrap();
            let bytes = output.buffer();
            let prefix = if erase || step != 1 {
                &b"\x1b[?2026h\x1b[2J\x1b[H"[..]
            } else {
                &b"\x1b[?2026h\x1b[H"[..]
            };
            assert!(bytes.starts_with(prefix));
            assert!(bytes.ends_with(b"\x1b[?2026l"));
            let start = bytes.windows(2).position(|s| s == b"\x1bP").unwrap();
            let end = start + bytes[start..].windows(2).position(|s| s == b"\x1b\\").unwrap() + 2;
            let (width, height) = (sim.screen.width as usize, sim.screen.height as usize);
            let mut expected = vec![[18, 18, 23]; width * height];
            expected[bird.y as usize * width + bird.x as usize] = [255, 0, 0];
            assert_eq!(decode(&bytes[start..end]), (width, height, expected));
            assert_eq!(renderer.legend_drawn, step < 2);
        }
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
    assert_eq!(cell_size_from_window(size), None);
    let exact = rbirds::platform::WinSize { row: 32, col: 100, xpixel: 1400, ypixel: 1088 };
    assert_eq!(cell_size_from_window(exact), Some((14, 34)));
    for invalid in [
        rbirds::platform::WinSize { col: 0, ..exact },
        rbirds::platform::WinSize { row: 0, ..exact },
        rbirds::platform::WinSize { xpixel: 0, ..exact },
        rbirds::platform::WinSize { ypixel: 0, ..exact },
        rbirds::platform::WinSize { xpixel: 1401, ..exact },
        rbirds::platform::WinSize { ypixel: 1087, ..exact },
    ] {
        assert_eq!(cell_size_from_window(invalid), None);
    }
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

#[test]
fn iterm_workaround_requires_its_terminal_version_response() {
    assert!(is_iterm2(b"\x1bP>|iTerm2 3.6.6\x1b\\"));
    for reply in [
        &b""[..],
        b"\x1bP>|WezTerm 20240203\x1b\\",
        b"\x1bP>|iTerm2-other 3.6.6\x1b\\",
        b"\x1bP>|iTerm2 3.6.6",
        b"iTerm2 3.6.6",
    ] {
        assert!(!is_iterm2(reply), "{reply:?}");
    }
}
