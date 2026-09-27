# Rejected Sixel plane reservation prototype

A prototype reserved output space once per foreground color plane, then
wrote that plane's runs into the reserved slice. A compressed run is never
longer than its literal columns, so the reservation has a known upper bound.
The buffer was truncated to the number of bytes actually written. This removed
repeated capacity checks in the run loop without changing the Sixel stream,
palette, resolution or simulation. It added no unsafe code or worker thread.

The prototype was **not retained**. Isolated encoding was faster, but the live
comparisons did not establish consistent combined CPU savings or improved
presentation. Production keeps the existing encoder. The failure-recovery
regression test described below is retained.

The baseline is commit `809d867`, executable SHA-256
`022e4fecea23407cadf9dac7f7b75aa37f8624fd21133270a4a27ced4412e962`.
The candidate is
`78c24dd5c07ab3cae729866837603b4ce152904e420b43b1274308ef005c5fa5`.
Both already contain the iTerm Sixel synchronized-erase and opaque-background
workaround for whole-window flashes.

## Correctness and isolated cost

The encoded bytes match for 17,408 generated whole-image and crop cases,
covering every alpha value, widths around block boundaries, partial bands and
changing dimensions. Fifteen deterministic simulation images also have equal
streams and decoded pixels. The simulation corpus samples frames 60, 180,
300, 450 and 600 at 1400×1088 with seed 42 and whole-cell crops. Its default
scene uses the headless black foreground; live runs negotiate terminal colors.

Each codec interval encodes the same five saved images 100 times, in
before/after/after/before order. These timings exclude simulation, transport
and terminal work.

| Scene | Before, ms/encode | After, ms/encode |
| --- | ---: | ---: |
| Default parameters | 0.4880 | 0.4625 |
| Dense | 2.6957 | 2.5301 |
| 32 birds | 0.06974 | 0.06998 |

Whole-application headless benchmarks at 1600×800 use 500 frames per interval,
seed 42 and the same ordering after warm-up. Default Sixel falls from
1.5665 to 1.5025 ms/frame, dense from 11.6045 to 11.2130, and sparse from
0.2980 to 0.2975. Output bytes per frame are unchanged. The dense scene uses
4,096 birds, speed 12, four hawks, three flocks, depth, trails and ember.
These headless timings are not live CPU rates.

An earlier prototype batched individual run appends. It produced identical
bytes but was 3–4% slower in the codec comparison, so it was rejected. Its
source and measurements remain in the archive.

The Sixel raster, Sixel PTY, allocation-failure and steady-state allocation
suites pass in native debug, native release and Rosetta release, 28 tests
per configuration. A new failure-recovery case warms the plane storage,
refuses output growth after plane construction, and verifies that retrying
with different colors discards partial data. Clippy with warnings denied,
formatting and diff checks pass.

## Measurement conditions

The host was an M4 Pro on macOS 26.5.2, with UFC off. A separately downloaded
iTerm 3.7.3 used an isolated preferences suite and one foreground 100×32 /
1400×1088 window with an opaque dark background. CPU runs use three seconds
of warm-up and fifteen-second intervals, in before/after/after/before order.
Each run ends before idle autopilot starts. No captures, profilers, builds or
tests run during CPU measurement. Counters include rbirds, iTerm's main
process and its separate Sixel decoder, excluding the compositor and GPU.

## Live CPU

The table gives means of two intervals per executable in CPU milliseconds
per second. All twelve intervals have valid counters and unambiguous
submission counts. All 60 sampled focus checks found the owned app active.

| Scene | App before → after | iTerm before → after | Decoder before → after | Combined before → after |
| --- | ---: | ---: | ---: | ---: |
| Default | 110.96 → 113.30 | 743.55 → 768.62 | 123.00 → 127.99 | 977.52 → 1009.91 |
| Dense | 842.83 → 830.14 | 1372.69 → 1385.44 | 449.90 → 453.15 | 2665.42 → 2668.73 |
| Sparse | 23.85 → 23.20 | 510.00 → 489.20 | 44.34 → 40.46 | 578.20 → 552.86 |

Combined CPU changed by +3.31%, +0.12% and −4.38%, respectively. Default
and sparse submissions stayed near 60 Hz; dense submissions rose from
58.60 to 59.23 Hz. Dense combined CPU per submission fell from 45.49 to
45.06 ms, a small change without a clear visible cadence improvement.

Frame traces show lower encoding wall time: default 0.773 → 0.753 ms and
dense 4.126 → 3.808 ms. However, the default candidate runs also spent more
time in the unchanged simulation and composition stages and averaged about
3% more output bytes. The live simulation follows measured elapsed time, so
these runs do not contain identical scene inputs. This prevents attributing
the entire default CPU increase to the encoder; it also prevents claiming
a reliable total CPU saving. The fixed-input codec measurements isolate
encoding work, but their improvement alone does not qualify this prototype
against the live CPU and smoothness objective.

## Separate presentation checks

Each capture lasted fifteen seconds, starting about three seconds after real
startup. All 8,037 samples retained the normal `[18,18,23]` dominant background.
All 839 focus observations found the window active and above other normal
windows in its content region. All six screenshots were inspected, with no
whole-window yellow or blank sample.

| Scene | Samples | Changes/s | p99 change gap, ms | Maximum gap, ms |
| --- | ---: | ---: | ---: | ---: |
| Default before | 1,317 | 59.96 | 27.34 | 41.93 |
| Default after | 1,309 | 60.02 | 27.06 | 34.38 |
| Dense before | 1,394 | 54.96 | 34.05 | 41.65 |
| Dense after | 1,409 | 55.35 | 34.10 | 44.70 |
| Sparse before | 1,329 | 59.88 | 27.48 | 32.15 |
| Sparse after | 1,279 | 60.05 | 29.23 | 36.43 |

Capture CPU was 0.43–0.54 seconds per run. The small dense cadence difference
does not establish a smoothness improvement. Window-surface capture does not
measure physical scanout or prove zero stutters; dense Sixel remains below
60 visible changes/s. No Windows or Linux GUI performance claim is made.

The new allocation-recovery test also passes against the retained production
encoder in native debug, native release and Rosetta release. Rebuilding after
removing the prototype restores the baseline executable hash above.

The owned test app was quit and its new window-autosave keys removed. The
original preferences compare unchanged; the installed iTerm and rbirds were
not replaced. The archive retains both rejected prototypes, their source
patches, drivers, CPU and capture traces, screenshots and validation logs.
Private preferences, downloaded apps and executables are excluded.

The [archive](live-sixel-reservation-2026-09-27-macos-arm64.tar.gz) contains
138 manifested files (3,445,358 compressed bytes). Its SHA-256 is
`a1a85267fcc35e26dc6e3359fd62e8277b87095f4c63ddd66cfff124383307e5`.
