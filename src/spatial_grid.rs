//! A uniform grid over the viewport for the neighbour search, translated from
//! cbirds `spatial_grid.c`.
//!
//! Items are bucketed by cell with a counting sort, so each cell's items are
//! one contiguous range of `indices`, in item order. The flocking sums run in
//! exactly that order, which is why the order is part of the contract.

#![forbid(unsafe_code)]

use std::fmt;

/// `spatial_grid_status_t` without `SPATIAL_GRID_OK`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridError {
    Argument,
    Memory,
}

impl GridError {
    /// `spatial_grid_status_string` for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            GridError::Argument => "invalid argument",
            GridError::Memory => "out of memory",
        }
    }
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `spatial_grid_status_string` over a whole status, `SPATIAL_GRID_OK`
/// included.
pub fn status_string(status: Result<(), GridError>) -> &'static str {
    match status {
        Ok(()) => "ok",
        Err(error) => error.as_str(),
    }
}

/// `spatial_grid_t`. `counts` and `offsets` hold `cell_count` and
/// `cell_count + 1` entries and `indices` holds `item_capacity`, as the C
/// allocations do; they are empty until the first successful prepare.
#[derive(Clone, Debug, Default)]
pub struct SpatialGrid {
    pub cell_size: i32,
    pub columns: i32,
    pub rows: i32,
    pub cell_count: i32,
    pub item_capacity: i32,
    pub counts: Vec<i32>,
    pub offsets: Vec<i32>,
    pub indices: Vec<i32>,
}

fn zeroed(length: usize) -> Result<Vec<i32>, GridError> {
    let mut storage = Vec::new();
    storage.try_reserve_exact(length).map_err(|_| GridError::Memory)?;
    storage.resize(length, 0);
    Ok(storage)
}

fn coordinate_to_cell(position: f64, cells: i32, cell_size: i32) -> i32 {
    if position.is_nan() || position <= 0.0 {
        return 0;
    }
    if !position.is_finite() || position >= f64::from(cells) * f64::from(cell_size) {
        return cells - 1;
    }
    (position / f64::from(cell_size)) as i32
}

impl SpatialGrid {
    /// `spatial_grid_init`.
    pub fn new(cell_size: i32) -> Result<SpatialGrid, GridError> {
        if cell_size <= 0 {
            return Err(GridError::Argument);
        }
        Ok(SpatialGrid { cell_size, ..SpatialGrid::default() })
    }

    /// `spatial_grid_prepare`: allocates only when the dimensions change or
    /// the capacity grows, and on failure leaves the grid as it was.
    pub fn prepare(
        &mut self,
        width: i32,
        height: i32,
        item_capacity: i32,
    ) -> Result<(), GridError> {
        if self.cell_size <= 0 || width <= 0 || height <= 0 || item_capacity <= 0 {
            return Err(GridError::Argument);
        }
        let columns = width / self.cell_size + i32::from(width % self.cell_size != 0);
        let rows = height / self.cell_size + i32::from(height % self.cell_size != 0);
        let cell_count = columns as usize * rows as usize;
        if cell_count == 0
            || cell_count > i32::MAX as usize
            || cell_count > usize::MAX / std::mem::size_of::<i32>() - 1
        {
            return Err(GridError::Memory);
        }
        if self.columns == columns && self.rows == rows && self.item_capacity >= item_capacity {
            return Ok(());
        }

        let capacity = self.item_capacity.max(item_capacity);
        let counts = zeroed(cell_count)?;
        let offsets = zeroed(cell_count + 1)?;
        let indices = zeroed(capacity as usize)?;
        self.columns = columns;
        self.rows = rows;
        self.cell_count = cell_count as i32;
        self.item_capacity = capacity;
        self.counts = counts;
        self.offsets = offsets;
        self.indices = indices;
        Ok(())
    }

    /// `spatial_grid_cell_for_position`: positions outside the grid map to the
    /// nearest border cell, and a grid never prepared answers cell (0, 0).
    pub fn cell_for_position(&self, x: f64, y: f64) -> (i32, i32) {
        if self.columns <= 0 || self.rows <= 0 {
            return (0, 0);
        }
        (
            coordinate_to_cell(x, self.columns, self.cell_size),
            coordinate_to_cell(y, self.rows, self.cell_size),
        )
    }

    fn cell_for_item(&self, position: &impl Fn(usize) -> (f64, f64), index: usize) -> usize {
        let (x, y) = position(index);
        let (cell_x, cell_y) = self.cell_for_position(x, y);
        (cell_y * self.columns + cell_x) as usize
    }

    /// `spatial_grid_build`: rebuilds the cell ranges without allocating.
    /// `position` is asked for each item twice, as the C reader is.
    pub fn build(
        &mut self,
        item_count: i32,
        position: impl Fn(usize) -> (f64, f64),
    ) -> Result<(), GridError> {
        if item_count < 0 || item_count > self.item_capacity || self.cell_count <= 0 {
            return Err(GridError::Argument);
        }
        let cells = self.cell_count as usize;
        let items = item_count as usize;

        self.counts[..cells].fill(0);
        for i in 0..items {
            let cell = self.cell_for_item(&position, i);
            self.counts[cell] += 1;
        }
        self.offsets[0] = 0;
        for cell in 0..cells {
            self.offsets[cell + 1] = self.offsets[cell] + self.counts[cell];
        }
        self.counts[..cells].fill(0);
        for i in 0..items {
            let cell = self.cell_for_item(&position, i);
            let slot = (self.offsets[cell] + self.counts[cell]) as usize;
            self.counts[cell] += 1;
            self.indices[slot] = i as i32;
        }
        Ok(())
    }

    /// The items of one cell, in item order.
    #[inline]
    pub fn cell_items(&self, cell: usize) -> &[i32] {
        &self.indices[self.cells_slots(cell, cell)]
    }

    /// Where the items of cells `first` to `last` inclusive lie in
    /// `indices`: the cells one after another, each in item order, as
    /// walking them one at a time would give them.
    #[inline]
    pub fn cells_slots(&self, first: usize, last: usize) -> std::ops::Range<usize> {
        self.offsets[first] as usize..self.offsets[last + 1] as usize
    }

    /// The items of the last build, cell by cell: `indices` as far as it
    /// was filled, empty before the first prepare.
    #[inline]
    pub fn items(&self) -> &[i32] {
        match self.offsets.get(self.cell_count.max(0) as usize) {
            Some(&end) => &self.indices[..end as usize],
            None => &[],
        }
    }
}
