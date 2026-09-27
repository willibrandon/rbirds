# Live efficiency and playback

These measurements compare `2fa8167` with the work on `perf/live-efficiency`.
They cover an Apple M4 Pro running macOS 26.5.2, on AC power with the existing
power mode left unchanged (`powermode 2`). The built-in Retina display is
3024×1964. Physical scanout timing was not measured. No local builds or test
suites ran during the comparison sessions.

The objective is minimum CPU cost at a steady 60 Hz with unchanged pixels and
controls. CPU per frame and CPU per second answer different questions: fixing
slow playback can increase work per second. Terminal CPU must be included when
evaluating graphics changes. These results do not establish zero stalls in
every emulator or at every workload.

## Application and transport

The first series used `tools/live-perf.py`: 600 frames, seed 42, 160×50 cells,
8×16 pixels per cell, three alternating baseline/candidate pairs per renderer.
UFC video was playing during this series. The candidate was `c3c3b94`, before
Kitty texture grouping and sprite row bounds. All samples were retained.

| Renderer | CPU ms/s, before → after | CPU ms/frame, before → after | Median run p99 receive interval, ms |
| --- | ---: | ---: | ---: |
| default (Braille) | 251.8 → 215.8 | 5.252 → 3.671 | 23.43 → 18.37 |
| Kitty | 84.2 → 103.6 | 1.898 → 1.780 | 25.47 → 18.55 |
| Sixel | 315.1 → 237.1 | 6.467 → 4.031 | 22.48 → 18.00 |
| blocks | 223.4 → 211.7 | 4.734 → 3.599 | 23.74 → 17.74 |
| sextants | 251.8 → 212.8 | 5.268 → 3.620 | 23.36 → 18.08 |

The CPU figures are medians across runs and include process startup. A CPU
rate of 100 ms/s is one tenth of one logical processor. This harness drains a
PTY; it does not paint a terminal window. The faster cadence explains why
Kitty CPU per second increased despite lower application cost per frame.

## Real terminals

Kitty 0.44.0 was launched in dedicated processes with configuration disabled,
an 800×500 logical window, and a 114×40-cell / 1596×1000-pixel viewport.
`tools/terminal-perf.py` measured application and terminal CPU separately.
Its Mach counters use the machine's 125/3 timebase. Neither compositor nor GPU
cost is included.

An initial three-pair comparison with UFC playing used occluded Kitty windows.
Grouping sprite rotations into vertical textures reduced combined application
and terminal CPU from about 506 to 262 ms/s. Application CPU rose from about
77 to 142 ms/s while terminal CPU fell from about 428 to 120 ms/s. This is
useful evidence about terminal parsing and image management, but occlusion
skips rendering work; the 48% reduction must not be presented as a visible
playback result.

Foreground CPU comparisons with UFC off used two alternating pairs of 1200
frames, separate from screen capture. Medians were:

| Renderer | Application CPU ms/s, before → after | Terminal CPU ms/s | Combined CPU ms/s | Combined CPU ms/frame |
| --- | ---: | ---: | ---: | ---: |
| default | 272.2 → 236.6 | 49.1 → 109.5 | 321.3 → 346.0 | 6.181 → 5.834 |
| Kitty graphics | 48.7 → 103.4 | 319.4 → 253.9 | 368.1 → 357.3 | 7.423 → 6.047 |

The default application's CPU fell about 13%, but combined CPU rose about 8%
while delivering more frames. The Kitty combined reduction was only about 3%,
and its candidate terminal samples varied substantially (228–279 ms/s). These
results do not support a blanket claim of lower visible-session CPU. Restored
frame cadence and terminal rendering change the totals; the covered-window
result is not a substitute for these runs. Fixed frame counts also average
different elapsed durations and therefore different parts of the simulation.

The final visible-window samples used an optimized build of
`tools/presentation-capture.swift`, requesting 120 Hz for twelve seconds.
The candidate includes sprite row bounds (`8dd03da`). Each entry below is one
capture, with samples taken after startup. The default baseline capture was
near the time of macOS permission approvals; retain it as exploratory evidence
rather than a quiet-host latency bound.

| Kitty renderer | Changed frames/s, before → after | p99 interval, ms | Maximum interval, ms |
| --- | ---: | ---: | ---: |
| default | 52.48 → 59.97 | 28.10 → 20.94 | 30.58 → 32.79 |
| Kitty graphics | 50.73 → 59.73 | 28.67 → 26.99 | 33.36 → 28.38 |

These are changed ScreenCaptureKit samples, not physical scanout. Sampling can
coalesce frames, delay timestamps or miss small pixel changes. The capture
process itself used 0.46–0.54 CPU seconds per session; compositor overhead is
additional. CPU comparisons run without this observer. Earlier unoptimized
capture runs are retained separately and are not interchangeable with these.

