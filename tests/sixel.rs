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
    decode_with_colour_selection(bytes, true)
}

fn decode_with_colour_selection(
    bytes: &[u8],
    definitions_select: bool,
) -> (usize, usize, Vec<[u8; 3]>) {
    assert!(bytes.starts_with(b"\x1bP0;1q\"1;1;"));
    assert!(bytes.ends_with(b"\x1b\\"));
    let mut at = b"\x1bP0;1q\"1;1;".len();
    let width = number(bytes, &mut at);
    assert_eq!(bytes[at], b';');
    at += 1;
    let height = number(bytes, &mut at);
    let mut pixels = vec![None; width * height];
    let mut colours = [[0; 3]; 256];
    let (mut x, mut y) = (0, 0);
    // WezTerm starts green and changes the drawing colour only on an explicit
    // selection. A palette definition alone leaves the current colour intact.
    let mut foreground = [0, 255, 0];
    while at < bytes.len() - 2 {
        let command = bytes[at];
        at += 1;
        match command {
            b'#' => {
                let colour = number(bytes, &mut at);
                if bytes[at] == b';' {
                    at += 1;
                    assert_eq!(number(bytes, &mut at), 2);
                    for channel in &mut colours[colour] {
                        assert_eq!(bytes[at], b';');
                        at += 1;
                        *channel = ((number(bytes, &mut at) * 255 + 50) / 100) as u8;
                    }
                    if definitions_select {
                        foreground = colours[colour];
                    }
                } else {
                    foreground = colours[colour];
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
                            pixels[(y + bit) * width + column] = Some(foreground);
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

// Interpret the frame's cursor moves, opaque block fills and Sixel rasters.
// Erased text backgrounds remain None: a transparent terminal needs an
// opaque glyph or raster at every pixel, not merely the right SGR colour.
fn decode_frame(bytes: &[u8], width: usize, height: usize, cw: usize, ch: usize) -> Vec<[u8; 3]> {
    let mut pixels = vec![None; width * height];
    let (mut at, mut row, mut col) = (0, 0, 0);
    let mut foreground = None;
    let mut opaque_backdrop = false;
    // prepare_sixel enables DECSDM. iTerm then anchors images at the screen
    // origin, ignoring CUP until the renderer resets that mode for a crop.
    let mut display_mode = true;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"\x1bP") {
            if opaque_backdrop {
                assert!(
                    pixels.iter().all(Option::is_some),
                    "paint the whole opaque backdrop before replacing its image"
                );
            }
            let end = at + bytes[at..].windows(2).position(|s| s == b"\x1b\\").unwrap() + 2;
            let (w, h, raster) = decode(&bytes[at..end]);
            let (image_row, image_col) = if display_mode { (0, 0) } else { (row, col) };
            for y in 0..h {
                for x in 0..w {
                    pixels[(image_row * ch + y) * width + image_col * cw + x] =
                        Some(raster[y * w + x]);
                }
            }
            at = end;
            continue;
        }
        let count = if bytes[at..].starts_with("█".as_bytes()) {
            at += "█".len();
            1
        } else {
            assert!(bytes[at..].starts_with(b"\x1b["), "unexpected text at {at}");
            let start = at + 2;
            let end = start + bytes[start..].iter().position(u8::is_ascii_alphabetic).unwrap();
            let parameters = &bytes[start..end];
            at = end + 1;
            if parameters.starts_with(b"?") {
                if parameters == b"?80" {
                    assert!(matches!(bytes[end], b'h' | b'l'));
                    display_mode = bytes[end] == b'h';
                }
                continue;
            }
            let values: Vec<usize> = if parameters.is_empty() {
                Vec::new()
            } else {
                std::str::from_utf8(parameters)
                    .unwrap()
                    .split(';')
                    .map(|s| s.parse().unwrap())
                    .collect()
            };
            match bytes[end] {
                b'H' => {
                    row = values.first().copied().unwrap_or(1) - 1;
                    col = values.get(1).copied().unwrap_or(1) - 1;
                    continue;
                }
                b'm' => {
                    foreground = values
                        .windows(5)
                        .find(|v| v[..2] == [38, 2])
                        .map(|v| [v[2] as u8, v[3] as u8, v[4] as u8]);
                    continue;
                }
                b'J' => {
                    pixels.fill(None);
                    continue;
                }
                b'b' => values[0],
                other => panic!("unexpected command {other}"),
            }
        };
        opaque_backdrop = true;
        for _ in 0..count {
            for y in row * ch..(row + 1) * ch {
                for x in col * cw..(col + 1) * cw {
                    pixels[y * width + x] = foreground;
                }
            }
            col += 1;
        }
    }
    assert!(display_mode, "restore DECSDM after cursor-positioned images");
    pixels.into_iter().map(|p| p.expect("every pixel must remain opaque")).collect()
}

#[test]
fn cropped_frames_match_full_rasters_through_clipping_trails_hawks_and_resize() {
    use rbirds::render::Renderer;
    use rbirds::simulation::{Bird, RenderMode, Sim};
    let mut sim = Sim::new();
    sim.render_mode = RenderMode::Sixel;
    sim.config.birds = 3;
    sim.config.trails = true;
    sim.config.hawks = 1;
    sim.config.flocks = 2;
    sim.config.palette = 1;
    sim.deep_look = true;
    sim.settle_the_bird_size();
    let mut renderer = Renderer::default();
    renderer.erase_sixel_before_frame = true;
    renderer.crop_sixel_frames = true;
    renderer.prepare_text_renderer(&mut sim, None, b"rbirds").unwrap();
    let mut output = KittyGraphics::new(1).unwrap();
    let mut reference = Sixel::default();
    let mut birds = vec![Bird::default(); 3];
    for (cols, rows, width, height) in
        [(100, 32, 1400, 1088), (100, 32, 1401, 1089), (8, 5, 40, 30)]
    {
        sim.apply_screen_size(cols, rows, width, height);
        for (x, y) in
            [(300., 250.), (-5., -5.), (width as f64 - 4., height as f64 - 4.), (10000., 10000.)]
        {
            for (i, bird) in birds.iter_mut().enumerate() {
                bird.x = x + i as f64 * 10.;
                bird.y = y + i as f64 * 5.;
                bird.layer = (i % 2) as i32;
                bird.trail_held = 1;
                bird.trail_at = 1;
                bird.trail_x[0] = x - 20.;
                bird.trail_y[0] = y - 10.;
            }
            sim.hawks[0].x = x + 30.;
            sim.hawks[0].y = y - 30.;
            output.clear();
            renderer.queue_render_frame(&mut output, &sim, &birds).unwrap();
            let expected = decode(reference.encode(&renderer.canvas).unwrap()).2;
            let actual = decode_frame(
                output.buffer(),
                width as usize,
                height as usize,
                sim.screen.cell_width as usize,
                sim.screen.cell_height as usize,
            );
            assert_eq!(actual.len(), expected.len());
            if let Some(at) = actual.iter().zip(&expected).position(|(a, e)| a != e) {
                panic!(
                    "{cols}x{rows}, {width}x{height}, bird {x},{y}: pixel {},{}: {:?} != {:?}",
                    at % width as usize,
                    at / width as usize,
                    actual[at],
                    expected[at]
                );
            }
        }
    }
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
fn palette_definitions_do_not_need_to_select_the_drawing_colour() {
    let mut image = Image::alloc(2, 13).unwrap();
    let mut expected = vec![[18, 18, 23]; 26];
    for row in 6..13 {
        for (column, colour) in [[255, 0, 0], [0, 102, 153]].into_iter().enumerate() {
            let at = row * 2 + column;
            image.pixels[at * 4..at * 4 + 4]
                .copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
            expected[at] = colour;
        }
    }
    let mut encoder = Sixel::default();
    for _ in 0..2 {
        let bytes = encoder.encode(&image).unwrap();
        assert_eq!(decode_with_colour_selection(bytes, false), (2, 13, expected.clone()));
        assert_eq!(decode(bytes), (2, 13, expected.clone()));
    }
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
        renderer.crop_sixel_frames = erase;
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
            if erase {
                let (cw, ch) = (sim.screen.cell_width as usize, sim.screen.cell_height as usize);
                let (x, y) = (bird.x as usize / cw * cw, bird.y as usize / ch * ch);
                let cursor = format!("\x1b[{};{}H", y / ch + 1, x / cw + 1);
                assert!(bytes[..start].ends_with(cursor.as_bytes()));
                let (w, h, pixels) = decode(&bytes[start..end]);
                assert_eq!((w, h), (cw, ch));
                let mut actual = vec![[18, 18, 23]; width * height];
                for row in 0..h {
                    actual[(y + row) * width + x..(y + row) * width + x + w]
                        .copy_from_slice(&pixels[row * w..(row + 1) * w]);
                }
                assert_eq!(actual, expected);
            } else {
                assert_eq!(decode(&bytes[start..end]), (width, height, expected));
            }
            assert_eq!(renderer.legend_drawn, step < 2);
        }
    }
}

#[test]
fn cropped_rasters_keep_source_stride_and_partial_bands() {
    let mut image = Image::alloc(31, 19).unwrap();
    let colours = [[255, 0, 0, 255], [0, 102, 153, 255], [255, 255, 255, 255]];
    for y in 0..19 {
        for x in 0..31 {
            image.pixels[(y * 31 + x) * 4..(y * 31 + x + 1) * 4]
                .copy_from_slice(&colours[(x + y) % colours.len()]);
        }
    }
    let mut encoder = Sixel::default();
    for (left, top, width, height) in
        [(0, 0, 31, 19), (9, 5, 13, 7), (30, 18, 1, 1), (1, 1, 30, 18)]
    {
        let mut expected = Vec::new();
        for y in top..top + height {
            for x in left..left + width {
                let pixel = colours[(x + y) % colours.len()];
                expected.push([pixel[0], pixel[1], pixel[2]]);
            }
        }
        let bytes = encoder.encode_region(&image, left, top, width, height).unwrap();
        assert_eq!(decode(bytes), (width, height, expected));
    }
    for (left, top, width, height) in [
        (0, 0, 0, 1),
        (0, 0, 1, 0),
        (31, 0, 1, 1),
        (0, 19, 1, 1),
        (30, 0, 2, 1),
        (0, 18, 1, 2),
        (usize::MAX, 0, 1, 1),
    ] {
        assert_eq!(
            encoder.encode_region(&image, left, top, width, height),
            Err(KittyError::Argument)
        );
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
