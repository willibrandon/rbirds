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

A follow-up used equal thirty-second CPU intervals after three seconds of
warmup, with 2400 frames per run, the same geometry and two alternating pairs.
The candidate was `8dd03da`; the later iTerm Sixel changes do not affect these
two renderers. No capture, builds or local test suites ran during this series.

| Renderer | Application CPU ms/s, before → after | Terminal CPU ms/s | Combined CPU ms/s |
| --- | ---: | ---: | ---: |
| default | 266.7 → 235.3 | 45.8 → 85.1 | 312.5 → 320.4 |
| Kitty graphics | 39.5 → 97.4 | 317.2 → 257.6 | 356.7 → 355.0 |

These are medians of two runs. Default combined CPU was 2.5% higher; Kitty's
0.5% difference is too small to establish a saving. Default candidate totals
ranged from 298.7 to 342.1 ms/s, and Kitty candidate totals from 342.7 to 367.4.
The narrower application and terminal changes are measurable, but the results
do not meet the objective of lower total CPU with smoother playback. The
[equal-time archive](live-equal-time-2026-09-26-macos-arm64.tar.gz) contains every
sample and the launch script. The candidate collected bounded in-memory traces;
the baseline predates tracing.

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
A further eighteen-second capture exercised a resize from 100×32 to 120×36,
trails, depth, four hawks and three flocks. None of its 1,623
samples had the yellow placeholder as the dominant color. Resize-transition
samples are retained and are not counted as steady-state timing evidence.
The script also sent `l`, incorrectly intended as a panel toggle; the actual
live toggle is `h`. This older run does not establish panel-toggle behavior.

A later iTerm sample found substantial stacks in image conversion, vertical
flipping and Metal texture creation. The terminal's mutation queue also waited
for its separate `iTerm2SandboxedWorker` image decoder. Sampling is intrusive:
that run included a 159 ms output wait and is excluded from cadence comparisons.
The stacks identify work to investigate; they do not establish its CPU share.

The terminal harness now accepts explicit `--helper-pid` selections. Two
separate foreground Sixel runs measured 20-second intervals after a three-second
warm-up, with no capture, profiler, build or test running. At 100×32 cells and
1400×1088 pixels, application CPU was 389.19 and 417.45 ms/s, iTerm's main process
1128.34 and 1171.96 ms/s, and the decoder helper 241.54 and 249.16 ms/s. Including
that helper gives combined rates of 1759.08 and 1838.57 ms/s. These are current
cost observations, not before/after improvement claims. Compositor, GPU and
unselected processes remain excluded; this did not use a freshly launched,
fully isolated iTerm process. The dedicated test window was foreground;
activity in other sessions was not measured separately.

Helper accounting was checked against independent `wait4` CPU totals. An exited
helper produces an explicit error and no partial aggregate, including when the
OS can still read its zombie-process counters. The
[follow-up archive](live-followup-2026-09-26-macos-arm64.tar.gz) retains the sample,
CPU observations, validation driver and selected process identities.

## Incremental construction improvements

A follow-up specializes the sprite blend loops and reuses paired sine/cosine
results. Text composition selects whole pixels by alpha; picture composition
uses the fact that a filled background remains opaque. The simulation reuses
the platform's existing paired result without approximating angles or changing
arithmetic order. C-oracle, raster and allocation checks cover these paths.

The deterministic construction comparison used seed 42, two warmups and four
alternating samples per executable. The blend comparison used 300 frames per
sample; the trigonometry comparison used 1500. Every before/after pair had the
same reported output bytes per frame.

| Change | Renderer | Median construction ms/frame, before → after |
| --- | --- | ---: |
| Blend loops | Braille | 1.396 → 1.179 |
| Blend loops | Sextants | 1.398 → 1.172 |
| Blend loops | Blocks | 1.343 → 1.113 |
| Blend loops | Sixel | 1.904 → 1.881 |
| Paired trigonometry, after blend changes | Kitty | 0.336 → 0.328 |
| Paired trigonometry, after blend changes | Kitty with trails, depth, four hawks, three flocks and speed 12 | 1.078 → 1.041 |

