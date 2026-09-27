# Text playback and terminal warm-up

Short measurements of default Braille playback in iTerm include substantial
glyph construction. The same recorded frames become cheaper and display more
regularly on a second pass through the same window. A three-second warm-up
does not establish a stable terminal cost on this host. Startup remains part
of the user experience and should be measured separately from later playback.

These probes used the rendering code from `ec4db12`, an M4 Pro, macOS 26.5.2,
iTerm2 3.6.6 and visible 100-by-32-cell windows. The native raster geometry was
1400 by 1088 pixels. CPU samples and presentation captures ran separately;
quiet CPU runs had no capture, profiler, builds or tests. The selected iTerm
process also owned existing idle sessions. No installed executable or terminal
preference was changed.

## Isolating the text cost

A small synchronized text counter reached 59.96 visible changes/s, establishing
that a simple text stream could approach 60 Hz in this window. An additional
PTY then recorded 1,200 actual rbirds frames. All 1,020 frames after the first
three seconds changed the decoded glyph grid, with at least 378 changed glyphs
per frame. Its capture measured 54.95 visible changes/s. A native replayer
sent those same frames without simulation, composition or the forwarding PTY;
it still measured 55.17 changes/s. Its write-completion intervals had a 17.12 ms
p99 and 17.95 ms maximum. These observations do not support application
composition jitter or identical glyph grids as the explanation for that run.

A three-second intrusive iTerm profile pointed to glyph construction through
`addNonASCII`, `metalImagesForGlyphKey` and CoreGraphics. These are wall-stack
samples, not CPU percentages. The official
[glyph key](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/Metal/Infrastructure/GlyphKey.h)
includes visual column but not foreground color. The
[text renderer](https://github.com/gnachman/iTerm2/blob/v3.6.6/sources/Metal/Renderers/iTermTextRenderer.mm)
allows 65,536 cached glyph entries. Cache warming is consistent with the
measurements; capacity thrashing was not established. The glyph-key file in
v3.6.11 was byte-identical, so this investigation does not establish an upgrade
as a remedy.

Replaying the identical 20-second corpus twice in one window produced:

| Measurement | First pass | Repeated pass |
|---|---:|---:|
| iTerm CPU, ms per elapsed second | 581.91 | 450.91 |
| Visible changes/s | 54.65 | 60.08 |
| p99 visible gap, ms | 52.33 | 30.57 |
| Maximum visible gap, ms | 66.01 | 38.24 |

CPU windows lasted 15 seconds, beginning about three seconds into each pass.
Separate captures lasted ten seconds. The selected stable decoder helper used
zero measured CPU. The 22.5% terminal CPU reduction is a warm-up observation,
not a production optimization.

## Actual simulation, early and later

The harness now accepts `--sample-count` and `--sample-gap-seconds`. In one
3,600-frame default run with seed 42, it measured 15-second windows beginning
3.00 and 43.02 seconds after launch. Both finished before the 60-second idle
threshold. Settings stayed fixed while the simulation evolved naturally.

| Measurement | Early window | Later window |
|---|---:|---:|
| rbirds CPU, ms per elapsed second | 216.10 | 221.72 |
| iTerm CPU, ms per elapsed second | 571.97 | 446.12 |
| Combined CPU, ms per elapsed second | 788.08 | 667.85 |
| Visible changes/s, separate run | 56.43 | 59.88 |
| p99 visible gap, ms | 50.21 | 30.33 |
| Maximum visible gap, ms | 80.63 | 36.05 |

Both CPU intervals were valid; helper CPU was zero. Separate 15-second captures
contained 1,376 and 1,392 samples, with zero nearly blank or bright-background
samples under the retained thresholds. Capture CPU was 1.01 and 0.95 seconds.
The actual application's trace had five isolated intervals above 25 ms and
none above 50 ms. Startup cadence is not fixed by this harness change.

Repeated reports retain completed windows and report an error if the child
exits during a later gap or sample. The whole measurement then fails. The
default single-interval JSON schema is unchanged. All 15 counter/deadline/report
checks passed locally on Python 3.9.6 and 3.14.7. The preceding `ec4db12` commit
passed [all seven CI jobs](https://github.com/willibrandon/rbirds/actions/runs/36307608946);
that run predates the repeated-window feature.

## Rejected change and limits

A prototype deferred foreground changes across blank cells. Its independent
decoder matched 3.84 million normal unselected glyph/color cells and reduced
output bytes by 13.68% and SGR commands by 39.18%. Four quiet ABBA replay runs
showed only a 0.79% reduction in median iTerm CPU; visible playback remained
55.99 changes/s. Retaining blank-cell foregrounds also changes latent attributes
that selection or cursor styles might expose. No production change was retained.

The [evidence archive](live-text-warmup-2026-09-27-macos-arm64.tar.gz) includes
the recorded stream, replay source, raw captures, CPU reports, traces, owned-window
screenshots, rejected prototype, source audit and profile excerpt. It retains
a completion-file read race in the final visible driver: the application had
exited successfully, the completed report was reread, and its owned window was
closed. Future completion files are published atomically.

Captures compare all content pixels at logical window resolution; color and
blank-frame checks sample a grid. They do not measure physical scanout or decode
individual displayed frame IDs. CPU counters exclude compositor/GPU work and
unselected processes. These observations do not guarantee stall-free playback
or establish native Windows/Linux desktop behavior. Early and later windows
should remain separate when evaluating future changes.
