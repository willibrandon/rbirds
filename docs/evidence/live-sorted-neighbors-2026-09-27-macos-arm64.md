# Neighbors in grid traversal order

The flock update now packs the repeatedly read neighbor fields into a compact
scratch buffer in spatial-grid order. Each neighbor lookup reads nearby data
instead of following an index into the larger bird structure and a separate
heading table. The cache keeps the same floating-point precision, traversal
order and random draws. It adds no allocation or worker thread.

Scratch buffers have capacities of 128, 1,024 or 4,096 entries. Fewer than
32 birds, or snapshots beyond the supported population, use the direct path.
The first prototype reserved the largest buffer for every call and regressed
the 32-bird benchmark; it was rejected. Separate, non-inlined helpers keep
large buffers out of small or uncached calls.

The baseline is commit `4a10911`, executable SHA-256
`fcad3c2f10e8ef06f71781178b1d1bdd0cdd02c2e2fe17ea5929d6e249d550f8`.
The final measured candidate is
`022e4fecea23407cadf9dac7f7b75aa37f8624fd21133270a4a27ced4412e962`.
Only the neighbor access implementation and its regression test differ.
Both executables already contain the iTerm Sixel flicker fix.

## Correctness

A new test compares cached and direct updates at every cache boundary, with
and without extra snapshot storage. It checks all bird fields, exact float
bits including signed zero, and random state across five updates. The grid
order deliberately differs from bird order. Review exposed stale indices in
unused grid capacity after a smaller build; the retained failing test captures
that case. Cache construction now reads only the grid's built items. The
expanded test passes in native debug, native release and Rosetta release.

The C boids, C spatial-grid, FMA-site and steady-state allocation suites pass
in native debug, native release and Rosetta release, 75 tests per configuration.
Those broad comparisons initially used the candidate before the active-item
bound correction; native release was repeated on the final implementation.
Clippy with all targets/features and warnings denied, formatting and diff
checks pass on the final source.

## Headless cost

Both executables were warmed up, then measured in before/after/after/before
order with seed 42 at 1600×800. Encoded bytes per frame match for every case.
These timings exclude terminal work and are not live CPU rates. Busy scenes
use speed 12, four hawks, three flocks, depth and trails.

| Scene | Before, ms/frame | After, ms/frame |
| --- | ---: | ---: |
| Default Kitty, 800 birds | 0.2300 | 0.2265 |
| Busy Kitty, 800 birds | 0.7240 | 0.6860 |
| Kitty, 4,096 birds | 1.8855 | 1.6885 |
| Busy Kitty, 4,096 birds | 6.8150 | 5.6755 |
| Default Braille | 0.9370 | 0.9350 |

Longer runs using process CPU counters check small populations without the
benchmark's rounded frame-time display. Means in microseconds per frame are
2.9013 → 2.8947 at one bird, 8.4497 → 8.3051 at 32, 30.9302 → 30.5696
at 128, 31.4182 → 31.1954 at 129, 330.1426 → 319.3546 at 1,024, and
330.3811 → 321.6917 at 1,025. These measurements include amortized startup
CPU; they support avoiding the first prototype's small-flock regression.

## Live CPU

The host was an M4 Pro on macOS 26.5.2, with UFC off. A separately downloaded
iTerm 3.7.3 ran in an isolated preferences suite, using one foreground
100×32 / 1400×1088 window with an opaque dark background. Each renderer ran
in before/after/after/before order, with three seconds of warm-up and a
fifteen-second CPU interval. Runs ended before idle autopilot. Both scenes
use 4,096 birds, speed 12, four hawks, three flocks, depth, trails, ember and
seed 42. No captures, builds, tests or profilers ran during these intervals.

CPU includes rbirds, iTerm's main process and its separate Sixel decoder.
It excludes the compositor, GPU and other processes. The table contains
means of two intervals per executable; raw intervals are in the archive.
All eight intervals have valid counters and unambiguous submission counts.
All 40 sampled focus checks found the owned app active.

| Renderer | App before → after, CPU ms/s | iTerm before → after, CPU ms/s | Decoder before → after, CPU ms/s | Combined before → after, CPU ms/s |
| --- | ---: | ---: | ---: | ---: |
| Dense Kitty | 554.43 → 497.53 | 449.13 → 445.56 | 0 → 0 | 1003.56 → 943.09 |
| Dense Sixel | 858.14 → 841.67 | 1332.27 → 1369.82 | 430.89 → 453.19 | 2621.30 → 2664.68 |

Kitty submitted about 60 frames/s on both sides, with 10.26% lower application
CPU and 6.03% lower combined CPU. Mean update time fell from 6.19 to 5.26 ms.
Sixel submitted 55.99 → 58.57 frames/s, with combined CPU per submission
falling from 46.82 to 45.50 ms. Its combined CPU per second **rose 1.65%** as
the terminal processed more frames. This is a throughput gain, not evidence
of lower total Sixel CPU. No universal percentage saving is established by
two intervals per side.

An earlier candidate, SHA-256
`2827679af5ba362ad7cb36d207e6d640ae20fbf68a481c4fcd4fa822013447d8`,
also has 20 CPU intervals and ten captures in `preliminary/`. It predates the
active-item bound correction and is not the final executable. Default
Braille, Kitty and Sixel remained near 60 visible changes/s, without an
established combined CPU reduction. The final executable's live recheck
covers the two dense scenes; its headless checks include the defaults.

## Separate presentation checks

Each foreground window-surface capture lasted fifteen seconds, starting about
three seconds into a fresh run. All 5,292 samples retained the normal dark
dominant background. All 559 focus observations found the window active and
above other normal windows in its content region. All four screenshots were
inspected; no whole-window yellow or blank sample appeared.

| Scene | Samples | Changes/s | p99 change gap, ms | Maximum gap, ms |
| --- | ---: | ---: | ---: | ---: |
| Dense Kitty before | 1,276 | 59.96 | 31.28 | 34.20 |
| Dense Kitty after | 1,244 | 59.89 | 27.76 | 33.92 |
| Dense Sixel before | 1,368 | 52.78 | 34.26 | 40.73 |
| Dense Sixel after | 1,404 | 53.96 | 33.62 | 37.86 |

Capture CPU was 0.49–0.54 seconds per run. Window-surface samples do not
measure physical scanout or prove zero stutters. Dense Sixel remains below
60 visible changes/s. No Windows or Linux GUI performance claim follows
from these Mac measurements.

The owned test app was quit and its new window-autosave keys removed. The
original preferences compare unchanged, and the installed iTerm and rbirds
executables were not replaced. The archive contains the source patch,
drivers, traces, CPU reports, presentation captures and validation logs.
Private preferences, downloaded apps and executables are excluded.

The [archive](live-sorted-neighbors-2026-09-27-macos-arm64.tar.gz) contains
288 manifested files (11,980,582 compressed bytes). Its SHA-256 is
`450262a5993d67ca27112aadeb9bccb0b890ba41e008d98f3fbbc84c5602d0ab`.
