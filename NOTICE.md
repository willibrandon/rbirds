# Attribution

rbirds is a Rust translation of [cbirds](https://github.com/clainstone/cbirds)
1.4.0, commit `cc446fc3cb80733371c62676533adcac2fc10002`.

- Each module under `src/` is translated from the cbirds C source named in its
  header comment: `boids.c`, `cells.c`, `font.c`, `gif.c`, `kitty_graphics.c`,
  `options.c`, `png.c` and `spatial_grid.c`. The algorithms, constants, tables,
  help text and the comments that explain them are the upstream author's work.
- `assets/sprite.png` is the upstream bird artwork. It is the same file as
  `matrix.png` and has the same bytes as the array in `sprite_png.h`
  (SHA-256 `cbd1cb4985cb59e6faa19556254c0404065124641ed0a86041144c64ee37165e`).
- The 5×7 bitmap font in `src/font.rs` is the upstream font, unchanged.

cbirds is copyright (c) 2025 clainstone@icloud.com and distributed under the MIT
License. The license text, with the rbirds copyright line (Brandon Williams)
added, is in [`LICENSE`](LICENSE).