The text construction improvement is about 16–17%. Paired trigonometry saves
about 2.5–3.4% in the Kitty construction samples; its Braille and Sixel differences
are too small to establish a saving. These are headless measurements, not total
CPU rates for visible sessions.

A separate foreground comparison measured the blend changes with the same
Kitty geometry and thirty-second CPU intervals described above. It ran original,
pre-blend and post-blend builds, then reversed the order, without screen capture
or concurrent builds/tests. Default application CPU fell from 233.9 to 222.7
ms/s; terminal CPU rose from 95.3 to 120.3, taking combined CPU from 329.2 to
343.0 ms/s. Original-build combined CPU was 307.7 ms/s in this series. In-memory
stage traces show mean composition time falling from about 1.42 to 1.02 ms,
while other stages and terminal CPU varied. Two runs per build do not establish
why the terminal total increased. A shared seed does not make live frame states
identical: simulation steps follow elapsed time. The total-CPU objective remains
unmet, despite the narrower construction and application savings.

Separate presentation captures of the final follow-up build measured 59.90
changed samples/s for default text and 58.92 for Kitty graphics over twelve
seconds. Their p99 intervals were 27.28 and 28.73 ms, and maximum intervals were
38.69 and 35.60 ms. No interval exceeded 50 ms. Capture CPU was 0.73 and 0.77
seconds and is excluded from CPU comparisons. A further eighteen-second iTerm
Sixel capture had no mostly-yellow samples among 1,637 samples; presentation
averaged 47.76 changed samples/s, with p99 34.97 ms and maximum 45.00 ms.
The [follow-up archive](live-kernels-2026-09-26-macos-arm64.tar.gz) contains the
raw observations, launch scripts, source diff and executable hashes.

## Heading reuse and panel updates

The next follow-up computes each snapshot bird's existing heading-table entry
once per simulation step, retaining the neighbor accumulation order. It uses
bounded stack storage and takes the original path below 32 birds. The initial
prototype increased one-bird construction time by about 6%; the small-flock
fallback reduced the measured differences at one and eight birds below 1.5%,
which is too small to establish a change. At 32 and 128 birds, the final samples
fell from 9.55 to 9.27 and from 38.62 to 36.08 microseconds per frame. These small
times are derived from the benchmark's rounded ceiling rate to avoid the much
coarser three-decimal millisecond display.

Against `3f1fa9e`, four alternating samples per executable measured construction
time of 0.331 to 0.303 ms for Kitty, 1.046 to 0.963 ms with trails, depth, four
hawks, three flocks and speed 12, and 2.884 to 2.652 ms with 4096 birds. Braille
fell from 1.126 to 1.078 ms. Output byte counts matched in every comparison.
These runs used 1500 frames, except the 4096-bird case used 300.

Live text and Kitty sessions now send only changed panel rows. The cache is
invalidated on geometry changes, panel removal and changes in row count; the
cursor finishes in the same position as a full panel write. Sixel still writes
the entire panel after each raster. The construction/reference path retains
the original protocol, so its measurements above cover heading reuse rather
than incremental panel output. Terminal-state tests compare text and cursor
state through slider/statistic updates, flock-count changes, panel toggles,
resizing and explicit redraws. Steady-state allocation checks cover the live
panel path.

A separate foreground series compared `3f1fa9e` with the follow-up build,
using two pairs in forward/reverse order, the same dedicated Kitty window and
thirty-second intervals after three seconds of warmup. No capture, builds or
tests ran during those CPU samples. These commands leave the panel hidden by
default, so this series does not measure the incremental panel optimization.

| Renderer | Application CPU ms/s, before → after | Terminal CPU ms/s | Combined CPU ms/s |
| --- | ---: | ---: | ---: |
| default | 223.6 → 207.6 | 120.3 → 104.8 | 343.9 → 312.4 |
| Kitty graphics | 97.3 → 92.2 | 258.0 → 245.9 | 355.3 → 338.1 |

