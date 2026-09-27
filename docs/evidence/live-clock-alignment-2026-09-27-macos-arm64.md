# CPU windows and frame submissions on a shared clock

CPU intervals start relative to process launch; frame traces start after terminal
negotiation and sprite preparation. Equal numeric offsets therefore select
different parts of a run. The harness previously kept these scopes separate.

Trace version 2 adds a named system-clock timestamp observed after each output
flush, before sleeping. The CPU harness reads the same clock around each group
of process-counter reads. A random session identifier and child PID reject stale
traces. Complete coverage and monotonic timestamps are required. Paused ticks
do not count as submissions, and ambiguous boundary events produce count and
cost ranges. This does not measure terminal presentation or attribute work that
crosses a sampling boundary to an individual frame.

On this M4 Pro / macOS 26.5.2, twenty Rust readings each fell between the
surrounding Python readings of `CLOCK_MONOTONIC`. The same check passed with
the Rust executable running through Rosetta. Native CI now runs this check on
both architectures of macOS, Linux and Windows; those new CI results were pending
when this report was written. Windows uses QPC and its reported frequency, with
integer nanosecond conversion. Neither path assumes an `Instant` or
`perf_counter` epoch.

## Actual iTerm intervals

Three foreground runs used iTerm 3.6.6, 100×32 cells / 1400×1088 pixels and
seed 42. Each sampled two five-second CPU windows after three seconds of warm-up,
with a three-second gap. No builds, tests, capture or profiler ran during these
intervals. UFC was off. iTerm was shared with otherwise idle existing sessions.
The decoder helper remained alive. All six intervals were valid, and no
submission fell inside a counter-read boundary bracket. Those brackets ranged
from 32 to 235 microseconds.

| Scene | Window | Submitted frames | App CPU ms/frame | iTerm CPU ms/frame | Decoder CPU ms/frame | Combined CPU ms/frame |
|---|---:|---:|---:|---:|---:|---:|
| Default Kitty | 1 | 301 | 3.95 | 7.22 | 0.00 | 11.18 |
| Default Kitty | 2 | 299 | 3.92 | 6.99 | 0.00 | 10.91 |
| Default Sixel | 1 | 301 | 6.55 | 13.98 | 2.40 | 22.93 |
| Default Sixel | 2 | 301 | 6.60 | 13.25 | 2.25 | 22.10 |
| Dense Sixel | 1 | 270 | 15.01 | 23.84 | 7.55 | 46.39 |
| Dense Sixel | 2 | 266 | 16.05 | 25.13 | 8.15 | 49.32 |

The dense scene used 4,096 birds, speed 12, four hawks, three flocks, depth,
trails and ember. These are absolute observations, not before/after savings.
CPU-counter quantization, unrelated terminal work, compositor and GPU costs
remain outside the frame-count uncertainty bounds.

The first dense window selected zero-based trace entries 149–418. Reusing the
CPU window's numeric offsets against `start_us` instead selected entries 181–447,
counting 267 frames instead of 270. Even when counts coincidentally match, the
wrong origin selects a different stretch of simulation. The raw observations
and both selections are retained.

## Checks and rejected experiments

All nineteen Python checks pass, including delayed startup, paused ticks,
boundary uncertainty, missing and malformed traces, stale identities, truncated
coverage, nonmonotonic timestamps and early process exits. Native debug/release
and Rosetta release PTY checks verify that actual trace timestamps fall inside
the parent process's clock bracket. Paused playback and the existing PTY
performance harness pass with trace version 2. Formatting, Clippy and whitespace
checks pass. An initial Python assertion compared floating-point cost values
exactly; it was corrected to compare within rounding tolerance.

Two composition prototypes were rejected before this harness change. Cached
opaque runs did not improve the default/dense Sixel construction benchmarks
materially. Skipping blend arithmetic over transparent destinations preserved
the compared Kitty bytes but increased composition time from 260 to 371 µs at
defaults and from 2,450 to 3,194 µs in the dense scene. Their source patches,
drivers, measurements and artifact hashes are retained; neither is in the code.

A separate confirmation of the preceding `b804229` build retained dark dominant
backgrounds in all 5,570 captured samples across default Sixel, dense Sixel and
default Kitty. All screenshots were inspected. The observations averaged 59.80,
50.58 and 59.97 visible changes/s respectively, with maximum gaps of 37.85,
44.39 and 37.35 ms. This supports the existing yellow-flash fix in those finite
runs; dense Sixel remains below 60 Hz. It is not a physical scanout measurement.

The clock-alignment executable SHA-256 is
`5c29a0a6e94f856ad535cbfb414ec7b097804bf3a5ed9f935891a129f4253ac1`, built from
`b804229` plus the retained patch. The [archive](live-clock-alignment-2026-09-27-macos-arm64.tar.gz)
contains 62 manifested files: CPU reports, traces, checks, drivers, source changes,
rejected experiments and the separate background captures. Its SHA-256 is
`e2acec21faf30f456c79654c52cdd8e7c8ea2ca0b4cbddc3c1bc1a9c79b3c5e9`.
All owned test windows were closed; the installed executable was not replaced.
