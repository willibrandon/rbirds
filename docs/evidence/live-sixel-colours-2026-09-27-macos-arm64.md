# Sixel palette selection and iTerm flash confirmation

The encoder now explicitly selects each newly defined palette entry before
drawing. WezTerm `20240203-110809-5046fc22` separates color definition from
selection in its [Sixel decoder](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/term/src/terminalstate/sixel.rs#L35).
Previously, the first background band used its initial green foreground, and
new bird colors used the preceding foreground until selected in a later band.
The extra selection leaves the intended raster unchanged in decoders where
defining a color also selects it.

An owned WezTerm process on macOS 26.5.2/M4 Pro used an isolated configuration,
Monaco 12, and 100×32 cells at 1400×1056 pixels, resized to 80×24 during the
probe. Captures before and after the change include centered birds, clipped
edges, off-screen birds, trails, hawks, panel changes and resizing. Six full
rasters were compared with an independent decoder using this terminal's
integer RGB conversion. Across 8,166,480 compared pixels, the old output had
34,088 mismatches against the intended raster; the corrected output had zero.
Both captures also matched the model of WezTerm's separate definition and
selection exactly. Window borders and rounded corners were excluded. The
panel case was inspected visually and excluded from this raster comparison.

The diagnostic also tried the existing iTerm crop path in WezTerm. At this
stage it remained disabled: its glyph background differed by one RGB level
from the Sixel background. A follow-up pixel comparison corrected the initial
visual assessment of the panel: all pixels in its 532×363 region are identical
between `static-fixed/8.png` and `static-fixed/9.png` in the archive. The
color-selection change does not fix the background conversion difference.
The [subsequent placement checks](live-wezterm-placement-2026-09-27-macos-arm64.md)
address that difference and qualify the cropped path separately.
The diagnostic forces an erase before both full and cropped frames; it does
not qualify ordinary playback speed or CPU usage. The initial capture attempt
used the wrong window-owner name, timed out, and is retained as a failed
harness attempt.

The [iTerm whole-window flash fix](live-2026-09-26-macos-arm64.md) was also
rechecked in installed iTerm 3.6.6, before and after this encoder change.
Each foreground run used the normal elapsed-time executable with
`--render sixel --seed 42 --frames 2400`, 100×32 cells, and a separate
30-second window capture after startup. No build, tests or profiling ran
during capture. These runs are presentation checks, not CPU comparisons.

| Build | Samples | Yellow or blank backgrounds | Visible changes/s | p99 gap | Maximum gap |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before palette selection | 2,819 | 0 | 59.951 | 27.56 ms | 33.72 ms |
| After palette selection | 2,795 | 0 | 59.986 | 27.19 ms | 32.59 ms |

Every sample's dominant color was the intended dark background. All 555
focus observations found the test window active and foremost over its content.
Both commands exited successfully and their owned windows were closed. These
are finite window-surface observations, not a guarantee about physical scanout
or every future scheduling condition. The installed `rbirds` was not replaced.

The new regression test failed with the old encoder and passes after the fix.
All 29 Sixel, Sixel PTY, allocation-failure and steady-state allocation checks
pass in native debug, native release and Rosetta release builds. Formatting
and Clippy with warnings denied also pass.

The [archive](live-sixel-colours-2026-09-27-macos-arm64.tar.gz) contains 163
hashed files: raw protocol output, unedited captures, probes, analysis scripts,
source snapshots and check logs. Its SHA-256 is
`6899851dc6e9fab38dff3c61cc2a127a13174c75ccbf30513a9a6b4114fac402`.
After extraction, `compare.py` reproduces the raster comparison with Pillow
and NumPy; `analyze-iterm.py` reproduces the presentation summary using only
Python's standard library. Binary identities are recorded in `environment.json`.
