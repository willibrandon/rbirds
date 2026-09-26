# rbirds

Rust port of [cbirds](https://github.com/clainstone/cbirds) 1.4.0, a flocking simulation that runs in the terminal.

## Build and run

```sh
cargo build --release
./target/release/rbirds
./target/release/rbirds --hawks 2 --panel
./target/release/rbirds --render kitty    # needs Kitty or Ghostty
./target/release/rbirds --render sixel    # needs Sixel and a cell-size query reply
./target/release/rbirds --record flock.gif --seed 42
./target/release/rbirds --help
```

On Windows, install the Rust MSVC toolchain and Visual Studio C++ build tools
(including the Windows SDK), then run from PowerShell in Windows Terminal:

```powershell
cargo build --release
.\target\release\rbirds.exe --render sixel --panel
.\target\release\rbirds.exe --render braille
.\target\release\rbirds.exe --record flock.gif --seed 42
```

Use **`--render sixel`** for pixel graphics in **Windows Terminal 1.22 or newer**.
Windows Terminal does not implement Kitty graphics; `--render kitty` is for
emulators that do. Braille remains the default on every platform. `blocks` and
`sextants` are also available; sextants need a font containing those glyphs.
Sixel support is negotiated when requested, and an unsupported terminal produces
an error with guidance. See [Windows and Sixel](docs/WINDOWS.md) for details.

There are no crate dependencies. Native bindings use system libraries: libc /
libSystem on Unix and Win32 / the Microsoft C runtime on Windows. Builds use
the Rust version pinned in `rust-toolchain.toml`; normal builds need no C source
compilation or reference checkout.

## Compatibility with cbirds

The port follows cbirds at commit [`cc446fc`](https://github.com/clainstone/cbirds/tree/cc446fc3cb80733371c62676533adcac2fc10002). On the documented Linux and macOS targets, the compatibility suite compares simulation, terminal output, GIF and cast files with cbirds built using its canonical flags. Differences include product identity, the explicit Sixel renderer and native Windows support, and the undefined-C-behavior cases in [DEVIATIONS.md](docs/DEVIATIONS.md). Windows has native behavioral tests; byte-identical output across different operating systems' math libraries is not promised.

On Apple Silicon, matching the C build takes two things the source doesn't show. Apple clang fuses 92 `a*b+c` expressions into FMA instructions, and it replaces each sin/cos pair with `__sincos_stret`, which can differ from separate calls in the last bit. The port does the same on that target. See section 5 of [DESIGN.md](docs/DESIGN.md).

## Tests

On Linux and macOS, the test suite builds the C reference and compares the two programs directly:

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

The Unix comparison tests need a C compiler. Set `RBIRDS_NO_ORACLE=1` to allow
skipping unavailable C comparisons; such a run is not compatibility evidence.

Windows has native console tests for input, resize, Unicode snapshots, Ctrl+Break,
panic recovery, output backpressure and cancellation, plus the portable codec,
renderer, parser and boids tests. Sixel output is independently decoded pixel by
pixel; CLI and Unicode recording tests run on both platforms. POSIX-specific
tests are compiled only on Unix.

```powershell
# PowerShell 7+, native Windows checks (no C reference required)
.\tools\verify.ps1
.\tools\perf.ps1                         # includes Sixel; native measurements
.\tools\reference.ps1                    # optional pinned C source verification
.\tools\verify.ps1 -AllLocal -Distribution Debian # also runs Unix checks in WSL
.\tools\perf.ps1 -CompareReference        # C comparison in WSL
.\tools\unix.ps1 vhs                     # Unix VHS workflow through WSL
.\tools\unix.ps1 linux -TaskArguments linux/amd64 # Docker workflow through WSL
```

The WSL workflows require the pinned Rust toolchain, C compiler and script-specific
tools inside that distribution. `unix.ps1` defaults to Debian and keeps WSL Cargo
build files separate from the Windows build. See [development commands](docs/WINDOWS.md#development).

CI is configured for native Windows x86_64 and Linux/macOS on both x86_64 and arm64.
See the [test results](docs/evidence/windows-sixel.md) for what has run locally.

## Docs

- [DESIGN.md](docs/DESIGN.md) covers the architecture and numerical details.
- [PORTING.md](docs/PORTING.md) describes the porting process and release checks.
- [COMPATIBILITY.md](docs/COMPATIBILITY.md) lists the behavior that has to match and how it's tested.
- [WINDOWS.md](docs/WINDOWS.md) covers Windows setup, Sixel, PowerShell workflows and validation limits.
- [docs/evidence](docs/evidence/README.md) records test runs and results.
- [DEVIATIONS.md](docs/DEVIATIONS.md) lists differences from cbirds.
- [c-test-inventory.csv](docs/c-test-inventory.csv) maps each C test to its Rust version.

## License

MIT, same as cbirds. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). The simulation, bird sprites and bitmap font come from cbirds.