The observed default reduction was about 9% in each pair. Kitty's median reduction was
about 5%, but individual paired totals moved in opposite directions; its
candidate terminal samples ranged from 231.9 to 259.9 ms/s. That series does
not establish a reliable combined Kitty saving. An earlier heading-cache-only
series reduced application CPU slightly but increased combined CPU by about
1.6% for default text and 4.5% for Kitty. Those samples are also retained, rather
than treating construction gains as proof of a visible-session saving.

An isolated panel series then used `--panel` on both executables. The control
was rebuilt from the accepted follow-up source with only the live
`incremental_legend` flag disabled; its patch and both executable hashes are
retained. Two forward/reverse pairs used the same thirty-second intervals and
geometry, with no capture, builds or tests during measurement.

| Renderer, panel visible | Application CPU ms/s, full → incremental | Terminal CPU ms/s | Combined CPU ms/s |
| --- | ---: | ---: | ---: |
| default | 216.5 → 210.2 | 153.0 → 113.8 | 369.5 → 324.0 |
| Kitty graphics | 90.7 → 92.7 | 281.8 → 254.8 | 372.5 → 347.5 |

Default combined CPU fell about 12% in the median, with savings in both pairs.
Kitty's median fell about 7%, but one pair increased and the terminal samples
overlapped substantially. This establishes a narrower text-panel improvement
on this machine, rather than a reliable reduction for every renderer.

Separate twelve-second captures of the follow-up build, with the panel hidden,
measured 59.70 changed samples/s for text and 59.44 for Kitty. Maximum intervals
were 33.25 and 31.12 ms. With `--panel` explicitly enabled, the corresponding
rates were 59.85 and 59.81, with maximum intervals 34.69 and 30.13 ms. None of
these captures had an interval above 50 ms. A further iTerm Sixel capture had
zero mostly-yellow samples among 1,559, but averaged 45.11 changed samples/s
and included one 63.09 ms gap. The flicker workaround does not resolve iTerm's
remaining presentation limit.

Corrected iTerm event runs start with `--panel`, send `h` to hide it, resize from
100×32 to 120×36, then send `h` to restore it. Both Braille and Sixel runs include
trails, depth, four hawks, three flocks and the ember palette. Final screenshots
show the restored panel after resizing. All 1,558 Sixel capture samples retained
a dark dominant background; none showed the yellow placeholder. Their timing
includes resize transitions and is not a steady-state latency measurement.
The [heading and panel archive](live-panels-2026-09-26-macos-arm64.tar.gz) retains
these samples, the prototype results, corrected event scripts, screenshots of
the dedicated test windows, source changes and executable hashes.

## Cell color selection

A fresh six-second application stack sample of `c1513cb` identified
`read_patch` as the largest active stack in default text playback. The sample
ran separately from CPU comparisons. The reader calculated both a dominant
color and an average for every patch, although it uses only the dominant color
until its eight-entry color table fills.

The reader now calculates the average only when the eighth distinct visible
color appears, then stops building the histogram. That fallback restarts the
original floating-point sums in the original pixel order. Transparent colors
do not count toward the limit, and dominant-color ties retain their ordering.
The C cell oracle covers both paths, including a new boundary case with an
eighth color at different positions and alpha values. Glyphs, emitted bytes
and reconstructed pixels remain exact; steady-state allocation checks pass.

Two alternating construction pairs used seed 42 and 1,000 frames per run,
after one warm-up per build. Median frame construction times were:

| Renderer | Ordinary sprite, ms before → after | Shaded custom sprite, ms before → after |
| --- | ---: | ---: |
| Braille | 1.094 → 1.015 | 1.365 → 0.938 |
| Sextants | 1.086 → 1.016 | 1.363 → 0.938 |
| Blocks | 1.032 → 0.911 | 1.337 → 0.909 |

The custom sprite is a retained 32×32 gradient disk with many distinct colors.
Output byte counts matched within each scenario. These construction results
exclude terminal work and do not measure presentation.

