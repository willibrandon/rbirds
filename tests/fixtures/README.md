# Boids test entry states

`boids-suite-linux-x64.txt` contains the 64 input states printed on stderr by
`tools/oracle/boids_suite.c` against cbirds commit
`cc446fc3cb80733371c62676533adcac2fc10002`. Captured with GCC 14.2.0 / glibc 2.41,
x86_64 Linux (Debian under WSL), using the canonical `-std=c99 -O3 -g` flags.

The C tests share globals across test functions. Windows cannot execute that
POSIX suite, so its translated tests load these recorded starting states and
then execute their original assertions using the native Rust implementation.
This is behavioral regression coverage, not a Windows C differential claim.
The Unix tests continue to capture their own target's C states at runtime;
x86_64 Linux also checks all recorded states against the fresh capture.

To regenerate on x86_64 Linux, run `tools/reference.sh` and
`cargo test --test c_boids` to build the observer, then run the resulting
`target/oracle/x86_64-linux/boids_suite` in an empty scratch directory with
`COLORTERM` unset. Save stderr to this fixture and rerun the tests. The observer
runs the C assertions before its successful exit; never accept an incomplete
capture. There must be 64 `entry`/`end` pairs.
