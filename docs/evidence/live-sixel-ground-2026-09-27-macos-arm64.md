# Sixel background plane comparison

Sixel now paints each six-row band's background as one solid run, then paints
the foreground colors over it. Previously the background plane encoded cutouts
around every bird. Skipping those cutouts reduces plane construction and output
without changing the final quantized pixels. The last band paints only its
remaining rows. Palette selection, alpha flattening, simulation and resolution
are unchanged. The synchronized erase and opaque glyph backdrop that prevent
iTerm's whole-window placeholder flash remain intact.

The baseline is commit `623a82e`. Its executable SHA-256 is
`9222e21aa4b6e250acb76ee857486c3c035bbf859cfa2183d4595316fa800f7b`;
the candidate is
`fcad3c2f10e8ef06f71781178b1d1bdd0cdd02c2e2fe17ea5929d6e249d550f8`.
Only the Sixel encoder changed between these executables.

## Pixel and codec checks

An independent decoder found identical output for 17,408 generated whole-image
and crop cases, spanning every alpha value, narrow widths, partial bands and
changing image sizes. Fifteen deterministic simulation images also matched.
The corpus samples frames 60, 180, 300, 450 and 600 at 1400×1088, seed 42,
using whole-cell crops. Its default-parameter scene uses the headless black
foreground; the live runs negotiate the terminal foreground normally.

The codec-only timing uses the same saved images for both encoders, 500
encodes per interval, in before/after/after/before order. It excludes simulation,
transport and terminal work. These small-corpus timings are not live CPU rates.

| Scene | Before, ms/encode | After, ms/encode | Encoded bytes change |
| --- | ---: | ---: | ---: |
| Default parameters | 0.5493 | 0.4932 | −25.08% |
| Dense | 2.8290 | 2.7423 | −9.41% |
| 32 birds | 0.0910 | 0.0689 | −21.96% |

Two real iTerm static comparisons used the full viewport, with default
parameters and the dense scene. Each before/after pair had zero differing RGBA
pixels across 1,523,200 pixels. All four screenshots were inspected.

## Live CPU

The host was the M4 Pro on macOS 26.5.2. The separately downloaded iTerm 3.7.3
ran in an isolated preferences suite with one owned 100×32 / 1400×1088 window.
UFC was off. There were no captures, profilers, builds or tests during CPU
measurement. Each scene ran in before/after/after/before order, with a
three-second warm-up and fifteen-second interval, then quit after about
21 seconds. Every run therefore ended before the idle autopilot starts.

The dense scene uses 4,096 birds, speed 12, four hawks, three flocks, depth,
trails and ember. The sparse scene uses 32 birds; other settings are defaults.
All runs use seed 42. Counters include rbirds, the terminal's main process and
its separate Sixel decoder helper. The compositor, GPU and other processes
are excluded. The table shows the mean of two intervals per executable;
raw intervals are retained, including variability in terminal cost.

| Scene | App before → after, CPU ms/s | iTerm before → after, CPU ms/s | Decoder before → after, CPU ms/s | Combined before → after, CPU ms/s |
| --- | ---: | ---: | ---: | ---: |
| Default | 123.71 → 113.45 | 785.63 → 777.24 | 142.58 → 127.19 | 1051.92 → 1017.88 |
| Dense | 857.76 → 861.56 | 1333.91 → 1335.86 | 465.29 → 435.16 | 2656.96 → 2632.58 |
| 32 birds | 26.34 → 22.76 | 499.46 → 466.59 | 44.27 → 37.00 | 570.06 → 526.35 |

Default and sparse submissions stayed near 60 Hz. Dense submissions increased
from 54.81 to 56.17 Hz, while combined CPU per submitted frame fell from
48.48 to 46.87 ms. That higher throughput slightly increases dense application
CPU per second. These are submissions, not displayed frames. All twelve
intervals have valid counters and unambiguous submission counts.

Combined mean CPU fell 3.24%, 0.92% and 7.67%, respectively. The small default
and dense aggregate differences should not be treated as precise universal
savings: there are only two samples per side, and terminal timing varies.
The application savings at fixed throughput and lower decoder cost support
the change; they do not establish a large overall improvement.

## Separate presentation checks

Each foreground capture lasted fifteen seconds, starting about three seconds
into a fresh run. Every sample retained the dark `[18,18,23]` dominant
background. All 559 focus observations found the owned window active and
above other normal windows in its content region. All four screenshots were
inspected; no whole-window yellow or blank sample appeared.

| Scene | Samples | Changes/s | p99 change gap, ms | Maximum gap, ms |
| --- | ---: | ---: | ---: | ---: |
| Default before | 1,369 | 59.96 | 27.03 | 33.86 |
| Default after | 1,300 | 60.03 | 28.21 | 33.85 |
| Dense before | 1,382 | 50.80 | 35.06 | 42.51 |
| Dense after | 1,384 | 52.05 | 34.70 | 43.56 |

Capture CPU was 0.46–0.54 seconds per run. Window-surface sampling is not
physical scanout and does not prove zero stutters. Dense Sixel still falls
short of 60 visible changes/s. No Windows or Linux GUI performance claim is
made from these Mac measurements.

Native debug, native release and Rosetta release checks pass the Sixel raster,
Sixel PTY, Kitty PTY, shared-image lifecycle and steady-state allocation suites
(30 tests per configuration). Formatting and Clippy with warnings denied pass.
CI for the baseline exposed one shared-image terminal double still answering
as iTerm 3.6.6; its supported-path fixture now answers 3.7.3. Tests for rejecting
older Kitty versions and for supporting old-version Sixel remain intact.

The owned app was quit and its new window-autosave keys removed. The original
preferences comparison found no changes; the user's installed iTerm and
rbirds executables were not replaced. The accompanying archive contains the
drivers, source patch, independent decoder, traces, CPU reports, captures and
validation logs, with a SHA-256 manifest. Private preferences, downloaded apps
and executables are excluded.

The [archive](live-sixel-ground-2026-09-27-macos-arm64.tar.gz) contains 137
manifested files (5,349,918 compressed bytes). Its SHA-256 is
`52d99ca3db8d9c4ecf91c413e37ebd408985520fe6810c9396c998ebb9ad6213`.
