# Deviations

D-001 through D-003 are proposed changes. D-004 and D-005 describe the accepted
Sixel and Windows additions. D-006 and D-007 describe live pacing and Kitty
sprite grouping. Test results are in [the evidence index](evidence/README.md).

The first three entries cover C behavior that is undefined, and so can't be reproduced
without reproducing memory corruption or an implementation-defined accident.
Where the reference's behavior is well defined, rbirds reproduces it, quirks
included except for the changes below (for example the benchmark's double hunt, the `c` byte that ends a
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

## D-006: live frame deadlines

The C loop sleeps for the remaining part of each frame, so wake-up delays shift
every subsequent frame. rbirds instead keeps monotonic 60 Hz deadlines and
rebases after an overrun. It blocks between frames without spinning. On macOS,
a one-shot kqueue timer requests minimal timer coalescing. No thread priority is
raised and the simulation still uses the actual elapsed frame duration.

This intentionally changes live timing under C07/C13, while retaining exact
simulation results for injected times, renderer output for identical state,
recordings and benchmark behavior. `--unlock-fps` still disables pacing.
The scheduler tests in `src/timing.rs`, ABI probe, PTY lifecycle tests and live
measurements cover the change. `RBIRDS_TRACE` is an optional developer diagnostic;
it does not add an option to the user-facing CLI. See [PERFORMANCE.md](PERFORMANCE.md).

## D-007: live Kitty sprite textures

Live Kitty playback packs rotations into vertical image strips and places their
source rectangles. The protocol's source pixels, pixel offsets and draw order
are preserved; uploads and placement bytes differ. Text, Sixel, recordings and
the C construction benchmark retain their existing formats. Explicit z values
preserve the original image ordering within the near and far layers. Repeated
border pixels preserve filtering at crop edges, and strips stay within 2048
pixels high for the supported sprite sizes.

The purpose is to reduce image lookup and texture switching in the terminal.
`render::atlas` tests compare decoded pixels and placement fields;
`tools/kitty-atlas-check.sh` checks the terminal's own decoding at sizes 4, 30
and 64, including overlapping birds, trails, depth and hawks. This changes the
live Kitty part of C12 and C15; exact C comparisons still cover the reference
protocol construction path. It does not change simulation arithmetic.
