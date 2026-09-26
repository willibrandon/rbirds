//! No per-frame allocation in steady state (docs/PORTING.md §7): once the
//! first frames have sized the grid, the canvas, the cells and the output
//! buffer, a live frame (keys, clock, simulation, composition, the panel,
//! the queued bytes) allocates nothing, in every renderer.
//!
//! The C allocates in steady state only where the port does too, and those
//! paths are not live frames: the recording's painted image per GIF frame.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rbirds::live::{Frame, LiveLoop};
use rbirds::platform::{Timespec, WinSize};
use rbirds::render::Renderer;
use rbirds::render::kitty::KittyGraphics;
use rbirds::simulation::{Bird, RenderMode, Sim};
use rbirds::spatial_grid::SpatialGrid;

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn note() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

// SAFETY: every call is forwarded unchanged to the system allocator; the
// counter is thread-local and never allocates.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's layout, passed on.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from System through this allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn counted<T>(body: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let result = body();
    COUNTING.with(|on| on.set(false));
    (result, ALLOCATIONS.with(Cell::get))
}

/// Allocations made by frames 30 to 150 of a live session with the panel up,
/// hawks, trails, depth and two flocks.
fn steady_state_allocations(render: RenderMode) -> usize {
    let mut sim = Sim::new();
    sim.config.birds = 300;
    sim.config.hawks = 2;
    sim.config.flocks = 2;
    sim.config.trails = true;
    sim.deep_look = true;
    sim.legend_enabled = true;
    sim.config.palette = 1;
    sim.render_mode = render;
    sim.settle_the_bird_size();
    sim.apply_notches();
    let window = WinSize { row: 30, col: 100, xpixel: 800, ypixel: 480 };
    sim.apply_screen_size(100, 30, 800, 480);
    let mut renderer = Renderer::default();
    if sim.drawing_with_text() {
        renderer.prepare_text_renderer(&mut sim, None, b"rbirds").expect("sprites");
    }
    let mut grid = SpatialGrid::new(12).expect("grid");
    grid.prepare(800, 480, 300).expect("prepare");
    sim.rng.seed(1);
    sim.set_frame_seconds(1.0 / 60.0);
    sim.hawk_sets_built = true;
    let mut birds = vec![Bird::default(); 300];
    let mut snapshot = birds.clone();
    sim.initialize_birds(&mut birds);
    sim.place_hawks();
    let mut graphics = KittyGraphics::new(1).expect("graphics");
    let mut live = LiveLoop::new(Timespec { tv_sec: 10, tv_nsec: 0 }, 300);
    let mut total = 0;
    for frame in 1..=150 {
        let at = Timespec { tv_sec: 10, tv_nsec: frame * 16_666_667 };
        graphics.clear();
        let (drawn, allocations) = counted(|| {
            live.frame(
                &mut sim,
                &mut renderer,
                &mut graphics,
                &mut birds,
                &mut snapshot,
                &mut grid,
                None,
                at,
                window,
            )
        });
        assert_eq!(drawn, Ok(Frame::Drawn));
        if frame > 30 {
            total += allocations;
        }
    }
    total
}

#[test]
fn a_kitty_frame_allocates_nothing_in_steady_state() {
    assert_eq!(steady_state_allocations(RenderMode::Kitty), 0);
}

#[test]
fn a_text_frame_allocates_nothing_in_steady_state() {
    for render in [RenderMode::Braille, RenderMode::Sextants, RenderMode::Blocks] {
        assert_eq!(steady_state_allocations(render), 0, "{render:?}");
    }
}
