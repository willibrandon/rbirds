# iTerm Kitty memory growth and process memory measurements

The composed Kitty path is not qualified for sustained use in iTerm 3.6.6.
A bounded run of `f4167e4` at default Kitty settings increased iTerm's physical
footprint from approximately 694 MiB before window creation to 1,126 MiB after
five seconds and 2.1 GiB after ten seconds. The driver stopped at its two-GiB
guard. The earlier short CPU and presentation comparisons did not detect this
failure. Their measured CPU rates remain observations of those short runs, not
evidence of an acceptable long-running renderer.

## Cause and upstream status

iTerm's [image controller](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/KittyImageController.swift#L37)
assigns a fresh UUID to every transmitted image. Its
[Metal renderer](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/KittyImageRenderer.swift#L142)
caches textures by that UUID without pruning old entries. Alternating four
protocol IDs does not bound this separate cache. The 3.6.11 source retains the
same behavior; that version was inspected, not run here.

An [upstream change committed September 18](https://github.com/gnachman/iTerm2/commit/50363ae0b2e243c76e4e614b26cf091a03be414d)
prunes textures whose images are absent from the current frame. A build containing
that change has not yet been qualified here. No installed terminal was upgraded
or modified. The PR remains a draft pending a working, bounded-memory path.
The Sixel yellow-placeholder fix addresses a different image-lifetime failure.

The investigation began with a ten-second intrusive sample of a one-bird Kitty
run. Drawing and frame setup dominated the sampled work. For example, texture
creation accounted for 230 samples under one drawing stack, while placement
deletion accounted for 15 samples across its two call sites. Frame-completion
histogram work was also visible. These counts include a shared terminal process
and sampling overhead; they are not CPU percentages or savings. No deletion
or sparse-texture prototype was adopted.

## Independent memory observations

The host was an M4 Pro running macOS 26.5.2 with iTerm 3.6.6. Each owned foreground
window used 100×32 cells / 1400×1088 pixels and seed 42. UFC was off, and existing
iTerm sessions were otherwise idle. No builds, tests, screen capture or profiler
ran during the native-counter measurements. A separate native footprint guard
sampled once per second. These runs qualify memory behavior, not CPU savings.

The short Kitty run used two 1.5-second intervals separated by one second, after
1.5 seconds of warm-up. The Sixel control used two ten-second intervals separated
by ten seconds, after three seconds of warm-up. All four CPU/trace intervals
were valid, with unambiguous submission counts of 90, 91, 600 and 600.

| Backend | Interval | App footprint change, MiB | iTerm footprint change, MiB |
|---|---:|---:|---:|
| Kitty | 1 | 0.000 | +150.547 |
| Kitty | 2 | 0.000 | +274.532 |
| Sixel | 1 | +0.047 | +156.438 |
| Sixel | 2 | +0.047 | +67.625 |

Endpoint growth alone does not distinguish warm-up from sustained accumulation.
The one-second observations show Kitty rising from 927 to 1,668 MiB during
playback, then falling to about 710 MiB several seconds after its window closed.
Sixel fluctuated between approximately 1,001 and 1,218 MiB after startup across
the roughly 35-second window, then fell to about 686 MiB after closing. Its
decoder helper grew in the first measured interval and shrank in the second.
That finite control does not prove bounded memory for arbitrary durations or
settings, but it did not reproduce Kitty's rapid continuing growth.

## Harness changes and checks

`terminal-perf.py` now retains memory gauges at the endpoints of each CPU
interval, alongside the existing process identity and liveness checks. macOS
supplies resident bytes and physical footprint; Linux supplies resident pages
converted with the actual page size; Windows supplies working set and private
commit. Each quantity keeps its own before, after and signed change. They are
not interchangeable metrics, and summing them would double-count memory.
The lifetime report includes terminal/helper endpoints; application memory is
sampled while the child is alive. No endpoint report claims to measure peaks.

All 21 checks pass under native and Rosetta Python, including a real child that
commits 32 MiB of random data, page-size conversion, declining memory gauges,
process replacement, exited helpers and complete report fields. The allocation
check uses a lower bound because Python's temporary allocation and allocator
retention can make the observed increase exceed the final buffer size.
The six native CI targets run these checks; results for this change were pending
when written.

The first Rosetta run exposed a pre-existing CPU conversion error. Translated
`mach_timebase_info` returned 1:1 nanosecond ticks, while `proc_pid_rusage` still
returned the kernel's 24-MHz ticks. The independent CPU-clock check rejected
the readings. The harness now obtains the kernel frequency through
[`hw.tbfrequency`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_mib.c).
Both native and translated checks then passed. The live measurements above
used native Python before that conversion change; its 125:3 Mach ratio gives
the same conversion as the 24-MHz hardware frequency. No Rosetta measurements
with the wrong scale were accepted.

The executable SHA-256 is
`92cc3f38cba9c0aa21b416f75f33ec5fb28177217b2f7e0f59bd33f27ffe8bd7`.
The [archive](live-iterm-memory-2026-09-27-macos-arm64.tar.gz) contains 40
manifested files: native memory observations, vmmap output, CPU reports and
traces, the intrusive sample, drivers, source snapshots, the upstream change,
the harness patch and both successful and failed checks. Its SHA-256 is
`b7e0a25dfe6f491e1dd2896b0b515101866232a6003a414c10de693452e70085`.
All owned windows were closed; installed executables and user preferences were
unchanged.
