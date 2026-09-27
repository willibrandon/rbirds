# Fixed-scene CPU measurement checks

The CPU harness now measures a specified range of identical simulation steps
through the real terminal path. Eight iTerm runs passed: each pair recorded
matching initial state and exactly frames 181–1080 inside the counter window.
The same executable was repeated, so these results qualify the measurement
method and do not establish an optimization or a display-smoothness gain.

The [preceding Sixel experiment](live-sixel-reservation-2026-09-27-macos-arm64.md)
exposed the problem: identical seeds in ordinary playback do not produce
identical frames when elapsed-time differences change the flock's movement.
That can change rendering cost even when the renderer itself is unchanged.

## Measurement path

`cargo build --release --example fixed-scene` builds a separate executable.
It shares live startup, terminal negotiation, rendering, output and pacing,
but advances the simulation by frame number at 60 logical steps per second.
The normal `rbirds` entry point retains elapsed-time simulation. Const-generic
specialization removes the controlled path's input, geometry and gate checks
from normal playback.

`tools/terminal-perf.py --fixture-frames 180:900 --trace ...` reads process
counters while the child waits at the two frame boundaries. The trace must
independently place all 900 submissions strictly inside the counter window.
Missing boundaries, child failure, incomplete traces, input and resizing
invalidate the run. `tools/compare-fixed-scenes.py` requires matching initial
state, renderer, frame range, platform and selected terminal process names.
It reports CPU per frame separately from CPU per elapsed second.

Initial state is a versioned diagnostic representation, not a portable replay
format. Changed code must still pass simulation and image-correctness checks.
Matching inputs alone cannot establish matching outputs. Terminal version,
configuration, host, visibility and background load remain operator controls.

The waits add a small boundary cost and are not display fences. Decoding and
presentation can lag a completed flush. Ordinary live CPU and visible-window
checks are still required for pacing, controls, resize, overload and smoothness.

## Real-terminal qualification

Host: Apple M4 Pro, macOS 26.5.2. A separate iTerm 3.7.3 process used an opaque
dark profile, Monaco 12, and a 100×32-cell / 1400×1088-pixel viewport. UFC was
off. There were no captures, profilers, builds or tests during CPU sampling.
A one-second memory guard and a focus check every five seconds remained active;
all 32 focus observations found the test application in front. The decoder
helper was selected explicitly. Compositor, GPU and unselected processes are
outside these CPU totals.

The executable was built over `e88871cb4b901d552e56942999f2018461c12ebe` with
the archived patch. SHA-256:
`ecc7236e035ce21011544295efd63af2ff4729705e032e4db462df6018eeb639`.
Each case ran twice with seed 42, 180 warm-up frames and 900 measured frames.
Dense Sixel used 4,096 birds, speed 12, four hawks, three flocks, depth, trails
and ember colors. The default case selected Braille.

Mean CPU milliseconds per submitted frame:

| Case | Application | iTerm | Decoder | Combined | Pair spread / mean |
| --- | ---: | ---: | ---: | ---: | ---: |
| Default | 1.665 | 4.415 | 0.000 | 6.080 | 1.23% |
| Sixel | 1.927 | 13.698 | 2.204 | 17.829 | 0.92% |
| Kitty | 1.009 | 6.158 | 0.000 | 7.167 | 0.37% |
| Dense Sixel | 14.460 | 23.657 | 7.769 | 45.886 | 0.22% |

The first three cases took approximately 14.99 seconds per measured interval;
dense Sixel took 15.22 and 15.20 seconds. The short pairwise spreads support
repeatability in this session; they are not confidence intervals or a guarantee
under different host load. No display-cadence conclusion is drawn from them.

An initial eight-run qualification used a direct process exit in the example.
It is retained under `preliminary` but excluded from the table. Review changed
the entry point to return `ExitCode` so normal Rust destruction runs, then
repeated all eight measurements. Terminal restoration already removed the
registered shared images; no shared-image leak was established by this change.
The test application was closed afterward, the original iTerm process remained
open, and the original preferences compared equal.

## Correctness checks

All 25 Python measurement checks pass, including frame-boundary sampling,
missing/late boundaries, frame mismatches, rejection of elapsed-time traces,
invalid options and preserving CPU counters when a child is cancelled.
The fixed-scene PTY tests compare complete Braille, Sixel and Kitty output
across normal pacing, unlocked output and a delayed reader. Input and resize
cases fail the run and restore the terminal. These pass in native debug,
native release and Rosetta release; the final native release run includes the
explicit child cleanup change.

The integer clock test spans 180 logical seconds without cumulative rounding
error. Another 57 release checks cover ordinary terminal playback, Sixel,
Kitty raster/shared images, allocation failure and steady-state allocation.
Formatting, Clippy with warnings denied, and patch whitespace checks pass.

[Raw evidence](live-fixed-scene-2026-09-27-macos-arm64.tar.gz) includes final and
clearly separated preliminary reports, frame traces, drivers, source and test
logs. Run its `analyze.py` to reproduce the table from final reports. The
archive manifest records every included file's SHA-256; private preferences,
applications and executables are excluded.
