# Live performance

The objective is low total CPU cost at a steady 60 frames per second, with
responsive controls and unchanged pictures. Measure the application and the
terminal separately. Moving work into the emulator does not make it free.
`--unlock-fps` removes pacing during animation and is a throughput diagnostic.
Paused playback continues to wait between input checks.

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

Samples with `drawn: false` are unchanged paused ticks. The report excludes
those ticks and intervals crossing them from animation timing, and uses the
summary's `drawn_frames` for CPU per submitted frame. Older traces without these
fields remain readable. `--frames` still counts loop ticks, including paused
ones, so a finite paused run still terminates.

Tracing is optional. It reserves space for 65,536 ticks, performs no log writes
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
The default injected input is `h`, the live panel toggle. Use `--panel` to start
with the panel visible, and measure that case separately from the default
command, which starts with it hidden. To pause the simulation, use
`--input-at 3 --input ' ' --trace target/paused.jsonl`. The harness checks the
trace's submitted-frame count and excludes intentional idle periods from
receive-interval distributions. `--pause-at` pauses the PTY reader instead;
it exercises output backpressure.
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

On native Windows, Linux and macOS, `tools/terminal-perf.py` records rbirds CPU
and a selected terminal process's CPU over the child's lifetime. It needs
Python 3.9 or later with no extra packages. Launch it as a terminal's direct
command, or pass `--terminal-pid` explicitly when running it from a shell:

```sh
python3 tools/terminal-perf.py --terminal-pid 12345 --output target/terminal.json --note 'terminal/version, visible, power mode, background work' -- ./target/release/rbirds --render kitty --seed 42 --frames 1800
```

From PowerShell, use `python` and `./target/release/rbirds.exe` with the same
arguments. Select the actual terminal process, not the shell; the default
parent PID is correct only when the terminal launched the harness directly.
Run the executable directly after `--`, without a shell or launcher in between.
In WSL, Linux process counters cannot measure a Windows terminal process;
that combination still needs separate host and guest tracing. Measuring
`wsl.exe` from Windows would count the launcher rather than Linux rbirds.

