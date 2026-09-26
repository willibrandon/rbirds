# Deviations

D-001 through D-003 are proposed changes. D-004 and D-005 describe the accepted
Sixel and Windows additions. D-006 is big-flock mode, which changes nothing unless
`--big-flock` is given, and D-007 is how the frame delay sleeps. Test results are
in [the evidence index](evidence/README.md).

The first three entries cover C behavior that is undefined, and so can't be reproduced
without reproducing memory corruption or an implementation-defined accident.
Where the reference's behavior is well defined, rbirds reproduces it, quirks
included (for example the benchmark's double hunt, the `c` byte that ends a
theme query, and the panel columns padded by bytes).

Reference for every entry: cbirds 1.4.0, commit
`cc446fc3cb80733371c62676533adcac2fc10002`, all targets.

## D-001: a flock grown and quit within one blocked write

- Input: a live run whose output is blocked (a stalled terminal, for example).
  During the wait, `+` (or `=`) arrives, then `q` before the next frame, and
  `--snapshot` is given.
- Reference behavior: `handle_input` raises `config.birds` at once, but the
  arrays are only resized at the top of the next frame. The `q` in the same wait
  ends the loop first, so `write_snapshot` calls `compose_onto`, which reads
  `config.birds` elements of arrays that hold fewer. That read is out of bounds
  (undefined behavior).
- rbirds behavior: composition and placement draw
  `min(config.birds, allocated)` birds (`render::compose::drawn`), so the
  snapshot shows the birds that exist.
- First divergence: the first `birds[i]` read past the allocation.
- Reason: memory corruption is never reproduced (docs/COMPATIBILITY.md §6).
- Affected requirements: C05, C15, C16.
- Regression test: `tests/deviations.rs::d001_a_count_above_the_arrays_draws_the_birds_that_exist`
  (composition and Kitty placements with the count above the arrays).
- Claim affected: none for defined inputs.

## D-002: integer overflow in cursor and paint arithmetic

- Input: cell grids or cell sizes near `INT_MAX` (for example `cells_paint`
  with `cols * cell_width` over `INT_MAX`). The application can't produce these,
  since its grids are at most 400×120 cells of images at most 16 KiB pixels
  wide. Only direct calls into the translated module can.
- Reference behavior: signed overflow (undefined), which usually wraps.
- rbirds behavior: cursor positions use explicit wrapping arithmetic, and the
  oracle tests confirm the result matches the C build. A painting too large to
  size returns `CellsError::Memory`. A text buffer whose doubling would wrap
  returns `Memory` instead of looping forever.
- Reason: sizes use safe, checked arithmetic (docs/DESIGN.md §5).
- Affected requirements: C11.
- Regression tests: `tests/cells_oracle.rs`, `tests/c_cells.rs`.
- Claim affected: none reachable from the CLI.

## D-003: option help lines past the C buffer

- Input: a table row whose long name is over 88 bytes and has a metavar. The
  cbirds table has none; only a synthetic test table reaches this.
