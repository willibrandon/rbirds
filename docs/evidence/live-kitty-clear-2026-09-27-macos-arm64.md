# Clearing retained Kitty canvases

The composed Kitty renderer previously cleared both full RGBA surfaces before
every frame. It now remembers each surface's painted bounds and clears one
contiguous span containing those bounds. Pixels outside that span are already
transparent. Hidden surfaces retain their bounds until reused; a resized surface
starts empty. Bounds are recorded before fallible encoding or transport work.

On this M4 Pro / macOS 26.5.2, the change consistently reduced application CPU
for a one-bird scene. It did not establish a combined application-and-terminal
CPU saving, or a repeatable saving at the default or dense settings. The image
replacement protocol is unchanged, including the existing iTerm flicker fixes.

## Foreground iTerm measurements

All sixteen intervals used iTerm 3.6.6, a foreground owned window of 100×32
cells / 1400×1088 pixels, seed 42, three seconds of warm-up and fifteen seconds
of CPU sampling. No capture, profiler, build or tests ran during these intervals.
UFC was off. Existing iTerm sessions were otherwise idle; its decoder helper
remained alive and used no measured CPU for these Kitty runs. The negotiated
local shared-memory transport was active.

Default and dense scenes used the order baseline, per-row prototype, contiguous
span, contiguous span, per-row prototype, baseline. The one-bird scene used
baseline, contiguous span, contiguous span, baseline. All intervals passed the
shared-clock trace checks, with no ambiguous boundary submission counts. Each
contained 899–901 submissions. Values below are medians of two observations in
CPU milliseconds per elapsed second; 1,000 represents one fully occupied core.

| Scene | App before | App after | iTerm before | iTerm after | Combined before | Combined after |
|---|---:|---:|---:|---:|---:|---:|
| One bird | 17.522 | 10.589 | 443.505 | 451.443 | 461.026 | 462.032 |
| Defaults | 244.997 | 236.661 | 419.052 | 418.669 | 664.049 | 655.330 |
| Dense | 526.392 | 526.575 | 480.948 | 477.779 | 1007.340 | 1004.354 |

The one-bird application's two baseline observations were 17.420 and 17.624;
the changed build measured 10.282 and 10.895. That is about 40% less application
CPU, but only about seven CPU milliseconds per elapsed second in absolute terms.
iTerm dominated the total, and its variation obscured that small saving.

Default application observations ranged from 234.879 to 255.115 before and
216.482 to 256.841 after. The apparent median improvement reversed with run
order, so it is not a repeatable saving. Dense application CPU was effectively
unchanged. The dense scene used 4,096 birds, speed 12, four hawks, three flocks,
depth, trails and ember. These runs do not establish results for other systems.

## Composition probes and correctness

A continuous helper explicitly enables the composed Kitty renderer in
`LiveLoop`; ordinary `--bench --render kitty` does not exercise it. It advances
600 frames at deterministic virtual 60 Hz and measures the last 420. The
accepted variant's helper settles the shipped fallback palette before sprite
preparation. Its median composition time was 257.594→249.856 µs for the default
configuration and 2,436.169→2,433.220 µs for the dense scene. This uses inline
output and no terminal, so it does not predict live CPU or visible delivery.

The rejected prototype cleared each clipped row separately. Its probe measured
254.673→269.120 µs for the default configuration and 2,426.683→2,449.700 µs for
the dense scene. That earlier helper left theme colors unlearned, so its default
sprites do not represent either the fallback palette or a real terminal theme.
Its actual iTerm comparisons used the learned terminal theme, but showed no
compelling overall advantage. Both variants and their measurements are retained.

An independent validation driver streamed all 600 frames from the baseline and
accepted builds. Complete output matched in both scenes, including each frame's
eight-byte length prefix:

| Scene | Bytes per stream | SHA-256, identical before and after |
|---|---:|---|
| Fallback palette | 96,869,627 | `72ceabd01397109428ebbfa9eee7b01db5de7b06cbb7a968d83128cf7e3e0707` |
| Dense | 520,956,474 | `abdb3f8c25d23dff071e2988f7ce2d5a57693c45828016fd62abd2b0c7797bb4` |

The new regression test compares retained surfaces against newly allocated
surfaces over forty frames, including clipping at every edge, translucent
sprites, empty scenes, panel toggles, hidden surfaces and four viewport sizes.
It checks complete active canvases and all output bytes. The five composition
tests passed in native debug/release and Rosetta release. Four allocation checks,
four Kitty PTY checks, formatting and Clippy also passed.

## Separate visible-window checks

Three ScreenCaptureKit runs inspected the accepted build, separately from CPU
comparisons. Every captured sample had the dark dominant color `[21,25,30]`;
all three screenshots were inspected without finding residual images or a
yellow background. Default and dense captures each lasted fifteen seconds.

| Scene | Samples | Visible changes/s | p99 gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|
| Defaults | 1,349 | 59.91 | 28.69 | 35.38 |
| Dense | 1,385 | 59.92 | 25.87 | 31.00 |

A twenty-second control run supplied another 1,736 samples while toggling the
panel three times and resizing from 100×32 to 80×24 and back. It used five birds,
four hawks, three flocks, depth and trails. Intentional scene changes make this
unsuitable for steady-state cadence claims. The sparse scene also makes a
near-blank color threshold insufficient to assess missing birds; the canvas
comparison and inspected screenshots provide additional correctness checks.
Capture itself used 1.06–1.28 CPU seconds per run, and compositor work is not
included in that figure. These finite logical-window observations do not measure
physical scanout or guarantee the absence of future stalls.

The baseline is `132d820d7101d48ebb6b4923a664e512b804d02a`, executable SHA-256
`5c29a0a6e94f856ad535cbfb414ec7b097804bf3a5ed9f935891a129f4253ac1`.
The accepted executable, built with the retained patch, has SHA-256
`92cc3f38cba9c0aa21b416f75f33ec5fb28177217b2f7e0f59bd33f27ffe8bd7`.
The [archive](live-kitty-clear-2026-09-27-macos-arm64.tar.gz) contains 104
manifested files: CPU reports and traces, capture data and screenshots, drivers,
patches, checks and artifact hashes. Its SHA-256 is
`2d85f992689608dfbb65dc5bdee59d4c90e3dbfb4edf98fc1a4bbef7c44a9f5c`.
All owned windows were closed. The installed executable was not replaced.
