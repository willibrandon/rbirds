# WezTerm Sixel profiling and rejected frame acknowledgements

A profile of the current cropped renderer points to decoded-image hashing as
a major remaining WezTerm cost. A five-second sample of the owned terminal found 2,593
SHA-256 stacks among 3,225 stacks under terminal action processing. The three
call paths hash new image data when constructing a frame, looking up the image
cache and constructing the cached image. The pinned
[image data implementation](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/termwiz/src/image.rs)
and [terminal image cache](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/term/src/terminalstate/image.rs)
confirm those paths. These stack counts are not CPU percentages; sampling
also includes blocked threads.

The display thread was sampled waiting for the pane mutex. WezTerm's
[output parser](https://github.com/wez/wezterm/blob/20240203-110809-5046fc22/mux/src/lib.rs)
holds synchronized output until the closing marker, then applies the batch.
This suggested testing whether bounded frame queues could avoid processing
images that never reach a visible window update.

Three scratch builds tested primary-device-attribute acknowledgements: waiting
after each submitted frame, preparing the next frame before waiting, and
placing the request inside the synchronized update and consuming the response
asynchronously. The production source and normal release binary were unchanged.
These diagnostic builds supplied no user input and did not implement a complete
response filter or fallback policy; none is suitable for shipping.

The owned WezTerm `20240203-110809-5046fc22` window used an isolated Monaco 12
configuration, 100×32 cells and 1400×1056 pixels on macOS 26.5.2/M4 Pro. Each
CPU comparison used seed 42, 180 warmup frames and exactly 600 measured fixed
simulation frames, in baseline/candidate/candidate/baseline order. No capture,
profiling, builds or tests ran during those measurements. UFC was off.

| Experiment | Scene | Build | Combined CPU | CPU ms/submitted frame | Submissions/s |
| --- | --- | --- | ---: | ---: | ---: |
| Wait after submission | Normal | Current | 118.55% | 23.496 | 50.46 |
| Wait after submission | Normal | Candidate | 101.09% | 24.984 | 40.46 |
| Request inside update | Normal | Current | 118.22% | 24.096 | 49.07 |
| Request inside update | Normal | Candidate | 118.07% | 24.786 | 47.64 |
| Request inside update | Dense | Current | 149.24% | 61.267 | 24.36 |
| Request inside update | Dense | Candidate | 150.43% | 60.895 | 24.70 |

Values are means of two runs; 100% CPU is one core. Dense uses 4,096 birds,
speed 12, four hawks, three flocks, depth, trails and the ember palette.
CPU boundaries follow application submissions; terminal processing and display
can lag. An acknowledgement confirms model processing, not presentation or
physical scanout. GPU and system compositor work are excluded.

Separate 15-second foreground captures used ordinary elapsed-time playback.
Their trajectories differ, so they are not fixed-scene CPU comparisons.

| Experiment | Scene | Build | Visible changes/s | p99 gap | Maximum gap |
| --- | --- | --- | ---: | ---: | ---: |
| Wait after submission | Normal | Current | 32.93 | 100.07 ms | 118.69 ms |
| Wait after submission | Normal | Candidate | 34.73 | 82.89 ms | 108.76 ms |
| Prepare before waiting | Normal | Current | 32.99 | 87.86 ms | 107.29 ms |
| Prepare before waiting | Normal | Candidate | 33.91 | 74.52 ms | 116.36 ms |
| Prepare before waiting | Dense | Current | 19.64 | 123.41 ms | 127.36 ms |
| Prepare before waiting | Dense | Candidate | 17.91 | 132.57 ms | 137.06 ms |
| Request inside update | Normal | Current | 33.38 | 84.42 ms | 117.10 ms |
| Request inside update | Normal | Candidate | 32.28 | 82.89 ms | 100.28 ms |
| Request inside update | Dense | Current | 19.65 | 116.66 ms | 119.01 ms |
| Request inside update | Dense | Candidate | 19.26 | 117.19 ms | 117.96 ms |

Waiting after submission lowered CPU per second by 14.7%, with 19.8% fewer
submissions and 6.3% more CPU work per submitted frame. Preparing before waiting
still made dense presentation worse, so that variant did not proceed to a CPU
comparison. Moving the query into the update removed the meaningful CPU saving
without improving visible throughput. None of the variants removed the long
presentation gaps. They are rejected; the current renderer remains in place.

All 12 CPU reports have matching fixed inputs and valid boundaries. All 22
measured commands exited successfully and restored terminal attributes. The
11,497 captured background samples were `[17,17,22]`, and all 1,403 capture
focus observations passed. Screenshots were inspected, the owned terminal was
closed, and installed applications, preferences and `rbirds` were unchanged.
The existing iTerm flash fix is unaffected. The next investigation should
reduce decoded image work while preserving pixels, rather than add query delays.

The [archive](live-wezterm-pacing-2026-09-27-macos-arm64.tar.gz) contains 240
hashed files, including raw CPU reports, presentation traces, screenshots,
the terminal profile, scratch source and measurement drivers. Its SHA-256 is
`66012bfc8552884b14d66d1f54cc7304b15f893b931bb7586e73c5573e0e23c4`.
After extraction, `python3 wezterm-pacing/analyze.py` reproduces the summaries
using only the Python standard library. All hashes and the reproduced summary
were checked independently from a fresh extraction.