Foreground comparisons used dedicated Kitty 0.44.0 processes, the same
114×40-cell / 1596×1000-pixel viewport, seed 42 and 1,800 ticks. Each CPU sample
covered twenty seconds after three seconds of warm-up. Two pairs ran in
before/after/after/before order, without capture, profiling, builds or tests.

| Renderer | Application CPU ms/s, before → after | Terminal CPU ms/s | Combined CPU ms/s |
| --- | ---: | ---: | ---: |
| default | 223.25 → 193.05 | 127.01 → 113.36 | 350.27 → 306.41 |
| blocks | 216.97 → 195.50 | 127.20 → 132.18 | 344.17 → 327.68 |

These are medians of two runs per build. Combined CPU fell in both pairs:
5–20% for default and 2–8% for blocks. Terminal readings varied, and the
simulation follows elapsed time, so the median is not a guaranteed saving.
Compositor and GPU costs remain outside these counters. This change does not
affect Kitty graphics or Sixel rendering.

Application traces stayed near 60 Hz. One default candidate run had a 25.11 ms
start interval; the other seven runs had none above 25 ms. Median encoding
time fell from about 1.73 to 1.35 ms for default and from 1.69 to 1.19 ms for
blocks. Separate twelve-second foreground captures measured 59.69 and 59.83
changed samples/s respectively. Their p99 intervals were 26.90 and 25.29 ms,
and maxima were 30.37 and 37.89 ms, with no gaps above 50 ms. The capture
process used about 0.75 CPU seconds in each run. These samples do not prove
physical scanout timing or the absence of every possible stall.

An additional iTerm 3.6.6 Kitty-protocol check produced a blank window: all 936
capture samples had the same content hash. A capability query returned `OK`.
A smaller probe then isolated image addressing: an upload using image number
`I` succeeded, but placements using that number received no reply and drew
nothing. Uploads and placements using explicit image ID `i` returned `OK`
and appeared, including cropped placements. This identifies another
compatibility issue to resolve; the text-reader change does not address it.

Raw CPU intervals, traces, construction runs, captures, protocol replies and
validation logs are retained in
[`live-cell-colour-2026-09-26-macos-arm64.tar.gz`](live-cell-colour-2026-09-26-macos-arm64.tar.gz),
with source changes, executable hashes and an independently checked manifest.

## Paused playback

Previously, pause stopped flight but continued rebuilding the spatial grid and
rendering the same scene at 60 Hz. Kitty placements and full Sixel images were
still submitted. Live playback now retains an unchanged paused frame while
continuing to process controls, resize, single-step, autopilot and quit. Pointer
reports alone do not repaint the held image. Unlocked playback also waits while
paused. The panel shows zero ongoing frame cost and rate until playback resumes.

Final-build foreground before/after pairs measured a ten-second interval
after seven seconds of warm-up. A space was sent at about four seconds after
window launch; the trace records idle ticks and no submitted frames throughout
each candidate measurement period. No builds, tests, profiler or capture ran during the
CPU samples. Default text and Kitty used dedicated Kitty 0.44.0 processes;
Sixel used iTerm 3.6.6 and its selected image-decoder helper.

| Paused renderer | Application CPU ms/s, before → after | Terminal CPU ms/s | Helper CPU ms/s | Combined CPU ms/s |
| --- | ---: | ---: | ---: | ---: |
| default text in Kitty | 204.43 → 1.95 | 93.83 → 0.14 | — | 298.26 → 2.09 |
| Kitty graphics | 50.06 → 2.22 | 296.17 → 0.10 | — | 346.23 → 2.33 |
| Sixel in iTerm | 296.71 → 2.08 | 1176.34 → 32.82 | 211.10 → 0.00 | 1684.16 → 34.89 |

These are paused-state observations, not improvements to moving animation.
The iTerm process was already running, so its main-process count can include
other sessions. Compositor and GPU work remain outside the counters. The
application still polls input and window size at 60 Hz; it does not busy-wait.

