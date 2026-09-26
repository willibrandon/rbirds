# rbirds

Rust port of [cbirds](https://github.com/clainstone/cbirds) 1.4.0, a flocking simulation that runs in the terminal.

## Build and run

```sh
cargo build --release
./target/release/rbirds
./target/release/rbirds --hawks 2 --panel
./target/release/rbirds --render kitty    # needs Kitty or Ghostty
./target/release/rbirds --record flock.gif --seed 42
./target/release/rbirds --help
```

There are no crate dependencies. The binary links only against the system C library.

## Compatibility with cbirds

The port follows cbirds at commit [`cc446fc`](https://github.com/clainstone/cbirds/tree/cc446fc3cb80733371c62676533adcac2fc10002). With the same seed and options, rbirds gives the same simulation, the same terminal output and the same GIF and cast files as cbirds built with its default flags. The intended differences are the program name (in the help, completions, version and cast title) and the cases in [DEVIATIONS.md](docs/DEVIATIONS.md), which involve undefined behavior in the C code.

On Apple Silicon, matching the C build takes two things the source doesn't show. Apple clang fuses 92 `a*b+c` expressions into FMA instructions, and it replaces each sin/cos pair with `__sincos_stret`, which can differ from separate calls in the last bit. The port does the same on that target. See section 5 of [DESIGN.md](docs/DESIGN.md).

## Tests

The test suite builds the C reference and compares the two programs directly:

- All 112 cbirds C tests, translated to Rust.
- Scripted simulations run through both programs, comparing every floating point value.
- Recordings, PNG and GIF codec output, renderer escape sequences and CLI output, compared exactly.
- Terminal handling (raw mode, signals, resizes, blocked output) checked under a pseudoterminal.
- The GIFs and cast that cbirds publishes, made again with the commands in its `docs/README.md`. On Linux they come out exactly as published.

```sh
tools/reference.sh   # fetch the pinned cbirds source into .reference/
tools/verify.sh      # run all checks; --all-local adds x86_64 macOS and Linux (Docker)
tools/perf.sh        # compare speed and memory with the C build
tools/vhs/live.sh    # both programs through the same VHS tape, side by side
```

The comparison tests need a C compiler. Set `RBIRDS_NO_ORACLE=1` to skip them.

CI runs the same checks on Linux and macOS, on both x86_64 and arm64.

## Docs

- [DESIGN.md](docs/DESIGN.md) covers the architecture and numerical details.
- [PORTING.md](docs/PORTING.md) describes the porting process and release checks.
- [COMPATIBILITY.md](docs/COMPATIBILITY.md) lists the behavior that has to match and how it's tested.
- [docs/evidence](docs/evidence/README.md) records test runs and results.
- [DEVIATIONS.md](docs/DEVIATIONS.md) lists proposed differences from cbirds.
- [c-test-inventory.csv](docs/c-test-inventory.csv) maps each C test to its Rust version.

## License

MIT, same as cbirds. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). The simulation, bird sprites and bitmap font come from cbirds.