- Reference behavior: `options_usage` overruns its line buffer (undefined).
- rbirds behavior: the line is cut at the buffer's 95 bytes.
- Affected requirements: C01 (the generic parser only).
- Regression test: `tests/options_oracle.rs` (a synthetic table; cases that
  don't overrun are compared exactly).
- Claim affected: none for the cbirds option table.

## D-004: explicit Sixel renderer

- Reference: cbirds 1.4.0 at the pinned commit above; all supported targets.
- Input: `--render sixel`, help, completions, or an invalid renderer choice.
- Reference behavior: Sixel is not a choice and no Sixel output is emitted.
- rbirds behavior / first divergence: the option parser accepts a fifth renderer;
  known help/choice strings include it. Live startup negotiates Sixel capability
  and cell dimensions; full frames use a fixed 256-color palette and repaint the
  picture background. Existing renderer defaults remain intact.
- Reason: provide pixel graphics in Windows Terminal and compatible emulators.
- Affected requirements: C01, C08, C12 through C17.
- Regression tests: `tests/sixel.rs`, `tests/pty_sixel.rs`,
  `tests/portable_cli.rs`, and the narrowly extended CLI differential corpus.
- Decision: accepted, including the help and completion changes.
- Claim affected: new Sixel output has no C equivalent; intentional CLI strings
  are normalized separately from unchanged Unix behavior.

## D-005: native Windows console

- Reference: cbirds 1.4.0 at the pinned commit above; Windows MSVC targets.
- Input: native Windows build, live console interaction, Unicode file arguments.
- Reference behavior: POSIX terminal/ABI dependencies prevent a native build.
- rbirds behavior / first divergence: platform selection chooses Win32 console
  records, window dimensions, a bounded writer and Ctrl-event handling. Normal
  exit and unwinding restore saved modes/code pages; Ctrl+C/Break return 130.
- Reason: run directly on Windows without WSL or application crate dependencies.
- Affected requirements: C01, C13 through C18; Windows math/CRT results are not asserted
  identical to a Unix C build. Invalid UTF-16 arguments are usage errors.
- Regression tests: native hidden-console tests in `src/platform/windows_tests.rs`,
  portable CLI/Unicode recording tests, and translated portable tests using the
  recorded C input fixture where a POSIX suite cannot run.
- Decision: accepted. Windows runs directly through the native console APIs.
- Claim affected: Windows has its own tests. C comparisons, Unix ABI checks and
  POSIX signal tests run on Unix. Windows ARM64 and appearance in real terminals
  still need testing. Forced termination may prevent terminal cleanup.

## D-006: big-flock mode

- Reference: cbirds 1.4.0 at the pinned commit above; all supported targets.
- Input: `--big-flock COUNT`, help, completions, and a live session started
  with it.
- Reference behavior: `--birds` stops at 4096, `+` stops there too, and there
  is no `--big-flock`. One thread flies and draws the flock, and frames are
  written as soon as they are built.
- rbirds behavior / first divergence:
  - `--big-flock COUNT` takes 1 to 65536 birds and overrides `--birds`
    wherever either appears. `--birds` keeps its range and its messages, so
    `--birds 5000` is still refused. `+` grows the flock to 65536.
  - A frame uses half the cores the system reports
    (`std::thread::available_parallelism`, at least one) for the neighbour
    search and the rest of each bird's step, composition, reading the canvas
    into cells, and Sixel encoding. Wing beats run afterwards on one thread
    in bird order, because a glide is the only random draw in a step. Each
    parallel step writes each bird, band of pixel rows or Sixel band from
    inputs no other thread writes, so the result doesn't depend on the
    thread count. For N up to 4096, `--big-flock N` flies and draws exactly
    the flock `--birds N` does.
  - At startup the terminal is sent a device status request (`\e[5n`). If it
    answers (`\e[0n`) within 250 ms, every frame is followed by the same
    request, and the next frame is built while the terminal reads but not
    written until the answer is in. The request is counted before the frame
    is written, so an answer read while it is written still counts. The
    terminal is then never more than a frame behind, however much a frame
    holds. Answers are taken out of the input before keys are read from it,
    so they don't count as keys or reset the idle clock, and no other byte
    is dropped. Keys read while a frame waits are acted on at once, as keys
    read while output is blocked are, and a q then leaves at once as it does
    there; however much is typed, the frame still waits. While an answer is due, putting the terminal back first
    reads it (for at most 250 ms, on the normal and the signal path alike),
    so it doesn't reach the shell. A terminal that doesn't answer at startup
    gets its frames as in cbirds. One that stops answering holds up one
    frame for a second, and after that its frames go out as in cbirds.
  - `--bench` prints a `threads` line after `render`.
  - Threads are started for each parallel step, and starting a thread
    allocates. In this mode a steady-state frame therefore allocates, which
    docs/PORTING.md §7 otherwise rules out. Buffers are still reused from
    frame to frame.
- Reason: real starling murmurations run to tens of thousands of birds, and
  past a few thousand one core can't fly and draw them 60 times a second.
  Using every core and writing frames as fast as they are built made the
  flock stop in flight: in VHS's terminal it stood still for up to 709 ms at
  65536 birds, because the terminal fell behind and was short of CPU to
  catch up. With half the cores and the pacing there were no stills
  ([evidence](evidence/README.md#live-pacing)).
- Affected requirements: C01 (option table, help, completions), C05
  (population past 4096), C13 (the startup request), C14 (the `+` limit,
  answers in the input), C15 (reading an outstanding answer before
  restoring), C17 (bench report and steady-state allocation).
- Regression tests: `tests/big_flock.rs` (6000 birds in every renderer and a
  flock grown past 4096 with `+`, each on 1, 2, 5 and 12 threads; 65536 birds
  on 1, 3 and 12; answers taken out of the keys, and counted when read while
  their frame is written; `--birds` and `--big-flock`
  giving the same bench frames, GIFs and cast; limits and messages), the
  pacing cases in `tests/pty_rbirds.rs` (a request after every frame, frames
  no faster than a slow terminal answers, even while input pours in, a key
  after a burst of keys, no stall, a terminal that doesn't answer or stops
  answering, a signal while an answer is due), the threaded runs in
  `tests/sim_oracle.rs` (every scenario on 2 and 5 threads against the C),
  `tests/sixel.rs::every_thread_count_writes_the_same_image`, the
  neighbour-gathering and Sixel band cases in `tests/allocation_failure.rs`,
  and the extended CLI normalization in `tests/cli_differential.rs`.
- Decision: accepted, including the help and completion changes.
- Claim affected: none without `--big-flock`. With it, up to 4096 birds the
  frames are cbirds' (checked against the C on several threads). Past 4096
  there is no C to compare with, so each thread count is compared with one
  thread instead.

## D-007: the frame delay ends on time

- Reference: cbirds 1.4.0 at the pinned commit above; seen on macOS.
- Input: any live run.
- Reference behavior: after a frame, `nanosleep` for the rest of the
  sixtieth of a second. macOS lets a sleep end up to about a third of its
  length late, so a 14 ms sleep ends 5 to 7 ms late. cbirds draws a frame
  every 18 to 21 ms on the M4 Pro (49 to 55 a second across runs, measured),
  and the faster a frame is built, the longer the sleep and the later the
  next frame.
- rbirds behavior / first divergence: the same deadline, slept towards a
  millisecond at a time (`live::sleep_until`), which ends within half a
  millisecond of it. rbirds draws a frame every 17 ms (59 a second). The
  frames and the time each one flies are unchanged.
- Reason: rbirds builds a frame two to three times faster than cbirds, so
  one long sleep made its frames later than cbirds', 49 a second against 52
  in the same run. Sleeping to the deadline means a faster frame can never make the next
  one late. On Linux, where sleeps end on time, nothing changes.
- Affected requirements: C07 (time), C13 (live frames).
- Regression tests: `tests/big_flock.rs::the_frame_delay_ends_on_time`,
  `tests/pty_rbirds.rs::the_default_flock_is_not_paced_and_never_stalls`.
- Decision: accepted.
- Claim affected: none. Live frame times are measured, not compared exactly
  (docs/COMPATIBILITY.md §1).