Use a dedicated terminal process: other tabs and windows in that process count
toward its CPU total. This measurement includes startup, excludes the compositor,
GPU and unselected helper processes, and does not establish whether the window
was visible. macOS CPU counters use the host's actual Mach timebase. Linux uses
`/proc/PID/stat` with `SC_CLK_TCK`, and Windows uses `GetProcessTimes` with a
retained process handle. Each run checks the units against Python's independent
process CPU clock. The APIs are described in the
[Linux proc documentation](https://docs.kernel.org/filesystems/proc.html) and
[Windows process timing documentation](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes).
Add `--warmup-seconds 3 --sample-seconds 30` to also sample both processes over
the same elapsed-time interval. Keep the child running longer than that window;
an early exit records a sample error. Fixed frame counts alone compare different
elapsed durations when pacing changes, which can also change the average
simulation workload. Retain lifetime and equal-time samples separately.
Do not assume that the warm-up delay reaches a stable cost. Terminal glyph
caches can still be filling, and startup performance matters to users too.
Add `--sample-count 2 --sample-gap-seconds 25` with `--warmup-seconds 3
--sample-seconds 15` to measure roughly seconds 3–18 and 43–58 in one unchanged
run. The gap starts after the preceding sample finishes. Keep the scene and
settings comparable, including any automatic simulation changes. Repeated
reports use `interval_samples` and `requested_interval_samples`; the default
single-window report retains `interval_sample`. An exit during a gap or sample
invalidates the whole measurement while preserving completed windows and the
next window's error. Compare early and later costs separately before calling
either a steady-state result.
The [iTerm text warm-up investigation](evidence/live-text-warmup-2026-09-27-macos-arm64.md)
retains an example where both CPU and visible cadence changed between windows.
Elapsed time uses Python's high-resolution `perf_counter`; reports include its
implementation and resolution. Timed waits recheck that clock until the requested
deadline, while a child exit still cancels the sample. A timeout returning early
must not shorten a requested measurement window.

Some terminals decode images in separate helpers. For example, iTerm2 uses an
`iTerm2SandboxedWorker` process for Sixel. Add `--helper-pid 12346` for each
identified, dedicated helper that remains alive throughout the sample. The
report keeps their CPU counters under `terminal_helpers`; `terminal_cpu_*`
still means the selected main terminal process alone. Include the helper sum
when calculating combined CPU. Exited or replaced processes produce errors
rather than a partial helper total. Helpers that start or restart during a run
need process-lifetime tracing; this tool does not discover or follow them.
On Windows, include the identified console host as a helper when it performs
work for the measured terminal. Process creation identities and retained
Windows handles prevent PID reuse from silently joining unrelated counters.

Reports include `measurement_valid`. An incomplete interval or a failed
terminal/helper counter invalidates the measurement and makes the harness exit
nonzero; the JSON retains the error and the child's own `status`. Accept a run
only when both the measurement is valid and the child exited successfully.
`python3 tools/test_terminal_perf.py` checks counter units, child CPU accounting,
intervals and process-exit failures. Native CI runs these checks on all six
supported OS/architecture combinations. They check measurement correctness,
not terminal appearance or performance budgets. The
[native counter validation](evidence/terminal-counters-2026-09-27.md) retains
the initial results and a live iTerm smoke run.

For a separate macOS presentation sample, give a visible test window a title
starting with `rbirds-perf-`, then run:

```sh
swiftc -O -parse-as-library tools/presentation-capture.swift -o target/presentation-capture
target/presentation-capture rbirds-perf-test target/presentation.json 12
```

This requires Screen Recording access and captures only the matching window.
It records ScreenCaptureKit timestamps and compares every captured content pixel
with the preceding image, excluding the title bar, border and row padding.
`changed` uses that comparison; `sampled_changed` and `hash` retain the earlier
eight-pixel sampling grid for comparison. Repeated images do not count as
animation. The capture uses the window's logical dimensions and records its own
CPU cost. Compile with optimization and run this separately from CPU comparisons.
The compositor also does work for capture; timestamps can be delayed or
coalesced. These samples are evidence about visible-window delivery, not physical
display scanout. `target/presentation-capture --self-test` checks pixel comparison
without screen-recording access; both macOS CI jobs run it.

The live scheduler follows monotonic deadlines and rebases after an overrun,
without catch-up bursts or busy waiting. macOS uses a one-shot kernel timer with
minimal coalescing for frame deadlines. Windows waits on console input, output
completion and cancellation events, and reuses its bounded transfer buffer.
Partial output writes advance a cursor through the queued frame. Retries do not
copy the unsent suffix; appending to a partially sent queue compacts it once.
The [output measurements](evidence/live-output-cursor-2026-09-27-macos-arm64.md)
record the CPU saving and unchanged dense-scene delivery limit in iTerm.
Live Kitty groups rotations into cropped texture placements, preserving source
pixels and stacking order while reducing terminal image lookups. In iTerm,
the live Kitty path instead composes sprites into at most two transparent
surfaces, keeping far birds below the text and near birds above it. Explicit
image IDs avoid iTerm's image-number addressing failure; the bounded placement
count avoids its repeated display-list rebuilds. Native pixels are transported
with lossless RGBA compression. Two sets of image IDs keep the displayed frame
alive while its replacement uploads; only the placement swap is synchronized.
The compressor chooses fixed or per-frame Huffman codes after accounting for
the code table's cost. This reduces transport bytes without changing pixels,
at the cost of a reusable token buffer and some additional application CPU.
Judge it by combined application and terminal CPU, alongside visible timing.
On a local macOS connection, iTerm can instead read these same RGBA pixels from
private POSIX shared-memory objects. A startup query must successfully read and
consume an object before this path is used. At most four images are outstanding;
an unread image keeps its contents, with that frame falling back to inline
compression. Remote or unsupported connections retain inline transport. The
[shared-memory observations](evidence/live-iterm-shm-2026-09-27-macos-arm64.md)
include combined CPU, dense playback, pixel integrity and cleanup checks.
Local alpha composition can differ slightly
from the terminal blending separate textures, as recorded in the
[Mac evidence](evidence/live-2026-09-26-macos-arm64.md).
Simulation arithmetic, bird count and resolution are unchanged.

The presentation capture also records the dominant sampled RGB color and its
fraction of content pixels. Inspect these alongside timing: a full-window
placeholder or blank flash is a rendering failure even if frames arrive on time.
A renderer alternating between blank and valid images can even report twice
its useful update rate. Inspect image content before interpreting changed images
as delivered simulation frames.
Optional hexadecimal colors after the duration record exact sample counts,
including colors that occupy too little of the window to be dominant. Each
frame also records its translucent sample count. For example, append
`121217 15191e` to distinguish the Sixel sky from the default background in
the tested iTerm profile. Choose colors from the actual profile being tested.
The iTerm Sixel investigation in the [Mac evidence](evidence/live-2026-09-26-macos-arm64.md)
is one example that transport measurements alone missed.

For iTerm, Sixel frames retire the previous image inside the synchronized
update and send a cell-aligned raster around the visible sprites. Opaque
full-block glyphs paint the empty sky behind that rectangle, preserving the
background even with window transparency enabled. A hidden startup cursor
probe verifies that the block character occupies one cell. Wide or unconfirmed
characters, and geometry that cannot be represented by whole cells, retain the
complete raster. Other terminals keep
the complete-raster path.

While paused, live playback keeps the last image until a key changes the scene,
a single step is requested, the window is resized or a visible autopilot slider
changes. Pointer reports alone do not repaint a paused image. Input and window
size are still checked at 60 Hz, including with `--unlock-fps`. The panel shows
zero ongoing frame cost and rate while paused, then starts fresh statistics
when playback resumes. No frame quality or simulation detail is reduced.
