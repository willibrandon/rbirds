# Output backpressure and presentation checks

These observations compare `be3bdc5` with a change to the output queue on an
M4 Pro running macOS 26.5.2 and iTerm2 3.6.6. The existing Sixel image-lifetime
fix and Kitty composed-image renderer are described in the
[earlier evidence](live-2026-09-26-macos-arm64.md).

Every partial flush previously removed the written prefix of the output vector,
copying all remaining bytes. Repeated backpressure could copy a large frame many
times before delivery. The queue now retains a write position until it empties,
compacting only when another command is appended to a partially written queue.
Its logical contents, capacity behavior, error handling and protocol bytes remain
covered by the C oracle. The oracle now compares actual drained bytes as well as
queued contents, including append and clear operations after a partial flush.

## CPU comparison

Two forward/reverse pairs per scene used a foreground 100-by-32-cell,
1400-by-1088-pixel window, seed 42, 1,200 loop ticks, a three-second warm-up and
a fifteen-second interval. No capture, profiler, local build or tests ran during
the CPU intervals. All eight samples were valid. The selected image-decoder
helper remained alive and used zero measured CPU. Medians in CPU milliseconds
per elapsed second were:

| Scene | Process | Before | After |
|---|---|---:|---:|
| Default Kitty | rbirds | 415.21 | 399.82 |
| Default Kitty | iTerm main process | 813.23 | 773.77 |
| Default Kitty | Combined | 1228.43 | 1173.59 |
| Dense Kitty | rbirds | 692.57 | 466.50 |
| Dense Kitty | iTerm main process | 1318.33 | 1296.02 |
| Dense Kitty | Combined | 2010.90 | 1762.52 |

The dense scene used 4,096 birds, speed 12, four hawks, three flocks, depth,
trails and the ember palette. Application CPU fell 32.6%, with combined CPU down
12.4%; both pairs agreed. Default combined CPU fell 4.5%, although the individual
pairs varied from about 1.4% to 7.3%. iTerm was shared with existing sessions;
these counters exclude compositor and GPU work. This does not establish the
same saving in other terminals or operating systems.

Dense median frame intervals stayed around 61 ms and flush wall time around
41 ms. Avoiding copies saves CPU but does not remove the terminal transport
limit. Whole-run application CPU per frame fell from 36.93 to 25.27 ms in that
scene; these lifetime values are separate from the equal-interval CPU rates.

## Visible observations

Separate ten-second captures used the candidate executable and the preceding
sampled-hash capture tool. All four screenshots were inspected. No capture
contained a nearly blank or bright-background sample by the retained analyzer's
thresholds.

| Renderer and scene | Samples | Changed images/s | p99 gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|
| Kitty, default | 931 | 59.66 | 29.66 | 42.74 |
| Kitty, dense | 756 | 16.20 | 72.64 | 74.63 |
| Sixel, default | 965 | 59.33 | 30.70 | 37.44 |
| Braille, default | 883 | 54.45 | 55.88 | 69.63 |

Capture CPU was 0.50 to 0.53 seconds per requested ten-second run. These are
ScreenCaptureKit observations, not physical scanout. The dense Kitty case remains
well below 60 changes/s. Braille's application trace stayed near 60 Hz, with no
interval above 25 ms, but the captured images changed less frequently.

The capture tool now compares every content pixel at the configured logical
window resolution. Its old sampled hash and color diagnostics remain available.
A headless self-test covers an off-grid single-pixel change, repeated images,
resize, title-bar exclusion and row padding. Both macOS CI jobs run this test.

Fresh default Braille captures with the complete comparison measured 54.03
changes/s before the output change and 54.82 after. Full and sampled comparisons
agreed for all 891 and 883 samples. The lower rate therefore predates this
change, and the sampling grid did not explain it in these runs. Repeated glyphs,
terminal presentation and capture coalescing have not been isolated. The p99 gaps
were 53.63 and 51.35 ms, with maxima of 63.90 and 74.57 ms. Capture CPU increased
to 0.63 and 0.69 seconds per requested ten-second run.

A separate 32-bird moving/paused check also found agreement between the two
comparisons. All 351 paused samples retained identical content after the first
sample. This validates the comparison's handling of static content; it does not
prove that future playback cannot stall.

## Validation and retained experiments

Debug and release C output-oracle tests pass, including the final drained-byte
comparison. Additional debug checks cover terminal startup, resize, cancellation
and Sixel output. Release checks cover C Kitty behavior, composed-image
cancellation, paused playback and steady-state allocations. Formatting, Clippy
and the release build pass. The measured candidate's executable hash matches
the release build. Native Windows and Linux visual playback were not measured.

A separate encoder prototype tested two, four and eight candidate matches per
hash bucket. Two candidates saved less than 1% of dense compressed bytes while
raising encoding time about 18% to 20%; larger searches cost more. All 32 outputs
decoded exactly to the eight RGBA fixtures with Python zlib. This experiment was
discarded. The prior compression archive contains the deterministic input streams
and extraction script; the new archive retains the prototype sources, timing log
and decoded-pixel hash manifest.

The [output archive](live-output-cursor-2026-09-27-macos-arm64.tar.gz) contains CPU
intervals, traces, captures, test-window screenshots, source changes, executable
hashes, drivers and local validation logs. No installed executable or terminal
preference was replaced.
