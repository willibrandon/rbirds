# Live performance

The objective is low total CPU cost at a steady 60 frames per second, with
responsive controls and unchanged pictures. Measure the application and the
terminal separately. Moving work into the emulator does not make it free.
`--unlock-fps` explicitly removes pacing and is a throughput diagnostic.

`--bench` remains a deterministic construction benchmark and C comparison.
It does not write frames, sleep between them, handle live input or measure a
terminal. Its default renderer is Kitty; the live default is Braille. Keep
those measurements, but use live runs to judge CPU use and playback. Live Kitty
also groups sprite rotations into textures; the C construction comparison
continues to emit the reference protocol.

## Capturing the live path

Build with `cargo build --release`. In an actual terminal, run:

```sh
RBIRDS_TRACE=braille.jsonl ./target/release/rbirds --seed 42 --frames 1800
RBIRDS_TRACE=kitty.jsonl ./target/release/rbirds --render kitty --seed 42 --frames 1800
RBIRDS_TRACE=sixel.jsonl ./target/release/rbirds --render sixel --seed 42 --frames 1800
python3 tools/trace-report.py braille.jsonl kitty.jsonl sixel.jsonl
```

In PowerShell, set `$env:RBIRDS_TRACE = 'sixel.jsonl'`, run
`.\target\release\rbirds.exe --render sixel --seed 42 --frames 1800`, then
`Remove-Item Env:RBIRDS_TRACE`. The same Python report reader works on Windows.

The trace records frame starts, scheduled wake-up lateness, simulation/input,
composition, encoding, output flush, requested sleep and output bytes. Stage
times are wall time and include any preemption during that stage. Process CPU
time includes every application thread and excludes startup and sprite upload.
The CPU rate includes the intro; the report excludes the first two seconds
from frame interval and stage distributions. A value of 100 CPU ms/s means
one tenth of one logical processor, regardless of the machine's core count.
The report also counts frames whose wake-up delay plus work exceeds a 60 Hz
budget, and the longest consecutive run of those frames. For `--unlock-fps`,
that budget is a comparison target rather than the active pacing setting.
`input_bytes` counts reads at the start of frames; input serviced during a
blocked flush is not included, so it is not a complete input-latency measure.

Tracing is optional. It reserves space for 65,536 frames, performs no log writes
during playback, and writes JSON lines after a clean exit with `q` or `--frames`.
It reports omitted samples when that bound is reached. An interrupted or failed
run can leave an empty trace; the report reader rejects it. Compare traced and
untraced runs when assessing observer overhead. Traces do not measure when a
frame becomes visible on the display.

## Controlled transport

On macOS or Linux, the Python standard-library PTY harness runs the actual live
loop, answers terminal queries and saves all receive timestamps and child CPU
time. No renderer is substituted. This isolates application and transport costs
from terminal painting:

```sh
python3 tools/live-perf.py --render default --frames 1800 --output target/live.json --trace target/live.jsonl
python3 tools/live-perf.py --render sixel --frames 600 --pause-at 3 --pause-for .25 --resize-at 5 --input-at 7 --output target/events.json --trace target/events.jsonl
python3 tools/live-perf.py --render kitty --output target/busy.json -- --birds 4096 --speed 12 --hawks 4 --flocks 3 --depth --trails
```

Use `--binary` to alternate baseline and candidate executables. The report
includes the binary hash, command, geometry, scripted events and raw frame
timestamps. Its child CPU counter includes startup, unlike the live trace.
Receive timestamps can be grouped or delayed by the harness's own scheduling;
correlate them with the application trace. Intentional reader pauses are recorded
as events and must not be mistaken for spontaneous application stalls.

## Qualification

Run the default command and all five renderers at normal and large terminal
sizes. Include 800 and 4096 birds, expensive settings, resize, panel changes,
mouse movement, and sessions lasting beyond the 60-second autopilot threshold.
Use native Windows, Linux and macOS. WSL is another execution environment, not
a separate performance objective.

Record executable revision, terminal/version, OS, hardware, cell and pixel
dimensions, display refresh rate, power mode, foreground visibility and other
active work. Alternate baseline and candidate runs and retain all observations,
including slow ones. Quiet-host runs and runs with background load answer
different questions. Shared CI is useful for correctness and bounded recovery;
it is not a stable machine for absolute CPU or tail-latency gates.

Report CPU ms/s and CPU per completed frame, p50/p95/p99/p99.9 and maximum
intervals, gaps above 25/50/100 ms, consecutive missed deadlines, output bytes,
and input service latency. Set absolute budgets for each measured machine and
terminal. Accept a CPU improvement only alongside unchanged quality and steady
cadence. Preserve raw data so rare stalls remain inspectable.

For actual presentation, correlate terminal/compositor traces or timestamped
screen captures with submitted frames. Screen capture has its own cost and
must be checked separately. A PTY drain, a protocol acknowledgement or a
successful write cannot establish that a frame reached the display. Windows
Performance Analyzer can separate CPU execution, readiness and waits; Linux
`perf` and macOS sampling tools can identify active work. Account for terminal
CPU/GPU costs alongside the application.

On macOS, `tools/terminal-perf.py` records rbirds CPU and a selected terminal
process's CPU over the child's lifetime. Launch it as a terminal's direct
command, or pass `--terminal-pid` explicitly when running it from a shell:

```sh
python3 tools/terminal-perf.py --terminal-pid 12345 --output target/terminal.json --note 'terminal/version, visible, power mode, background work' -- ./target/release/rbirds --render kitty --seed 42 --frames 1800
```

Use a dedicated terminal process: other tabs and windows in that process count
toward its CPU total. This measurement includes startup, excludes the compositor
and GPU, and does not establish whether the window was visible. Its macOS CPU
counters are converted from Mach ticks with the host's actual timebase.
Add `--warmup-seconds 3 --sample-seconds 30` to also sample both processes over
the same elapsed-time interval. Keep the child running longer than that window;
an early exit records a sample error. Fixed frame counts alone compare different
elapsed durations when pacing changes, which can also change the average
simulation workload. Retain lifetime and equal-time samples separately.

For a separate macOS presentation sample, give a visible test window a title
starting with `rbirds-perf-`, then run:

```sh
swiftc -O -parse-as-library tools/presentation-capture.swift -o target/presentation-capture
target/presentation-capture rbirds-perf-test target/presentation.json 12
```

This requires Screen Recording access and captures only the matching window.
It records ScreenCaptureKit timestamps and whether a sampled content hash
changed, plus the capture process's own CPU cost. Repeated images do not count
as animation. Compile with optimization and run this separately from CPU
comparisons. The compositor also does work for capture; timestamps can be
delayed or coalesced, and sampled hashes can miss small changes. These samples
are evidence about visible-window delivery, not physical display scanout.

The live scheduler follows monotonic deadlines and rebases after an overrun,
without catch-up bursts or busy waiting. macOS uses a one-shot kernel timer with
minimal coalescing for frame deadlines. Windows waits on console input, output
completion and cancellation events, and reuses its bounded transfer buffer.
Live Kitty groups rotations into cropped texture placements, preserving source
pixels and stacking order while reducing terminal image lookups. Rendering
optimizations retain the same pixels, cell output and simulation arithmetic. Bird count, resolution and quality are never reduced automatically.

The presentation capture also records the dominant sampled RGB color and its
fraction of content pixels. Inspect these alongside timing: a full-window
placeholder or blank flash is a rendering failure even if frames arrive on time.
The iTerm Sixel investigation in the [Mac evidence](evidence/live-2026-09-26-macos-arm64.md)
is one example that transport measurements alone missed.
