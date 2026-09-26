//! Recoverable allocation failure at the reference's recovery points
//! (docs/DESIGN.md §7, docs/PORTING.md §4.C).
//!
//! Where the C checks a `malloc`/`calloc`/`realloc` result and recovers or
//! reports, the port reserves fallibly and does the same. This test binary
//! installs an allocator that refuses, on the thread that arms it, every
//! allocation at or above a size, and drives each recovery point into it:
//! a flock that cannot grow keeps the birds it has (and the live loop puts
//! the count back), and images, grids, cell grids, output buffers and the GIF
//! writer report out of memory instead of aborting the process.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rbirds::image::{Image, PngError};
use rbirds::simulation::{Bird, Sim};
use rbirds::spatial_grid::{GridError, SpatialGrid};

struct Refusing;

thread_local! {
    /// Allocations of at least this many bytes fail on this thread.
    static REFUSE_FROM: Cell<usize> = const { Cell::new(usize::MAX) };
}

fn refused(size: usize) -> bool {
    REFUSE_FROM.try_with(|limit| size >= limit.get()).unwrap_or(false)
}

// SAFETY: every call is forwarded to the system allocator unchanged, except
// that some requests are refused by returning null, which GlobalAlloc
// permits for any allocation.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refused(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the layout is the caller's, passed on as is.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refused(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: as for alloc.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, which is System's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if refused(new_size) {
            return std::ptr::null_mut();
        }
        // SAFETY: as for dealloc and alloc.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Runs `body` with every allocation of `bytes` or more refused.
fn refusing<T>(bytes: usize, body: impl FnOnce() -> T) -> T {
    REFUSE_FROM.with(|limit| limit.set(bytes));
    let result = body();
    REFUSE_FROM.with(|limit| limit.set(usize::MAX));
    result
}

fn flock(sim: &mut Sim, count: i32) -> (Vec<Bird>, Vec<Bird>) {
    sim.apply_screen_size(100, 30, 800, 480);
    sim.config.birds = count;
    sim.rng.seed(3);
    let mut birds = vec![Bird::default(); count as usize];
    sim.initialize_birds(&mut birds);
    (birds.clone(), birds)
}

/// resize_the_flock: "Both arrays change or neither does."
#[test]
fn a_flock_that_cannot_grow_keeps_what_it_has() {
    let mut sim = Sim::new();
    let (mut birds, mut snapshot) = flock(&mut sim, 100);
    let before = birds.clone();
    let rng = sim.rng.clone();
    let grown = std::mem::size_of::<Bird>() * 4096;
    let resized = refusing(grown, || sim.resize_the_flock(&mut birds, &mut snapshot, 100, 4096));
    assert!(!resized);
    assert_eq!(birds, before);
    assert_eq!(snapshot.len(), 100);
    // No bird was placed, so no random number was drawn.
    assert_eq!(sim.rng, rng);
}

#[test]
fn sixel_plane_allocation_failure_preserves_output_and_can_recover() {
    use rbirds::render::kitty::{KittyError, KittyGraphics};
    use rbirds::render::sixel::Sixel;
    let image = Image::alloc(4096, 1).unwrap();
    let mut encoder = Sixel::default();
    let mut output = KittyGraphics::new(1).unwrap();
    output.write_raw(b"previous").unwrap();
    let result = refusing(4096, || encoder.queue(&mut output, &image));
    assert_eq!(result, Err(KittyError::Memory));
    assert_eq!(output.buffer(), b"previous");
    encoder.queue(&mut output, &image).unwrap();
    assert!(output.buffer().starts_with(b"previous\x1bP0;1q"));
    assert!(output.buffer().ends_with(b"\x1b\\"));
}

/// The live loop's side of it: `config.birds = live_birds`.
#[test]
fn the_live_loop_puts_the_count_back_when_growing_fails() {
    use rbirds::live::{Frame, LiveLoop};
    use rbirds::platform::{Timespec, WinSize};
    let mut sim = Sim::new();
    sim.config.bird_size = 30;
    let (mut birds, mut snapshot) = flock(&mut sim, 100);
    let mut renderer = rbirds::render::Renderer::default();
    let mut graphics = rbirds::render::kitty::KittyGraphics::new(1).expect("graphics");
    sim.render_mode = rbirds::simulation::RenderMode::Kitty;
    let mut grid = SpatialGrid::new(12).expect("grid");
    grid.prepare(800, 480, 100).expect("prepare");
    let mut live = LiveLoop::new(Timespec { tv_sec: 1, tv_nsec: 0 }, 100);
    let window = WinSize { row: 30, col: 100, xpixel: 800, ypixel: 480 };
    // '+' grows the flock by a quarter; refuse the new arrays, and nothing
    // smaller, so the frame itself proceeds.
    let grown = std::mem::size_of::<Bird>() * 126;
    let frame = refusing(grown, || {
        live.frame(
            &mut sim,
            &mut renderer,
            &mut graphics,
            &mut birds,
            &mut snapshot,
            &mut grid,
            Some(b"+"),
            Timespec { tv_sec: 1, tv_nsec: 16_666_667 },
            window,
        )
    });
    assert_eq!(frame, Ok(Frame::Drawn));
    assert_eq!(sim.config.birds, 100);
    assert_eq!(live.live_birds, 100);
    assert_eq!(birds.len(), 100);
}

#[test]
fn an_image_that_cannot_be_allocated_is_out_of_memory() {
    let result = refusing(1 << 20, || Image::alloc(1024, 1024));
    assert_eq!(result, Err(PngError::Memory));
    // And the codec reports the same, not an abort.
    let source = Image::alloc(600, 600).expect("alloc");
    let resized = refusing(1 << 20, || rbirds::image::png::resize(&source, 1000, 1000));
    assert_eq!(resized, Err(PngError::Memory));
    let encoded = refusing(1 << 16, || rbirds::image::png::encode(&source));
    assert_eq!(encoded, Err(PngError::Memory));
}

/// spatial_grid_prepare: refused, and the grid left as it was.
#[test]
fn a_grid_that_cannot_grow_is_left_as_it_was() {
    let mut grid = SpatialGrid::new(12).expect("grid");
    grid.prepare(120, 120, 10).expect("prepare");
    let before = (grid.columns, grid.rows, grid.item_capacity, grid.counts.len());
    let result = refusing(1 << 16, || grid.prepare(4000, 4000, 10));
    assert_eq!(result, Err(GridError::Memory));
    assert_eq!(before, (grid.columns, grid.rows, grid.item_capacity, grid.counts.len()));
}

#[test]
fn cells_and_output_buffers_report_out_of_memory() {
    let mut cells = rbirds::render::cells::Cells::new(true).expect("cells");
    let result = refusing(1 << 16, || cells.resize(400, 120));
    assert_eq!(result, Err(rbirds::render::cells::CellsError::Memory));
    let mut graphics = rbirds::render::kitty::KittyGraphics::new(1).expect("graphics");
    let text = vec![b'x'; 1 << 17];
    let result = refusing(1 << 16, || graphics.write_raw(&text));
    assert_eq!(result, Err(rbirds::render::kitty::KittyError::Memory));
    assert!(graphics.is_empty());
}

#[test]
fn a_gif_writer_that_cannot_allocate_reports_it() {
    let dir = std::env::temp_dir().join(format!("rbirds-alloc.{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("x.gif");
    let result =
        refusing(1 << 20, || rbirds::image::gif::GifWriter::open(&path, 2000, 2000, 4).err());
    assert_eq!(result, Some(rbirds::image::gif::GifError::Memory));
    let _ = std::fs::remove_dir_all(&dir);
}
