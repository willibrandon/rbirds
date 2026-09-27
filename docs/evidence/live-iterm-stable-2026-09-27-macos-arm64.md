# iTerm Kitty version requirement and stable-release qualification

Live Kitty startup now requires iTerm 3.7.3 or a dated nightly from September
19, 2026 onward. iTerm's [3.7.3 release notes](https://iterm2.com/downloads.html)
explicitly identify the animation texture-memory fix, and its
[renderer source](https://github.com/gnachman/iTerm2/blob/v3.7.3/sources/MetalRenderer/Renderers/KittyImageRenderer.swift)
prunes textures absent from the current frame. A fresh check of the official
downloads found this stable release after the earlier nightly qualification.

Affected or unrecognized iTerm versions receive a startup error before any
Kitty upload or alternate-screen entry. The message gives the required stable
version and the Sixel/text alternatives. This avoids silently changing an
explicit renderer request. Sixel and text startup are unaffected; other Kitty
terminals retain their existing renderer. Nightly dates are handled separately
from stable version numbers so an older nightly's large patch number cannot
bypass the check.

## Why reusable sprites were rejected

A separate prototype reused the ordinary Kitty sprite atlas, substituting
explicit image IDs because iTerm's image-number addressing does not work for
this scene. It uploaded the sprite textures once instead of uploading composed
frames. Its default-scene foreground capture on installed iTerm 3.6.6 contained
665 samples, but only 37 changed images: 3.79 changes/s with a 507.80-ms maximum
gap. All 93 focus observations confirmed the window was in front.

The terminal's sampled footprint stayed between 820.27 and 824.27 MiB during
this short capture. That is consistent with reusing uploaded textures, but
the severe cadence failure already ruled out this alternative. No CPU-saving
claim, dense-scene run or long-duration memory qualification was made for it.
The screenshot, trace, protocol changes and failed capture are retained.

## Checks and test configuration

Native Apple Silicon and Rosetta checks passed: one portable version-policy
test, five Kitty PTY tests, seven Sixel PTY tests and nine Sixel codec/rendering
tests on each architecture. They cover fragmented replies, affected and fixed
versions, nightly boundaries, unsupported version formats, ordinary Kitty
terminals, rejected shared-memory transport, interrupted output and terminal
restoration. Clippy passed with warnings denied. A real run in installed iTerm
3.6.6 exited with the expected diagnostic in approximately 15 ms.

The stable app was downloaded from the official
[3.7.3 archive](https://iterm2.com/downloads/stable/iTerm2-3_7_3.zip).
Its SHA-256 matched the published value:
`eb7a166061e58602e3d4bdf69d92f2c8cf6a63feed002f6adc07128a71c8dc39`.
Deep, strict signature verification passed for GEORGE NACHMAN / H7V7XYVQ7D.
It ran from the ignored work directory with a separate preferences suite and
one owned test profile. The installed app was not upgraded.

The host was an M4 Pro on macOS 26.5.2, with UFC off and other iTerm sessions
idle. The profile used Monaco 12, 100×32 cells / 1400×1088 native pixels and an
opaque RGB 21,25,30 background. Default and dense runs used seed 42. The dense
scene used 4,096 birds, speed 12, four hawks, three flocks, depth, trails and
the ember palette. CPU/memory runs were separate from presentation captures.
Runs beyond one minute include rbirds' normal idle drift of flocking sliders;
their early and later CPU rates are not fixed-configuration comparisons.
The rbirds executable SHA-256 was
`9222e21aa4b6e250acb76ee857486c3c035bbf859cfa2183d4595316fa800f7b`.

## Sustained Kitty observations

Each run lasted approximately 174 seconds, including startup and exit. After
five seconds of warm-up, six fifteen-second CPU intervals were separated by
fifteen-second gaps. A native footprint guard sampled about once per second;
all 34 application-focus checks in each run found the test app active. No
builds, tests, capture or profiler ran during these measurements.

| Scene | App CPU, ms/s | iTerm CPU, ms/s | Sampled peak footprint, MiB | Mean footprint at 5–60 s, MiB | Mean footprint at 110–172 s, MiB |
|---|---:|---:|---:|---:|---:|
| Default | 60.26 | 357.91 | 418.94 | 312.74 | 342.73 |
| Dense | 544.65 | 422.94 | 450.69 | 407.70 | 369.70 |

All twelve intervals had valid process counters and unambiguous counts of
899–901 completed frame submissions. CPU excludes the compositor, GPU and
unselected helpers. These are absolute observations; no matched comparison
isolates a CPU saving from the version check or terminal upgrade.
The app's footprint increased by approximately 0.83 MiB in each run, which
includes its growing trace-record buffer. The one-second terminal samples
exclude transient peaks between observations. The runs do not reproduce the
old terminal's multi-gigabyte growth within seconds, but they do not prove
bounded memory for arbitrary durations or settings.

## Separate presentation checks

Both fifteen-second Kitty captures retained a dark dominant background, and
all 140 focus observations per scene found the test window in front.

| Scene | Samples | Changes/s | p99 change gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|
| Default | 1,277 | 60.03 | 28.79 | 30.41 |
| Dense | 1,328 | 59.99 | 32.22 | 35.99 |

Capture-process CPU was 0.50 and 0.52 seconds respectively. These window-surface
samples are not physical scanout, and gaps near two frame periods remain.
They establish neither flawless timing nor continuously unobstructed visibility
between the approximately 10-Hz focus observations.

The paused check retained one image across 216 samples. A separate control run
covered single-step, panel hide/show, resize to 120×36 and resume. Its 1,311
samples had a dark dominant background and all 176 focus observations passed.
The default, dense, static, paused and resumed screenshots were inspected.

The complete static viewport matched the previously qualified September 26
nightly capture with zero differing RGBA pixels. Compared with the older 3.6.6
capture, 6,515 pixels differed inside the panel, mostly along the rounded border;
outside the panel every pixel matched. The inset panel interior had nine
one-level channel differences. Both comparisons and their rectangles are kept.

## Braille and Sixel on the stable release

Four additional fifteen-second foreground captures checked the other remaining
Mac delivery concerns. Braille's early and later windows began about three and
43 seconds into the same run. Sixel used separate default and dense runs. Every
capture had 140/140 successful focus observations, with a dark dominant color
throughout. All four screenshots were inspected.

| Scene | Samples | Changes/s | p99 change gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|
| Braille, early | 1,295 | 60.03 | 27.96 | 35.05 |
| Braille, later | 1,227 | 60.04 | 28.36 | 30.02 |
| Sixel, default | 1,155 | 59.95 | 27.50 | 33.86 |
| Sixel, dense | 1,351 | 52.14 | 34.09 | 36.67 |

The older terminal's early Braille delivery deficit did not recur in this
observation. That is not a matched experiment isolating which terminal change
helped, nor proof about all profiles or text workloads. Dense Sixel still falls
short of 60 changes/s on the stable release. No CPU comparison was run for
these extra captures; capture-process CPU ranged from 0.40 to 0.56 seconds.

The owned stable app was quit after the checks. Two AppKit window-autosave keys
for its new profile were removed from the original preferences domain, and
the before/after preference comparison found no changed keys. The original
iTerm process and installed executables remained intact. Private user
preferences and the downloaded app are excluded from the evidence archive.

The [archive](live-iterm-stable-2026-09-27-macos-arm64.tar.gz) contains 108
manifested files, including traces, CPU/memory reports, captures, drivers,
the rejected atlas experiment and the version-check patch. Its SHA-256 is
`33eb907dc00c433609d3c09d23509caad44e3a71163d4e04cd5fd6441db57164`.
