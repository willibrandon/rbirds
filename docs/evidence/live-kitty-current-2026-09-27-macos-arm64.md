# Current Kitty playback checks

Current playback in Kitty 0.44.0 stayed near 60 visible changes per second in
eight captures covering Braille, Kitty graphics, blocks and sextants at two
viewport sizes. No sampled gap exceeded 50 ms, and none of the 10,078 samples
was blank or had a nonblack dominant background. These are finite checks of
the current build, not an optimization comparison or a zero-stutter guarantee.

## Setup

Apple M4 Pro, macOS 26.5.2, AC power, existing power mode 2. Each run used a
dedicated Kitty process with `--config NONE`, default font/colors, an explicit
initial window size, remembered sizing disabled and automatic exit when its
test window closed. Existing Kitty, iTerm and Ghostty processes remained open.
UFC was off. No local builds, tests or profilers ran during measurement.

The normal case used 800 birds and a 114×40-cell / 1596×1000-pixel viewport.
The large case used 171×60 cells / 2394×1500 pixels and 4,096 birds, speed 12,
four hawks, three flocks, depth, trails and ember colors. Both used seed 42.
Size, population and palette change together here; differences between these
cases cannot be attributed to any one setting.

Runtime sources match `b08e959bdcdcf313ab8e5714a691af33dc5133be`, whose change
from `d2a5d58` only coordinates a CI test. The fixed-scene executable SHA-256
is `ecc7236e035ce21011544295efd63af2ff4729705e032e4db462df6018eeb639`;
ordinary playback is
`ecd861f3871f40b7cb6dd2a6a9e4441a5fe88ac7825ca06ae1dee12bef26a43d`.

## CPU

The [fixed-scene harness](live-fixed-scene-2026-09-27-macos-arm64.md) measured
frames 181–1080 after 180 warm-up frames, twice per case. All sixteen final
runs recorded matching initial states within each pair and exactly 900
submissions inside their CPU counter windows. Each interval took about 14.99
seconds. Capture was disabled. A one-second memory guard and a focus check
every five seconds remained active; all 64 focus checks found the owned
application in front. The greatest observed terminal footprint was 274.1 MiB.

Mean CPU milliseconds per submitted frame:

| Case | Application | Kitty main process | Combined |
| --- | ---: | ---: | ---: |
| Normal Braille/default | 3.100 | 2.246 | 5.346 |
| Normal Kitty graphics | 1.320 | 4.705 | 6.025 |
| Normal blocks | 2.841 | 2.243 | 5.084 |
| Normal sextants | 3.051 | 2.193 | 5.244 |
| Large dense Braille/default | 8.835 | 1.036 | 9.872 |
| Large dense Kitty graphics | 7.058 | 8.063 | 15.121 |
| Large dense blocks | 8.588 | 1.221 | 9.809 |
| Large dense sextants | 8.538 | 1.066 | 9.604 |

Pair spreads ranged from 0.15% to 5.04% of their means. These totals cover the
application and Kitty's main process, including its threads. They exclude GPU,
compositor and unselected processes. A process-tree check identified Kitty's
separate `kitten __atexit__` helper; it was not selected. The figures are not
complete system-energy measurements.

The first large dense Braille attempt exited with status 1 before its final
measurement boundary. The harness correctly rejected it and retained the
invalid report. Its terminal settings were restored and the process exited,
but the diagnostic was not saved, so its cause is undetermined. Subsequent
runs redirect diagnostics to files. The invalid attempt remains in the raw
evidence, outside the sixteen valid samples and their averages.

## Ordinary visible playback

Separate runs used the normal elapsed-time application, with 1,500 frames and
a fifteen-second ScreenCaptureKit observation after startup. The observer
requested 120 Hz and compared all content pixels. All 1,114 focus observations
found the owned application active and its content window foremost.

| Case | Changed samples/s | p99 gap, ms | Maximum gap, ms |
| --- | ---: | ---: | ---: |
| Normal Braille/default | 59.952 | 28.856 | 31.374 |
| Normal Kitty graphics | 59.620 | 24.271 | 30.450 |
| Normal blocks | 59.976 | 28.878 | 35.423 |
| Normal sextants | 59.876 | 29.248 | 32.526 |
| Large dense Braille/default | 59.998 | 20.795 | 35.703 |
| Large dense Kitty graphics | 59.814 | 20.888 | 36.084 |
| Large dense blocks | 59.782 | 21.480 | 37.497 |
| Large dense sextants | 60.029 | 27.000 | 29.320 |

Capture itself used 1.14–2.14 CPU seconds per observation, in addition to
compositor cost. These measurements observe window surfaces, not physical
scanout; capture scheduling can delay or coalesce samples. All eight saved
screenshots were inspected. Each normal run restored its terminal settings
and closed its owned process. The captures do not cover the 60-second
autopilot transition, sustained memory behavior or a loaded host.

A separate capability probe returned `CSI ? 62 ; 52 ; c`, without Sixel.
Explicit Sixel startup exited with status 1 and a diagnostic. Its terminal
settings matched those immediately before application launch. The preceding
raw-mode capability probe set the kernel's PENDIN bit; this initially made a
comparison spanning both programs differ. A follow-up captured each boundary
and attributed that difference to the probe, not the application.

[Raw evidence](live-kitty-current-2026-09-27-macos-arm64.tar.gz) contains the
reports, traces, all eight screenshots, rejected attempt, capability probes,
drivers and analysis. Its `analyze.py` reproduces both tables with Python's
standard library. A SHA-256 manifest covers the included files. No executables,
application bundles or private preferences are included.