Deterministic tests compare simulation state, canvas pixels and each submitted
frame with full rendering in all five modes. They cover controls, split mouse
reports, stepping, resizing, autopilot, input handled during a blocked flush,
resume and the outro. A PTY test checks no output while idle, exactly one frame
for a step, normal restoration and bounded idle ticks even with unlocked FPS.
The C-oracle path and steady-state allocation checks pass.

A separate iTerm capture covered pause, single-step, panel removal, resize from
100×32 to 120×36, panel restoration and resume with trails, depth, four hawks
and three flocks. All 1,407 sampled frames retained a dark dominant background;
none showed the yellow placeholder. Screenshots show zero ongoing panel
statistics while paused and restored statistics after resuming. This event run
is not used as steady-state cadence or CPU evidence.

The trace and PTY reports now distinguish loop ticks from submitted frames.
The PTY harness verifies submitted counts against the trace and separately
checks the final restoration marker. Timing distributions exclude intervals
crossing intentional idle ticks. Legacy traces, idle-only traces and a
pause/resume trace were checked explicitly.

The [paused-playback archive](live-paused-2026-09-26-macos-arm64.tar.gz) retains
measurements, traces, validation logs, drivers and source changes.

## Further iTerm and neighbor-cache checks

A fresh thirty-second foreground Sixel capture of `dcbfd4c` found zero
mostly-yellow samples among 2,731. The greatest yellow-pixel fraction was
10.14%, consistent with the yellow birds on the dark background in the saved
screenshot. Changed samples averaged 46.95/s, with a p99 interval of 34.25 ms
and maximum of 41.75 ms. The capture process used 1.15 CPU seconds; this was
not a CPU comparison. The 2,400-frame command exited successfully, and its
dedicated window was closed. This confirms that the subsequent CPU changes
retain the Sixel flicker workaround; the presentation-rate limitation remains.
All seven Sixel regression tests passed again, including exact raster decoding,
erase ordering, resize and terminal identification.

An iTerm-only Kitty prototype replaced image-number addressing with explicit
image IDs. Birds became visible, but the application submitted only about 21
frames/s and a separate ten-second capture measured only 2.53 changed samples/s,
with a maximum interval of 704.87 ms. The 1,200-frame command took 58.17 seconds.
This prototype was discarded.

