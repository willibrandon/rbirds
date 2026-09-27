# Sixel placement through actual iTerm startup

These checks use an M4 Pro, macOS 26.5.2 and iTerm2 3.6.6. They correct a
qualification gap in the earlier cropped-Sixel measurements. The original
yellow-placeholder failure and synchronized-erase fix are described in the
[earlier investigation](live-2026-09-26-macos-arm64.md). This correction keeps
that fix and repairs a separate image-placement defect introduced by cropping.

`prepare_sixel` enables DECSDM (`CSI ?80h`). In that mode, iTerm's
[image insertion code](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/VT100ScreenMutableState%2BTerminalDelegate.m)
places Sixel images at the screen origin, ignoring the cursor position used
for a crop. The previous static launcher explicitly disabled DECSDM, while
the independent frame decoder ignored mode changes. Both therefore missed
the production startup interaction.

A four-image tiling experiment exposed the problem: every tile landed at
the top-left corner, hiding other tiles. Its decoder reported identical pixels
and its capture reported no bright or blank backgrounds, but its screenshot
was visibly wrong. Its timing results are invalid for performance qualification;
no CPU comparison of that prototype was run and tiling was not adopted.

The renderer now resets DECSDM around each positioned image and restores it
immediately afterward, inside the synchronized update. Cropping requires a
changeable mode response, whole-cell geometry and a confirmed single-width
backdrop. Unknown or permanent mode responses retain full rasters. On exit,
cleanup cancels incomplete control sequences before restoring either original
mode state. CAN followed by ESC CAN also handles iTerm's
[Sixel parser](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/VT100SixelParser.m),
which accumulates a lone CAN instead of aborting the image.

## Correctness checks

The updated viewport decoder starts in the mode established by production
startup. It reproduced the misplaced pixels before the change, then passed
afterward through clipping, trails, hawks, off-screen sprites, empty scenes
and resize. Regression diagnostics now report the first differing pixel
instead of dumping both complete rasters.

A visible probe used the library's real `Terminal`, `prepare_sixel`, width
negotiation and renderer. Six pairs compared a full raster with the crop:
centered birds, bottom/right clipping, top/left clipping, empty sky, a panel
after resizing from 100×32 to 80×24 cells, and the same resized scene after
hiding the panel. All six complete content rectangles had zero differing RGB
pixels. Restoring the old frame mode behavior reproduced 57,421 differing
pixels in the centered scene. The full-resolution screenshots and frame bytes
are retained, and the centered and clipped candidate screenshots were inspected.

The initial probe completed all screenshot pairs but its subsequent cleanup
loop incorrectly reused one-shot terminal state and timed out on a mode query.
That harness failure is retained. The corrected cleanup probe resets the
test-only global state between acquisitions and allows 100 ms for each query.
It interrupted a representative frame at every byte boundary with both original
mode states: all 142 real-iTerm queries confirmed restoration. PTY tests also
interrupt the actual application while a Sixel image is incomplete and check
raw attributes, alternate-screen cleanup and both original mode states.

All nine raster tests and seven Sixel PTY tests passed in native debug and
release and Rosetta release builds. The terminal restoration state test,
formatting, whitespace checks and Clippy with all targets/features passed.
The Windows negotiation expectation includes the new positioning capability;
native Windows execution remains a CI check.

## Fresh visible observations

Separate foreground captures used the actual executable, seed 42 and a
100×32-cell, 1400×1088-pixel window. Capture started about three seconds after
application launch. The dense scene used 4,096 birds, speed 12, four hawks,
three flocks, depth, trails and the ember palette.

| Scene | Capture seconds | Samples | Visible changes/s | p99 gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|---:|
| Default Sixel | 30 | 2,836 | 59.85 | 28.12 | 34.75 |
| Dense Sixel | 15 | 1,478 | 45.33 | 35.07 | 43.94 |

Every sample had the dark `[18,18,23]` dominant background. The largest
dominant-color fractions were 93.28% and 91.74%; no mostly empty or whole-window
yellow frame appeared. Both screenshots were inspected. Capture used 1.93 and
1.12 CPU seconds respectively. These are finite observations of displayed
changes, not proof that stalls can never occur. Dense playback still falls
short of 60 visible changes per second.

The previous crop CPU savings and cadence claims are superseded because those
runs displayed misplaced images. They remain in their original archives as
historical observations. The full-raster flicker fix, Kitty measurements and
text measurements are separate and unaffected.

Two additional runs measured CPU without capture, profiling, builds or tests.
Each interval lasted fifteen seconds after three seconds of warm-up; both
counter samples were valid. These are absolute post-correction observations,
with one sample per scene, and establish no comparative saving.

| Scene | Application CPU ms/s | iTerm CPU ms/s | Decoder CPU ms/s | Combined CPU ms/s |
|---|---:|---:|---:|---:|
| Default Sixel | 400.46 | 785.70 | 130.22 | 1,316.38 |
| Dense Sixel | 849.39 | 1,208.97 | 378.34 | 2,436.69 |

iTerm's main process was shared with existing idle sessions. The selected
decoder helper remained alive; compositor, GPU and unselected processes are
excluded. Native Windows and Linux desktop presentation was not measured.

The measured executable SHA-256 is
`7c0f772ddf9f2a5266b3f5aff8fa87e044f1dcab140563b47c4eee97acfea328`,
built from `4e67b58929625a7cc2203c6cdbf4ed2f6da6bda3` plus the retained source
patch. The [evidence archive](live-sixel-position-2026-09-27-macos-arm64.tar.gz)
contains the pixel comparisons, screenshots, frame streams, captures, traces,
CPU observations, probe sources, cleanup results and validation record. Its
manifest checks every included file. Large rejected tiling replay corpora are
excluded; their generators, hashes, failed qualification and screenshots are
retained instead.

Archive SHA-256:
`a805f5e23bad5a8decaa02e61d379cf7402f003146989753129febd49e46503f`
(6,193,448 bytes; 104 files verified against the manifest).
