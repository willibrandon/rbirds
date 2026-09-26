//! Translation of cbirds `tests/spatial_grid_test.c`, one `#[test]` per C test
//! under the C test's own name.

use rbirds::spatial_grid::{GridError, SpatialGrid};

const CELL_SIZE: i32 = 12;

fn range_contains(grid: &SpatialGrid, cell: usize, value: i32) -> bool {
    grid.cell_items(cell).contains(&value)
}

#[test]
fn test_layout_and_clamping() {
    let points: [(f64, f64); 6] =
        [(0.0, 0.0), (11.9, 11.9), (12.0, 0.0), (24.9, 12.0), (-10.0, 5.0), (99.0, 99.0)];
    let mut grid = SpatialGrid::new(CELL_SIZE).expect("init");
    grid.prepare(25, 13, 6).expect("prepare");
    assert_eq!(grid.columns, 3);
    assert_eq!(grid.rows, 2);
    assert_eq!(grid.cell_count, 6);
    // Preparing again for the same size reuses the storage rather than
    // reallocating it.
    let storage = (grid.counts.as_ptr(), grid.offsets.as_ptr(), grid.indices.as_ptr());
    grid.prepare(25, 13, 6).expect("prepare again");
    assert_eq!(storage, (grid.counts.as_ptr(), grid.offsets.as_ptr(), grid.indices.as_ptr()));
    grid.build(6, |i| points[i]).expect("build");

    assert_eq!(grid.offsets[grid.cell_count as usize], 6);
    assert_eq!(grid.counts[0], 3);
    assert!(range_contains(&grid, 0, 0));
    assert!(range_contains(&grid, 0, 1));
    assert!(range_contains(&grid, 0, 4));
    assert!(range_contains(&grid, 1, 2));
    assert!(range_contains(&grid, 5, 3));
    assert!(range_contains(&grid, 5, 5));

    assert_eq!(grid.cell_for_position(f64::NEG_INFINITY, f64::NAN), (0, 0));
    assert_eq!(grid.cell_for_position(f64::INFINITY, f64::INFINITY), (2, 1));

    grid.prepare(12, 12, 6).expect("prepare smaller");
    assert!(grid.columns == 1 && grid.rows == 1);
    grid.build(6, |i| points[i]).expect("build smaller");
    assert_eq!(grid.counts[0], 6);
}

fn next_random(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state
}

fn random_coordinate(state: &mut u32, span: i32, offset: i32) -> f64 {
    f64::from(next_random(state) % span as u32) / 10.0 - f64::from(offset)
}

fn assert_neighbors_match(
    grid: &SpatialGrid,
    points: &[(f64, f64)],
    target_index: usize,
    vision_cells: i32,
) {
    let count = points.len();
    let mut expected = vec![false; count];
    let mut actual = vec![false; count];
    let radius = f64::from(vision_cells * CELL_SIZE);
    let radius_squared = radius * radius;
    let target = points[target_index];
    for (i, point) in points.iter().enumerate() {
        if i == target_index {
            continue;
        }
        let dx = target.0 - point.0;
        let dy = target.1 - point.1;
        if dx * dx + dy * dy < radius_squared {
            expected[i] = true;
        }
    }

    let (center_x, center_y) = grid.cell_for_position(target.0, target.1);
    let min_x = (center_x - vision_cells).max(0);
    let max_x = (center_x + vision_cells).min(grid.columns - 1);
    let min_y = (center_y - vision_cells).max(0);
    let max_y = (center_y + vision_cells).min(grid.rows - 1);
    for cell_y in min_y..=max_y {
        for cell_x in min_x..=max_x {
            let cell = (cell_y * grid.columns + cell_x) as usize;
            for &i in grid.cell_items(cell) {
                let i = i as usize;
                if i == target_index {
                    continue;
                }
                let dx = target.0 - points[i].0;
                let dy = target.1 - points[i].1;
                if dx * dx + dy * dy >= radius_squared {
                    continue;
                }
                assert!(!actual[i], "point {i} visited twice");
                actual[i] = true;
            }
        }
    }
    assert_eq!(expected, actual, "target {target_index}, {vision_cells} cells");
}

#[test]
fn test_against_brute_force() {
    const POINT_COUNT: usize = 512;
    let mut points = [(0.0, 0.0); POINT_COUNT];
    let mut random_state: u32 = 0x6c8e9cf5;
    for point in points.iter_mut() {
        point.0 = random_coordinate(&mut random_state, 9000, 130);
        point.1 = random_coordinate(&mut random_state, 6000, 100);
    }
    // Exact cell boundaries and points immediately around them.
    points[0] = (12.0, 12.0);
    points[1] = (24.0, 12.0);
    points[2] = (36.0, 36.0);
    points[3] = (-1.0, 20.0);
    points[4] = (641.0, 20.0);

    let mut grid = SpatialGrid::new(CELL_SIZE).expect("init");
    grid.prepare(640, 384, POINT_COUNT as i32).expect("prepare");
    grid.build(POINT_COUNT as i32, |i| points[i]).expect("build");
    for vision_cells in 1..=12 {
        for target in 0..POINT_COUNT {
            assert_neighbors_match(&grid, &points, target, vision_cells);
        }
    }
}

#[test]
fn test_invalid_arguments() {
    // spatial_grid_init(NULL, ...) has no Rust form: a grid is a value. The
    // refusals below keep the guarantee it protects, that nothing is built
    // from an invalid request.
    assert_eq!(SpatialGrid::new(0).err(), Some(GridError::Argument));
    let mut grid = SpatialGrid::new(CELL_SIZE).expect("init");
    assert_eq!(grid.prepare(0, 10, 1), Err(GridError::Argument));
    // Building a grid never prepared is refused, however few items.
    assert_eq!(grid.build(0, |_| (0.0, 0.0)), Err(GridError::Argument));
}
