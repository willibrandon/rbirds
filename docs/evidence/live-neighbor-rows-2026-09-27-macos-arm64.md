# Visiting neighboring cells as row ranges

The spatial grid uses counting sort: adjacent cells already share contiguous
item storage. The flock update previously entered a separate loop for every
cell, including empty cells. It now visits one item range per selected row.
Cell order, item order, neighbor tests and every floating-point operation are
unchanged. This removes repeated range lookups without adding a cache,
allocation, thread or approximation.

The starting point is `b45522e`, which includes the corrected iTerm Sixel
placement. All observations use an M4 Pro, macOS 26.5.2 and iTerm2 3.6.6.

## Construction comparison

Each case used seed 42, the Kitty construction benchmark at 1600×800 pixels,
one warm-up per executable and before/after/after/before order. Values are
medians of two runs per executable, in milliseconds per frame. The first two
cases used 1,500 frames; the others used 400. Output bytes per frame matched
within each case. These timings exclude terminal painting.

| Flock configuration | Before | After | Reduction |
|---|---:|---:|---:|
| Default | 0.3065 | 0.2325 | 24.1% |
| Default with depth | 0.3075 | 0.2400 | 22.0% |
| 4,096 birds | 2.6725 | 1.9035 | 28.8% |
| 4,096 birds with depth | 2.8460 | 2.0800 | 26.9% |
| Dense scene with trails, depth, hawks and flocks | 9.5415 | 6.8425 | 28.3% |

The dense scene also uses speed 12, four hawks, three flocks and the ember
palette. The live comparisons below use the same demanding settings.

## Live CPU and delivery

Each scene used four visible 100×32-cell, 1400×1088-pixel runs in
before/after/after/before order. CPU intervals lasted fifteen seconds after
three seconds of warm-up. Default text also measured seconds 43–58 in the same
run. No capture, profiler, builds or tests ran during CPU sampling. All twenty
intervals were valid. Values below are medians of two runs per executable,
in CPU milliseconds per elapsed second.

| Scene/window | App before → after | iTerm before → after | Decoder before → after | Combined before → after |
|---|---:|---:|---:|---:|
| Default text, early | 209.92 → 190.78 | 581.50 → 580.14 | 0 → 0 | 791.42 → 770.93 |
| Default text, later | 213.58 → 207.24 | 444.69 → 441.19 | 0 → 0 | 658.27 → 648.42 |
| Default Kitty | 226.59 → 236.36 | 422.36 → 425.17 | 0 → 0 | 648.95 → 661.54 |
| Default Sixel | 397.42 → 392.45 | 819.44 → 798.90 | 140.99 → 135.65 | 1357.85 → 1327.01 |
| Dense Sixel | 848.14 → 834.56 | 1180.73 → 1307.50 | 377.54 → 431.43 | 2406.41 → 2573.50 |

Default text application CPU fell 9.1% early and 3.0% later. Both early pairs
improved; in the later window one pair improved and the other changed by only
+0.3%. Default Sixel's median combined reduction was 2.3%, but its first pair
increased and the reverse pair decreased. Those variations remain in the raw
observations; this does not establish a saving in every run.

Kitty's first comparison increased combined CPU by 1.9%. Its traces showed
faster updates but higher composition and transport preparation time in one
candidate run. A separate four-run repeat reduced combined CPU by 1.4%
(664.42 → 655.32 ms/s); all four intervals were valid. Across all eight runs,
median application CPU was 231.37 → 233.27 ms/s and median combined CPU was
650.38 → 658.45 ms/s. The evidence establishes no total Kitty CPU reduction.
Every run retained the negotiated shared-image transport.

Dense Sixel's mean update time in trace seconds 3–18 fell from about 9.14 to
6.37 ms. Application delivery in those trace windows rose from 45.03 to
52.37 frames/s. Its CPU sample windows therefore include more image processing:
application CPU fell 1.6%, while combined CPU rose 6.9%. This is a throughput
improvement with a measured total-CPU tradeoff. Trace times begin after terminal
startup, so they are not precisely aligned with the harness's launch-relative
CPU windows; no exact combined CPU-per-frame claim is made from their ratio.

iTerm's main process was shared with existing idle sessions. The selected
decoder helper remained alive. Compositor, GPU and unselected processes are
excluded, and native Windows/Linux desktop performance was not measured.

## Visible observations

Separate fifteen-second captures compared dense Sixel and checked the text
candidate early and later in one run. The screenshots were inspected.

| Scene | Samples | Visible changes/s | p99 gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|
| Dense Sixel, before | 1,416 | 44.99 | 33.88 | 37.91 |
| Dense Sixel, after | 1,429 | 50.78 | 33.90 | 36.83 |
| Default text, early | 1,273 | 55.74 | 50.74 | 71.85 |
| Default text, later | 1,268 | 60.01 | 29.47 | 37.79 |

Every Sixel sample retained the dark `[18,18,23]` dominant background. The text
captures retained their normal terminal background. Capture CPU was 1.11,
1.08, 0.98 and 0.97 seconds respectively. These finite observations preserve
the flicker fix and show a dense-scene cadence gain, but do not achieve 60 Hz
dense playback or remove text startup stalls.

## Verification

Seventeen comparisons with the C simulation, three spatial-grid checks and
four floating-point checks passed in native debug/release and Rosetta release.
These include recorded and live trajectories, renderers, seeds, speed extremes,
perception slider values, trails, layers, hawks, flocks and long transitions.
The four allocation tests passed, including text, Kitty and Sixel frames.
Clippy passed with all targets and features.

## Rejected experiments

A fresh three-second profile of the corrected dense Sixel executable found
596 active leaf samples in flock updates, 332 in Sixel encoding and 322 in
composition. The main terminal continued to spend substantial time converting
and copying image pixels; its decoder helper spent 480 leaf samples in Sixel
decoding. These intrusive samples identify work, not CPU percentages or savings.

Two encoder prototypes classified equal eight-pixel blocks together. Replacing
the special empty-sky path slowed default construction by 20.0%, sparse
construction by 65.7% and the dense case by 3.4%. A version retaining the sky
special case still slowed them by 18.3%, 59.2% and 4.2%. Both were removed.

Moving the depth-layer rejection ahead of distance arithmetic passed the C
and floating-point checks but did not improve construction. Default time was
essentially unchanged, the dense depth case regressed 1.7%, and the busy case
regressed 2.8%. That change was also removed. Only the row-range traversal is
part of the final implementation.

## Retained artifacts

The [archive](live-neighbor-rows-2026-09-27-macos-arm64.tar.gz) contains raw CPU
reports, traces, capture samples, screenshots, construction results, launch and
analysis scripts, the source patch, diagnostic profiles and rejected variants.
The 137 included files were verified against its SHA-256 manifest. The archive
is 3,211,072 bytes with SHA-256
`a09156cc5b4fb87607db1163eb0291f0b925a2084dda5ac93bc0521594109516`.

Executable SHA-256 values are
`7c0f772ddf9f2a5266b3f5aff8fa87e044f1dcab140563b47c4eee97acfea328`
before and
`b45f29d6df3e26eca9aae7b990d83f7b7a3c324290e0fe0b6af2f4826dc3679d`
after. All owned test windows were closed after successful exits.
