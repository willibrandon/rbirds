# Deviations

Status: every entry is **proposed**. None is accepted until the owner records the
decision here; until then the affected requirement stays incomplete
(docs/COMPATIBILITY.md §6).

Each entry concerns C behavior that is undefined, and so cannot be reproduced
without reproducing memory corruption or an implementation-defined accident. Where the
reference's behavior is well defined, rbirds reproduces it, including its quirks
(for example the benchmark's double hunt, the `c` byte that ends a theme query,
and the byte-padded panel columns).

Reference for every entry: cbirds 1.4.0, commit
`cc446fc3cb80733371c62676533adcac2fc10002`, all targets.

## D-001: a flock grown and quit within one blocked write

- **Input:** a live run whose output is blocked (for example, a stalled
  terminal). During the wait, `+` (or `=`) arrives, and `q` arrives before the
  next frame. `--snapshot` is also given.
- **Reference behavior:** `handle_input` raises `config.birds` at once, and the
  arrays are only resized at the top of the next frame. `q` in the same wait ends
  the loop before that happens, so `write_snapshot` → `compose_onto` reads
  `config.birds` elements of arrays that hold fewer. This is an out-of-bounds read
  (undefined behavior).
- **rbirds behavior:** composition and placement draw
  `min(config.birds, allocated)` birds (`render::compose::drawn`). So the snapshot
  shows the birds that exist.
- **First divergence:** the first `birds[i]` read past the allocation.
- **Reason:** never reproduce memory corruption (docs/COMPATIBILITY.md §6).
- **Affected requirements:** C05, C15, C16.
- **Regression test:** pending, in the PTY suite (blocked output, `+q`, snapshot).
- **Claim affected:** none for defined inputs.

## D-002: integer overflow in cursor and paint arithmetic

- **Input:** cell grids or cell sizes near `INT_MAX` (for example, `cells_paint`
  with `cols * cell_width` over `INT_MAX`). These cannot come from the
  application, whose grids are at most 400×120 cells of at most 16 KiB-pixel
  images. They can only come from the translated module's own API.
- **Reference behavior:** signed overflow (undefined), usually wrapping.
- **rbirds behavior:** cursor positions use explicit wrapping arithmetic, and the
  oracle tests confirm this matches the C build's result. A painting too large to
  size is `CellsError::Memory`. A text buffer whose doubling would wrap is
  `Memory` rather than an endless loop.
- **Reason:** safe, checked arithmetic for sizes (docs/DESIGN.md §5).
- **Affected requirements:** C11.
- **Regression tests:** `tests/cells_oracle.rs`, `tests/c_cells.rs`.
- **Claim affected:** none reachable from the CLI.

## D-003: option help lines past the C buffer

- **Input:** a table row whose long name exceeds 88 bytes and has a metavar. The
  cbirds table has none; only a synthetic test table reaches this.
- **Reference behavior:** `options_usage` overruns its line buffer (undefined).
- **rbirds behavior:** the line is cut at the buffer's 95 bytes.
- **Affected requirements:** C01 (the generic parser only).
- **Regression test:** `tests/options_oracle.rs` (synthetic table, cases that
  don't overrun are compared byte for byte).
- **Claim affected:** none for the cbirds option table.