WezTerm 20240203-110809-5046fc22 exposed a limit the PTY harness missed. At
100×32 cells / 1400×1024 pixels, visible Sixel playback remained around
21–22 changed samples/s. The candidate's median application update,
composition and encoding totaled about 2 ms, but its median flush took about
38 ms, with repeated long gaps. A separate process sample found substantial
time in SHA-256 hashing of full decoded images. The version's
[Sixel implementation](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/term/src/terminalstate/sixel.rs)
and [image cache](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/term/src/terminalstate/image.rs)
also show this work. Changing Sixel color-pass ordering preserved every pixel
but did not improve total CPU or cadence in two alternating pairs; that
experiment was discarded. Faster application encoding cannot remove this
terminal's full-raster processing cost.

iTerm2 3.6.6 exposed two distinct failures. It advertises Sixel but does not
answer `CSI 16 t`. Its native PTY dimensions were 1400×1088 for 100×32 cells;
its own `ReportCellSize` returned 7×17 logical pixels at scale 2. The new Unix
fallback uses those exact native dimensions when the cell query is unavailable.
Explicit cell-query replies retain priority, and Windows still requires them.

Once startup worked, the window intermittently flashed brown/yellow. The first
five-second capture contained 18 mostly-yellow samples out of 469, reaching
98.86% of sampled content pixels. This is iTerm's missing-image placeholder:
its [Metal image renderer](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/Metal/Renderers/iTermImageRenderer.m#L125)
uses brown when an image run has no image info. Its
[screen-state merger](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/VT100ScreenState.m#L294)
releases overwritten images, and its
[image marks](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/iTermImageMark.m#L58)
remove the registry entry when destroyed. The source and capture identify an
image-lifetime failure during replacement, rather than an encoded background
color change. The precise internal scheduling race was not instrumented.

Removing synchronized updates alone still produced 18 bad samples out of 440.
Erasing without synchronization removed the yellow but introduced blank frames
(133 of 453 samples). Explicitly clearing the old image inside the synchronized
update removed both artifacts. rbirds now identifies iTerm with `XTVERSION` and
uses that sequence there, leaving other terminals' replacement path intact.
The raster decoder tests check that this does not change image pixels.

The final build's thirty-second capture had zero yellow or blank samples among
2,728 samples. A separate thirty-second prototype capture had zero among 2,754.
These observations cover a finite run, not every possible scheduling condition.
There is a cost: visible changed samples fell from about 58/s with flashes to
46/s with the workaround, while the application's transport continued near
60 Hz. Final presentation p99 was 33.80 ms and maximum was 43.38 ms. This fixes
the severe full-window flash; iTerm Sixel still does not meet the smoothness
objective. The default text and Kitty renderers do not use this workaround.
A further eighteen-second capture exercised panel toggles, a resize from
100×32 to 120×36, trails, depth, four hawks and three flocks. None of its 1,623
samples had the yellow placeholder as the dominant color. Resize-transition
samples are retained and are not counted as steady-state timing evidence.

## Quality and demanding settings

Kitty's own decoder compared original uploads with packed textures at sprite
sizes 4, 30 and 64. All 364 placements in each scene retained their source
pixels, position and stacking order, including overlapping birds, trails,
depth and hawks. The image count fell from 1560 to 26, 29 and 58 respectively.
A foreground static capture at size 64 had zero differing content pixels
after excluding the title bar. `tools/kitty-atlas-check.sh` reproduces the
decoder check with an installed Kitty. A separate foreground comparison in
Ghostty 1.3.1 also had zero differing content pixels for the size-64 scene.

The qualification series at `7e8551f` also included 4096 birds, speed 12, four
hawks, three flocks, depth and trails. Kitty and Braille had no PTY receive
gaps above 25 ms during 1200-frame runs. Sixel had three, with a maximum of
30.51 ms, and used about 883 CPU ms/s. Expensive settings can exceed a frame
budget even before terminal painting.

A 4200-frame default run crossed the 60-second autopilot threshold and included
a resize, panel toggle and deliberate 250 ms reader pause. Its one long gap
coincided with that pause; playback recovered without a catch-up burst.
Large-window blocks, sextants and Sixel runs are also retained. These bounded
runs exercise recovery; they cannot prove that a stall will never occur.

Pixel, C-oracle, protocol, scheduler, cancellation and allocation tests cover
the implementation changes. A steady-state frame still allocates nothing.
Native Windows, Linux and macOS CI covers correctness; measured Windows and
Linux desktop playback remains outside this Mac evidence.

## Raw observations

The [raw archive](live-2026-09-26-macos-arm64.tar.gz) retains comparison samples, frame traces,
presentation timestamps, scripted events, binary hashes and the rejected
experiments. Executables and unrelated application windows are excluded.
Use `tools/trace-report.py` for trace distributions and compare only runs with
matching visibility, geometry, observer and background-work conditions.
