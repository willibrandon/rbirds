# Reading text pixels by row

A fresh three-second application profile on `e4a8de4` still identified
`read_patch` as the largest active leaf in default text playback. The reader
computed an offset and checked a four-byte slice for every pixel. It now
checks each clipped row once and iterates its RGBA chunks. The clipped area
also supplies the pixel count directly. Pixel order, floating-point sums,
dominant-color ties and the eight-color fallback are unchanged.

This affects Braille, sextant and half-block color selection. It does not
change terminal glyphs, escape sequences or image transport. The profile was
intrusive diagnostic evidence, separate from the following measurements.

## Construction and live CPU

Two forward/reverse pairs per renderer compared 1,000-frame construction runs,
seed 42, with one warm-up per build. Frame times in milliseconds were:

| Renderer | Ordinary sprite, before → after | Shaded sprite, before → after |
|---|---:|---:|
| Braille | 1.027 → 1.001 | 0.968 → 0.918 |
| Sextants | 1.022 → 0.996 | 0.954 → 0.912 |
| Blocks | 0.922 → 0.880 | 0.920 → 0.850 |

These are medians of two runs per build, excluding terminal work. Reductions
ranged from 2.5% to 7.6%; output bytes per frame matched within each case.
The shaded sprite is the retained 32-by-32 gradient fixture from the earlier
cell-color investigation.

Four live default runs used before/after/after/before order on an M4 Pro,
macOS 26.5.2 and iTerm2 3.6.6. Each had a visible 100-by-32-cell,
1400-by-1088-pixel window, seed 42 and 3,600 loop ticks. The harness sampled
15-second windows around seconds 3–18 and 43–58. No capture, profiler,
build or tests ran during these CPU measurements. All eight windows were valid.

| CPU, ms per elapsed second | Early before | Early after | Later before | Later after |
|---|---:|---:|---:|---:|
| rbirds | 226.91 | 209.98 | 234.37 | 219.35 |
| iTerm main process | 585.18 | 578.83 | 450.35 | 452.56 |
| Combined | 812.09 | 788.81 | 684.72 | 671.91 |

Application CPU fell in both pairs and both windows. Median reductions were
7.5% early and 6.4% later; combined reductions were 2.9% and 1.9%. The stable
decoder helper used zero measured CPU. These are small same-host gains, not
cross-platform guarantees. Existing idle sessions shared the iTerm process,
and compositor/GPU work remains outside the counters.

## Appearance and verification

A separate candidate run captured 1,325 early and 1,340 later samples.
Visible changes were 56.35 and 59.86 per second, p99 gaps were 45.52 and
28.78 ms, and maximum gaps were 75.35 and 45.65 ms. Both captures had zero
nearly blank or bright-background samples under the retained thresholds.
The screenshot was inspected. The application trace had a 16.99 ms p99 interval,
one interval above 25 ms and none above 50 ms. Capture CPU was 1.01 and
0.92 seconds. The finite run exited successfully and its owned window closed.

This does not establish a cadence improvement: startup still reflects iTerm's
[glyph warm-up](live-text-warmup-2026-09-27-macos-arm64.md), and occasional
application wake delays remain. ScreenCaptureKit observes content changes at
logical window resolution, not physical scanout or individual displayed frame
IDs. Color and blank-frame diagnostics use a sampled grid.

All six independent C cell-oracle tests pass in native debug and release
builds and in an Intel macOS release build under Rosetta. They compare cell
state, glyphs, emitted bytes and painted pixels, including clipping, unusual
cell sizes and color-table overflow. The release allocation test still finds
zero steady-state allocations in all three text renderers. Formatting and
Clippy across all targets and features pass. Native Windows/Linux performance
was not measured; broader native CI remains a separate check of the commit.

The [evidence archive](live-text-rows-2026-09-27-macos-arm64.tar.gz) retains
source changes, executable hashes, benchmark samples, CPU windows, raw captures,
the application profile, trace, owned-window screenshot and validation logs.
No installed executable or terminal preference was replaced.
