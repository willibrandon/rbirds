# Native terminal CPU counters

At `005637ee8aaff3f8a6bdef20c204e922a9b5af4c`, the nine measurement checks passed
on native Windows x64 and ARM64, Linux x64 and ARM64, and macOS Intel and ARM64
in [CI run 36302167747](https://github.com/willibrandon/rbirds/actions/runs/36302167747).
This records the counter-check steps; the longer Unix application gates were
still running when the status snapshot was saved.

The checks compare native counters with Python's process CPU clock, compare a
child's independently reported CPU with its lifetime total, and exercise equal
elapsed-time intervals and selected helpers. Forced helper and terminal exits
must leave invalid reports rather than zero CPU totals. Incomplete intervals
fail the harness while preserving the child's own exit status. Parser checks
cover Linux process names containing whitespace and parentheses, and ensure
descendant CPU fields are not mistaken for the process's own CPU. Changed
creation identities and backwards counters are rejected.

Both Windows runners recorded 0.15625 seconds through the native counter and
the reference clock in the units check. Separate local runs passed on macOS
ARM64 and Linux ARM64 in the existing Debian Docker image. The latter validates
Linux process accounting in a container; it is not Linux desktop evidence.

A foreground iTerm 3.6.6 smoke run used the unchanged release executable from
the adaptive compression change, 100 by 32 cells at 1400 by 1088 native pixels,
seed 42, Kitty rendering and 600 frames. The portable harness recorded a valid
five-second interval after a one-second warm-up, including the selected image
decoder helper. The app trace contained no intervals above 25 ms after warm-up;
the maximum was 17.84 ms. There was no simultaneous capture, build or local
test. This is a compatibility check for the harness, not a CPU improvement or
presentation-rate comparison. Only the test's own terminal window was closed.

The [raw archive](terminal-counters-2026-09-27.tar.gz) contains local test logs,
the two Windows counter-check log excerpts, native CI step states, the live CPU
report and trace, source hashes and executable hash. It excludes executables
and unrelated windows. Reproduce the correctness checks with
`python3 tools/test_terminal_perf.py`, or `python` on Windows. Desktop appearance,
compositor/GPU costs and shared-machine performance remain separate checks.

A later [Windows x64 run](https://github.com/willibrandon/rbirds/actions/runs/36307146574/job/108585908049)
passed every application gate but exposed an intermittent harness timing error:
a requested 200 ms interval was reported as 187 ms. The harness had trusted one
timed wait and used `monotonic` for elapsed time. That left short samples exposed
to early timeouts and coarse clock steps. Python's
[performance counter](https://docs.python.org/3/library/time.html#time.perf_counter)
is intended for short elapsed-time measurements; older Windows Python versions
use [different implementations](https://github.com/python/cpython/blob/v3.12.10/Python/pytime.c)
for `monotonic` and `perf_counter`.

The harness now measures elapsed time with `perf_counter` and rechecks its
deadline after a timeout. It continues to cancel immediately when the child
exits, and records the clock implementation, resolution and Python version.
The original 200 ms minimum assertion remains. Three deterministic checks
cover early timeouts, an exit during the remaining wait and an already-stopped
child. All twelve checks pass locally on macOS with system Python 3.9 and
Homebrew Python 3.14. Native CI validates the same code on Windows and Linux.
