# WezTerm Sixel placement and CPU comparison

WezTerm `20240203-110809-5046fc22` now receives a cell-aligned image around the
visible sprites, with an opaque glyph background. The same approach already
used in iTerm needed three compatibility changes: match WezTerm's truncated
Sixel RGB percentages, end the concealed startup update before asking for its
cursor position, and finish each background row with an ordinary character.
WezTerm's [REP implementation](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/term/src/terminalstate/mod.rs#L2253)
wraps immediately at the right margin and scrolls at the bottom. An ordinary
final character defers that wrap. The regression model reproduced the blank
last row before this change.

The version response, changeable DECSDM mode, single-width glyph report and
exact cell geometry must all agree before cropping is enabled. Unknown
WezTerm versions retain the full raster. iTerm retains its existing RGB
conversion and explicit image retirement inside synchronized updates.

An isolated process on macOS 26.5.2/M4 Pro used Monaco 12, 100×32 cells and
1400×1056 pixels. Fourteen static captures cover centered birds, clipping at
both edges, an empty scene, trails, hawks, panel changes and resizing to 80×24
and back. Six full/cropped pairs match across every screenshot pixel. The
remaining pair differs in 333 pixels within the bottom-right cell: the full
raster leaves it blank, while the crop restores the intended bird pixels.
An unobscured 80-pixel portion of that cell matches the independent decoder
only in the cropped capture. The empty scene still has the same last-cell
background discrepancy in both paths; its internal terminal cause remains
unresolved. This is not a claim of perfect corner rendering.

Numeric comparison also corrected the earlier visual report that a cropped
panel disappeared. Its entire 532×363 region is identical in the two retained
captures. The new static panel pair also matches exactly. No panel workaround
was added.

The CPU comparison uses preserved commit `1a04f04` binaries and the new build,
seed 42, 180 warmup frames and exactly 600 measured frames. Each workload ran
baseline, candidate, candidate, baseline in the same foreground window. The
normal workload uses defaults with explicit Sixel; dense adds 4,096 birds,
speed 12, four hawks, three flocks, depth, trails and the ember palette. Fixed
simulation inputs match across all four runs in each workload. No builds,
tests, capture or profiling ran during CPU measurement. UFC was off.

| Workload | Build | App ms/frame | Terminal ms/frame | Combined ms/frame | Combined CPU | Submitted frames/s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Normal | Full raster | 2.075 | 41.721 | 43.796 | 109.99% | 25.12 |
| Normal | Cropped | 1.974 | 20.835 | 22.809 | 118.52% | 51.96 |
| Dense | Full raster | 14.513 | 59.181 | 73.694 | 136.78% | 18.56 |
| Dense | Cropped | 14.434 | 46.955 | 61.389 | 146.77% | 23.91 |

Values are means of two runs; 100% CPU is one core. Work per frame falls 47.9%
and 16.7%, but total CPU per second rises with throughput. These results do
not establish a reduction in total CPU usage. The counters exclude GPU and
system compositor work and include boundary handshake overhead.

Separate 15-second window captures used ordinary elapsed-time playback after
startup. They establish visible changes, not physical scanout or identical
simulation trajectories.

| Workload | Build | Visible changes/s | p99 gap | Maximum gap |
| --- | --- | ---: | ---: | ---: |
| Normal | Full raster | 11.00 | 126.79 ms | 136.55 ms |
| Normal | Cropped | 26.34 | 66.93 ms | 72.85 ms |
| Dense | Full raster | 17.81 | 81.38 ms | 128.37 ms |
| Dense | Cropped | 19.48 | 60.63 ms | 82.00 ms |

The throughput improvement is visible, but this terminal still falls short
of smooth 60 Hz playback. These measurements do not justify a zero-stutter
claim. All CPU runs restored terminal attributes and exited successfully.
The four live commands also restored terminal attributes. Every captured
background sample had the expected RGB `[17,17,22]`, and every focus observation
found the window active and foremost.

A final 30-second foreground capture in installed iTerm 3.6.6 used the new
ordinary executable at 100×32 cells. All 2,779 background samples were
`[18,18,23]`, with no yellow or blank samples. It recorded 59.18 visible changes/s,
a 29.97 ms p99 gap and a 46.72 ms maximum gap; all 277 focus observations passed.
The command exited successfully, and only the owned test windows were closed.
The installed `rbirds` binary and terminal preferences were not changed.

All 31 selected Sixel, Sixel PTY, allocation-failure and steady-state allocation
checks pass in native debug, native release and Rosetta release builds.
Formatting and Clippy with warnings denied pass. The checked-in evidence
includes the failing wrap regression before the fix.

The [archive](live-wezterm-placement-2026-09-27-macos-arm64.tar.gz) contains 337
hashed files, including raw protocol, unedited screenshots, CPU reports,
presentation traces, probes, source snapshots and check logs. Its SHA-256 is
`b3c1885dc5f0107d58010f42392d615f0d1c4f86feec697a6c490ebdc6547b23`.
After extraction, `analyze.py` reproduces the summaries using Pillow and NumPy.
All archive hashes and reproduced summaries were checked independently.
