# Isolated iTerm Kitty cache-fix qualification

iTerm 3.7.20260926-nightly did not reproduce the rapid Kitty texture accumulation
observed in 3.6.6. Two approximately three-minute runs used the same rbirds
executable as the [memory failure investigation](live-iterm-memory-2026-09-27-macos-arm64.md).
The default run's sampled terminal footprint peaked at 408.81 MiB; the dense
run peaked at 432.22 MiB. Later samples remained within the early range.
Separate foreground captures delivered approximately 60 changes/s without a
full-window placeholder or blank flash. These are finite observations of this
build and configuration. They do not qualify older iTerm releases or establish
perfect pacing under every setting.

## Build and isolation

The app came from the official [nightly archive](https://iterm2.com/downloads/nightly/)
as `iTerm2-3_7_20260926-nightly.zip`, SHA-256
`9a2d2c6ea3e6fb58d75101e871478fa45ea62411e863eba974d5614e6449c8ed`.
Its version is `3.7.20260926-nightly`; deep, strict code-signature verification
passed with publisher GEORGE NACHMAN, team H7V7XYVQ7D, matching the installed
app's publisher. This nightly follows the upstream
[texture-cache fix](https://github.com/gnachman/iTerm2/commit/50363ae0b2e243c76e4e614b26cf091a03be414d).
The comparison tests the whole nightly, not an isolated application of that
commit, so CPU differences cannot be attributed to that change alone.

The downloaded app ran from the ignored work directory with its own `-suite`,
one test profile and an owned window. Only display colors, fonts and geometry
were copied into the test profile. The first profile omitted the dark color
variants; those light-background captures are retained as diagnostics. The
reported runs used the corrected dark profile: 100×32 cells, 1400×1088 native
pixels, Monaco 12, opaque background RGB 21,25,30. UFC was off. The installed
iTerm 3.6.6 remained open and otherwise idle.

No installed executable was replaced. The test app was quit after checking
pause, controls and resize. AppKit wrote two window-autosave keys for the new
test profile into the original preferences domain despite the separate suite.
After quitting, only those two newly created keys were removed. Comparing the
original preferences before and after cleanup found no changed keys. The
private preferences backup is excluded from the evidence archive.

## Sustained observations

Each scene used seed 42, five seconds of warm-up, then six fifteen-second CPU
intervals separated by fifteen seconds. The driver stopped playback after
approximately 172 seconds. A native memory guard sampled about once per second;
an application-focus check ran about every five seconds. All 34 focus checks in
each run found the test app active. There were no builds, tests, captures or
profilers during these intervals. Guard and focus-check overhead remains part
of the test environment.

The runs include normal idle drift of flocking sliders after one minute.
Their early and later CPU rates are not fixed-configuration comparisons.

The dense scene used 4,096 birds, speed 12, four hawks, three flocks, depth,
trails and the ember palette. All twelve CPU/trace intervals were valid and
contained an unambiguous 900 or 901 completed submissions. The retained CPU
figures describe the new terminal configuration, not a matched before/after
saving. They exclude the compositor, GPU and unselected helpers.

| Scene | App CPU, ms/s | iTerm CPU, ms/s | Sampled peak footprint, MiB | Mean footprint at 5–60 s, MiB | Mean footprint at 110–172 s, MiB |
|---|---:|---:|---:|---:|---:|
| Default | 59.12 | 331.85 | 408.81 | 305.01 | 272.01 |
| Dense | 519.94 | 438.95 | 432.22 | 390.76 | 370.85 |

The app's footprint increased by approximately 0.83 MiB between the first and
last interval endpoints in each run. Optional tracing retains frame records
in memory during playback, so this instrumented run is not a measurement of
untraced application growth. Process endpoints and one-second observations do
not measure every transient allocation or GPU resource independently.

## Presentation and image correctness

Separate fifteen-second captures used the same two scenes. Every focus sample
found the owner active and the test window foremost over the captured content.
Every image sample had the dark background as its dominant color.

| Scene | Captured samples | Changes/s | p99 change gap, ms | Maximum gap, ms | Focus observations |
|---|---:|---:|---:|---:|---:|
| Default | 1,241 | 60.02 | 26.98 | 34.03 | 140/140 |
| Dense | 1,341 | 59.87 | 28.84 | 34.67 | 140/140 |

Capture-process CPU was 0.51 seconds in each run. ScreenCaptureKit observes a
logical window surface; its timing is not physical scanout. Approximately
10-Hz focus observations cannot establish uninterrupted visibility between
samples. These captures still show occasional gaps near two 60-Hz frame
periods, so they do not support a claim of flawless cadence.

A saved static stream was replayed at the original geometry and compared with
the earlier 3.6.6 screenshot. All pixels outside the panel matched. Within the
panel, 6,515 pixels differed, mostly along its rounded text border; the inset
interior contained nine differing pixels with a maximum channel difference of
one. The screenshots show a small border-position difference between terminal
versions. This is not a full-viewport pixel-identical result. The retained
comparison reports the complete difference count and bounds instead of hiding
the panel differences.

A three-second paused capture contained 204 samples and only the initial image
counted as changed. A separate nineteen-second capture exercised single-step,
panel hide/show, resize to 120×36 and resume. Its 1,190 samples retained a dark
dominant background; all 177 focus observations passed. Paused and resumed
screenshots were inspected for birds, hawks, trails and panel placement.
Transition timing is not steady-state cadence evidence.

## Capture harness correction

ScreenCaptureKit can keep receiving a window after another window covers it.
An initial dark-profile capture did exactly that when an extra test shell had
focus; it is retained separately and excluded from foreground results.
`presentation-capture.swift` now records owner activity and the foremost normal
window intersecting its content region. It ignores title-bar buttons, which
macOS 26 can represent as a separate normal window. A first implementation
mistook those buttons for an occluding window; that diagnostic is also retained.

The self-test covers detached buttons, transparent windows, non-normal layers,
real overlapping windows and an absent content window. Pixel and focus checks
pass locally. The preceding renderer commit passed all seven CI jobs. The
process CPU/memory checks in the subsequent harness commit passed on all six
native macOS, Linux and Windows targets; its full Intel Mac gate was still
running when this report was written.

The [evidence archive](live-iterm-fixed-build-2026-09-27-macos-arm64.tar.gz) retains
114 manifested files: reports, traces, screenshots, drivers, safe
test profile, signing verification, source snapshots and failed qualification
captures. The downloaded application and private user preferences are omitted.
The executable SHA-256 is
`92cc3f38cba9c0aa21b416f75f33ec5fb28177217b2f7e0f59bd33f27ffe8bd7`.
The archive SHA-256 is
`0d4e1a4e15378e0a4e92975a0b911190cb6593381366ca80b2e58833195ba471`.