A separate intrusive iTerm profile traced most display-command samples through
the rebuilding of the draw list. In iTerm 3.6.6, each
[placement invokes its delegate](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/KittyImageController.swift#L805),
which immediately
[rebuilds the list](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/VT100ScreenMutableState.m#L6584).
That operation
[filters, converts and sorts all placements](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/KittyImageController.swift#L1065).
With hundreds of bird placements per frame, this repeats substantial terminal
work. Addressing compatibility alone therefore does not establish usable
Kitty playback in this version of iTerm. These profile stacks are diagnostic
evidence, not a CPU reduction measurement.

A separate simulation experiment packed neighbor positions, flock, layer and
cached heading into a contiguous array. It retained arithmetic and traversal
order and passed 17 C simulation comparisons, four floating-point checks and
three allocation checks. Two alternating construction pairs gave:

| Configuration | Before ms/frame | Prototype ms/frame |
| --- | ---: | ---: |
| Kitty, default population | 0.2920 | 0.2885 |
| Kitty, trails/depth/hawks/flocks, speed 12 | 0.9325 | 0.8985 |
| Kitty, 4,096 birds | 2.5625 | 2.4460 |
| Kitty, 32 birds | 0.0090 | 0.0095 |
| Braille, default population | 1.0005 | 1.0030 |

Every pair retained identical output byte counts; that check alone does not
establish pixel equivalence. These are construction timings without terminal
painting. The modest crowded-case gain came with a larger stack cache and no
demonstrated default-session saving. The experiment was discarded without
claiming an improvement in live CPU use.

The [diagnostic archive](live-diagnostics-2026-09-26-macos-arm64.tar.gz) retains
both rejected patches, benchmark samples, profiles, captures, executable
hashes and an independently checked file manifest. Neither prototype is in
the accepted source or the current release executable.

## Cropped Sixel frames in iTerm

The full-window Sixel raster made iTerm decode, convert and upload empty sky
along with the birds. The renderer now tracks the conservative bounds of all
visible sprites, including trails and hawks, and encodes that rectangle
directly from the canvas rows. The crop starts and ends on terminal-cell
boundaries. An opaque full-block backdrop covers the entire viewport before
the image is placed, inside the same synchronized update and explicit erase.
Non-integral cell geometry retains the full raster. A startup cursor query
also requires the block character to occupy exactly one cell; a wide character
or missing response retains the full raster. Other terminals retain
their existing path; sprite resolution and simulation arithmetic are unchanged.

Three simpler backdrops failed qualification. Ordinary text background colors
matched an opaque window but became translucent when window transparency was
enabled. Full-block glyphs only outside the crop passed a static comparison
but left default-background rectangles during motion. A repeated capture of
that rejected implementation found the wrong color in as much as 38.15% of
sampled pixels. Moving the outside-only backdrop after the image also failed,
with wrong-color regions covering as much as 34.59% of samples and missing
birds. Painting the entire opaque backdrop before the image removed those
rectangles.
The precise internal iTerm invalidation behavior was not instrumented.

The capture tool now accepts specific RGB colors to count in every sample,
including colors too sparse to become dominant. It also records translucent
samples. In separate ten-second captures of the corrected build, the unwanted
default-background color appeared in at most one of 5,984 sampled pixels per
frame in the ordinary window, and none in the transparent window. Screenshots
show a uniform dark simulation background in both. These counters sample a
window including its edges; they do not replace inspection of the image.

Two foreground CPU pairs ran in before/after/after/before order, using the same
100×32-cell, 1400×1088-pixel iTerm 3.6.6 window, seed 42 and 1,800 loop ticks.
Each CPU interval covered twenty seconds after three seconds of warm-up.
No capture, profiler, local build or test ran during those intervals.

| Counter | Before CPU ms/s | After CPU ms/s |
| --- | ---: | ---: |
| Application | 411.23 | 395.40 |
| iTerm main process | 1143.95 | 818.10 |
| Image decoder helper | 252.31 | 134.97 |
| Combined | 1807.49 | 1348.47 |

These are medians of two runs per executable: about 25% lower combined CPU,
with both pairs improving. The iTerm process was shared with existing sessions;
the compositor, GPU and unselected processes remain excluded. The earlier
31% and 27% observations belong to rejected backdrops and are not the accepted
result.

A first demanding-settings pair (4,096 birds, trails, depth, four hawks,
three flocks and speed 12) increased combined CPU from 2574.30 to 2629.02 ms/s,
about 2%. Two subsequent pairs in before/after/after/before order reduced it
from 2584.21 to 2489.55 ms/s, about 4%, with both pairs improving. The initial
increase is retained as an indication of terminal-process variability. The
same twenty-second intervals and separate main/helper accounting were used;
this does not establish a saving at every setting. Separate ten-second captures
of the demanding case improved from 25.32 to 31.47 changed samples/s, with
p99 intervals of 52.35 to 42.73 ms and maxima of 53.67 to 44.75 ms. Neither
build achieved 60 visible updates/s at those settings.

Separate ordinary and transparent-window captures measured 59.60 and 59.92
changed samples/s, with p99 intervals of 29.34 and 27.71 ms and maxima of 37.75
and 34.05 ms. A preceding thirty-second full-backdrop capture averaged 59.49/s
but contained an 82.46 ms gap. Its application trace also had a 60.72 ms start
interval and a 48.24 ms update-stage outlier. The cause of that scheduling
outlier was not established; neither an average near 60 Hz nor the later short
runs proves zero stalls. The earlier accepted full-raster capture averaged
46.95 changed samples/s.

Independent protocol decoding compares the reconstructed opaque viewport with
the full raster through crop-size changes, off-screen sprites, clipping,
trails, hawks, empty scenes and resizing. It checks that the entire opaque
backdrop precedes each cropped image. Exact C comparisons, pause behavior,
PTY negotiation/restoration and steady-state allocation checks pass. Static
opaque and transparent-window comparisons of the initial block backdrop had
zero differing bird or sky pixels; 11–16 panel pixels differed by at most two
channel levels. The raw comparisons retain these small text differences. Short alternating
construction comparisons for Kitty, Braille and full-raster Sixel retained
identical byte counts, with median frame times within 1.5% of the previous
build; these are not terminal-presentation measurements.

A separate twenty-second control capture covered pause, single-step, panel
hide/show, resize and resume. All 1,465 samples retained a dark dominant
background. Intentional paused intervals are excluded from steady-playback
claims. The screenshots and trace retain the event sequence.

An additional temporary profile set iTerm's minimum-contrast control to 0.5.
Both the full-raster and cropped builds retained a dark background in separate
captures (800 and 924 samples); the profile and its own windows were removed.
An exploratory native-inline-image backdrop was not qualified: iTerm requested
permission to display the image. The prompt was declined and the probe stopped.
No inline-image backdrop or profile-setting changes are part of the renderer.

A double-width-character profile then exposed a separate defect: the block
backdrop left stripes, with the default-background color covering up to 21.72%
of sampled pixels. The final build probes the block character's width once in
the empty alternate screen, with the probe hidden and erased within a
synchronized update. Only a one-cell cursor advance enables cropping; missing,
malformed or wide replies retain the full raster and the original flicker fix.
A normal-profile capture and a wide-profile capture then contained zero sampled
pixels of that unwanted background color (1,093 and 1,185 samples). They measured
56.04 and 24.69 changed samples/s respectively, with maximum gaps of 43.68 and
61.73 ms. The full-raster fallback preserves the image, but carries no claim of
60 Hz playback. Temporary profiles and test windows were removed.

The [width-check archive](live-sixel-width-2026-09-26-macos-arm64.tar.gz) retains
the striped failure, corrected captures, profile settings, source patch,
executable hashes and passing rendering, PTY, allocation and lint checks. The
CPU pairs above predate this startup guard; no steady-frame work changed for
the ordinary one-cell profile.

The [Sixel crop archive](live-sixel-crop-2026-09-26-macos-arm64.tar.gz) retains
all variants, CPU intervals, traces, captures, validation logs and source
patches, with executable hashes and an independently checked manifest.

## Wider atlas experiment

A follow-up prototype packed sprites into two-dimensional pages, keeping the
near birds, far birds and hawks/trails in separate images. Kitty 0.44.0 scans
uploaded images when resolving a placement and groups adjacent placements
sharing a texture. The prototype reduced the decoder scene from 26, 29 and
58 strip images to three pages at sizes 4, 30 and 64. All 364 source rectangles,
positions and drawing order remained identical. A foreground size-64 capture
with text crossing the sprites had zero differing pixels in the checked
content area. The decoder harness now also rejects image groups that cross
the text layer, which matters because Kitty draws those layers separately.

This did not reduce measured CPU. Two alternating foreground pairs used the
same seed, window and 30-second interval after a three-second warm-up, without
capture or profiling. The page prototype's median application CPU rose from
89.30 to 99.20 ms/s, terminal CPU from 248.59 to 249.55 ms/s, and combined CPU
from 337.89 to 348.75 ms/s. One pair improved combined CPU and the other
regressed; application CPU increased in both. Crop placements also needed more
protocol fields. The implementation was discarded, retaining the vertical
strips and the additional decoder check. Reducing the image count further did
not establish a playback benefit.

The [atlas experiment archive](live-followup-2026-09-26-macos-arm64.tar.gz)
contains both pairs, traces, source patch, executable hashes, scripts and
captures of the dedicated static test windows.

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
a resize and deliberate 250 ms reader pause. Its one long gap
coincided with that pause; playback recovered without a catch-up burst.
Large-window blocks, sextants and Sixel runs are also retained. These bounded
runs exercise recovery; they cannot prove that a stall will never occur.
That run's injected `l` was not a panel toggle. The PTY harness now defaults to
the correct `h` key, matching the interactive controls rather than the `-l`
command-line option.

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
