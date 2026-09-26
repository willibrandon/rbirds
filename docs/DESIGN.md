# rbirds design

Status: implementation contract; application code is not yet implemented.

## 1. Objective and reference

Produce a Rust executable named `rbirds` with the behavior of cbirds 1.4.0 at commit `cc446fc3cb80733371c62676533adcac2fc10002`. Preserve the simulation, CLI, rendering, input, recording, failure behavior, and supported platforms. Establish compatibility before optimizing or adding features.

The [compatibility contract](COMPATIBILITY.md) defines observable behavior. The [port process](PORTING.md) defines the evidence needed to accept an implementation. The pinned implementation and tests take precedence over descriptive comments or README prose when they disagree. Record those disagreements instead of silently choosing a new behavior.

The initial reference contains 7,147 lines of C implementation, including the asset generator, and 5,488 lines of C tests. Its 112 named tests are tracked in [the inventory](c-test-inventory.csv). Counts describe scope, not completeness of coverage.

## 2. Dependency and platform policy

The owner has agreed to permit Rust's standard library and system libraries. Apply that decision as follows:

| Area | Decision |
| --- | --- |
| Rust packages | One first-party Cargo package; no external runtime, build, development, optional, target-specific, Git, path, or vendored crate dependencies. |
| Standard library | `std`, `core`, and `alloc` supplied by the selected Rust toolchain are allowed. This is not a `no_std` project. |
| Native libraries | OS-provided libraries such as libc, libSystem, and libm are allowed through explicit bindings. The `libc` Cargo crate is not allowed. |
| Application implementation | Rust source, including the existing codec algorithms translated to Rust. Do not ship the C application behind a Rust entry point. |
| Runtime tools | No subprocesses for raw mode, image conversion, compression, recording, or simulation. No runtime network access. |
| Ordinary build | Rust/Cargo, the target standard library, and the platform linker/SDK. No C source compilation, code downloads, or generated bindings in the application build. |
| Validation tools | Git, C compilers, make, and host tools may build and inspect the C reference and ABI probes. They are not required to run or ordinarily build rbirds. Test code remains free of third-party Cargo dependencies. |

Start with Rust edition 2024 and pin Rust 1.96.0, the toolchain observed locally during design. This is an initial reproducibility choice, not a claim that it is the latest version. Introduce `rust-toolchain.toml` and `Cargo.lock` in phase P1. Set the initial `rust-version` to 1.96; lower it only after testing that version. Offline builds assume the toolchain and target components are already installed.

The current rbirds checkout resolves Homebrew Rust/Cargo 1.92.0 through PATH, while the installed rustup-managed tools provide 1.96.0. Use the rustup-managed tools for the pin and verify executable resolution and versions in P1. A toolchain file does not change a directly invoked Homebrew compiler. Both observations are recorded in the reference manifest.

Initial release targets match the reference's CI architecture coverage:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`

Linux musl, Windows, BSD, and 32-bit targets are outside the initial support claim. Reject unsupported target combinations explicitly rather than using an unverified Unix ABI. Establish minimum OS/libc versions from the chosen release build environments and record them before release; a target triple alone does not define that minimum.

## 3. Proposed organization

These are planned modules, not files that already exist. Keep modules small enough to test independently, without inventing a general framework.

| Rust module | C reference | Responsibility |
| --- | --- | --- |
| `main.rs`, `app.rs` | `boids.c:main` and mode entry points | Parse, select mode, coordinate resources, return explicit exit codes. |
| `options.rs` | `options.c`, option table in `boids.c` | Table-driven options, validation, help, aliases, completions. |
| `config.rs` | constants, presets, `apply_notches` | Defaults and derived settings; retain the reference's precedence rules. |
| `rng.rs` | `seed_random`, `next_random` | Exact 31-word generator and draw order. |
| `simulation/` | flocking, hawks, formations, wings, depth | Owned simulation state, deterministic stepping, population changes. |
| `spatial_grid.rs` | `spatial_grid.c` | Stable cell membership and neighbor traversal with reusable storage. |
| `image/` | `png.c`, `gif.c` | RGBA image operations, PNG/DEFLATE/checksums, GIF/palette/LZW. |
| `sprites.rs`, `font.rs` | sprite generation in `boids.c`, `font.c` | Embedded artwork, shape rasterization, rotation, tint, glyph data. |
| `render/cells.rs` | `cells.c` | Braille, sextants, blocks, changed-cell emission and pixel reconstruction. |
| `render/kitty.rs` | `kitty_graphics.c` | Protocol encoding, identifiers, uploads, placements, deletions. |
| `render/compose.rs`, `panel.rs` | composition and legend in `boids.c` | Draw order, trails, panel geometry and formatting. |
| `input.rs` | `handle_input`, mouse and Konami parsing | Persistent byte parser and ordered actions. |
| `record.rs`, `bench.rs` | recording and benchmark entry points | GIF/cast clocks, snapshots, statistics and summaries. |
| `terminal.rs`, `platform/` | termios, signals, polling, writes | Safe terminal interface over narrowly scoped OS bindings. |

Expose a library for integration tests and a thin binary. Embed the verified sprite bytes with `include_bytes!`; match the compiled C asset, not merely an assumed equivalent PNG. Compare `matrix.png` with `sprite_png.h` before choosing the canonical asset. Preserve the embedded bitmap font exactly.

Keep the upstream copyright and MIT notice with translated code and assets. Add source attribution before importing implementation or artwork.

## 4. State and execution

Use owned structs for configuration, simulation, render state, input parsing, and terminal lifecycle. Avoid process-global mutable simulation data. Each test must be able to start from a fresh state without depending on test execution order.

`Simulation` owns birds, hawks, formation state, flock statistics, animation state, and the RNG. Keep a reusable snapshot of birds. Preserve the C ordering: build the grid from the snapshot, run hawk hunting, update birds from the snapshot, repeat required speed substeps, then derive sprite headings and render. Do not replace this with in-place neighbor updates or parallel iteration.

Application time is an explicit input. Preserve the distinction between elapsed wall time, frame duration, flight time, and encoded recording time. Live execution measures a monotonic clock; tests inject durations and event timing. Recording advances its encoded clock independently of machine speed. Do not add a new fixed-step accumulator, frame-delta cap, or pause policy during the port.

Use `Vec`/slices for contiguous storage and preserve iteration order. Reuse grid, sprite, canvas, cell, and output buffers. Preallocate before steady-state drawing where practical. Population changes and resizes can allocate; failures must preserve the C recovery behavior where it is defined. Avoid unordered containers in any path that affects numerical accumulation, random draws, palette ordering, or protocol output.

Rendering produces bytes separately from writing those bytes. All renderers share a pending-output buffer that retains an unwritten suffix. Input remains serviceable during output backpressure. Do not discard, repeat, reorder, or replace a partially written frame.

## 5. Numerical fidelity

Preserve C `double` calculations as `f64`, and actual `float` storage, including trig-table entries, as `f32`. Preserve intermediate casts, truncation, rounding, signed remainder, thresholds, and expression ordering. A mathematically equivalent expression can change a trajectory.

Use explicit wrapping operations only where the C code intentionally performs unsigned wrapping, especially the RNG. Use checked arithmetic for file lengths, capacities, image dimensions, and indexes. Debug and release builds must agree on valid inputs. Do not globally disable overflow checks to make translated code pass.

Keep the RNG algorithm, warmup, seed-zero behavior, signed seed conversion, output range, and call sequence. Test internal seeds across all `u32` values represented by the C fixtures even though the public CLI permits only `0..=2147483647`.

Retain the spatial grid's cell scan order and the order of birds within each cell. Preserve the existing trig lookup, motion limits, substep formula, palette quantization, and sprite sampling. Avoid introducing explicit fused multiply-add, approximate math, SIMD, or multithreading during parity work.

Default to exact floating-point state comparisons against the canonical C build on the same target under injected identical inputs. If they differ, find the first divergent operation. If necessary, use narrowly wrapped system math calls to match the reference. Rust documents platform/toolchain variation in the precision of functions such as [`sin`](https://doc.rust-lang.org/std/primitive.f64.html#method.sin); matching the RNG does not establish cross-platform trajectory identity.

No general epsilon, perceptual similarity threshold, or rounding of trace values is an accepted substitute for this gate. A numerical exception requires a concrete reproduction, error bound, downstream effect analysis, and an explicit change to the compatibility contract. Until then, the mismatch remains open. Cross-platform claims compare each Rust target with its own pinned C control; characterize differences among C controls separately.

## 6. Terminal and unsafe boundary

Most code must be safe Rust. Confine `unsafe` to `platform/`, including any required system math wrappers. Each unsafe operation needs a stated safety invariant and a test or ABI evidence that supports it. Safe modules should prohibit unsafe code locally.

Use `unsafe extern "C"` declarations and `#[repr(C)]` for necessary native data structures. Rust's [FFI declarations](https://doc.rust-lang.org/reference/items/external-blocks.html) are unchecked contracts, and [`repr(C)`](https://doc.rust-lang.org/reference/type-layout.html#the-c-representation) does not supply the correct OS fields or constants for us. Maintain bindings per supported OS/architecture/ABI combination. Do not assume Darwin and Linux have the same `termios`, `sigaction`, `sigset_t`, errno access, flag values, or ioctl argument types.

Build C probes against the native headers in validation jobs. Compare sizes, alignment, field offsets, scalar widths, signedness, constants, and callable signatures with Rust bindings. Probes are validation artifacts, never part of the ordinary application build. Use exact C-compatible types, including variadic argument types where needed.

Represent terminal ownership as a state machine: untouched, raw mode acquired, alternate screen entered, sprites possibly uploaded, restored. Mark acquired resources immediately and unwind partial initialization in reverse order. Restoration is idempotent and restores saved attributes, cursor, mouse reporting, synchronized-update state, alternate screen, and uploaded images as appropriate.

Use explicit cleanup on normal and error paths plus a `Drop` fallback. Keep panic unwinding enabled initially and test terminal restoration after a controlled panic. Do not call `std::process::exit` while a terminal guard owns resources: it [does not run Rust destructors](https://doc.rust-lang.org/std/process/fn.exit.html). Return an exit code after cleanup. Panics must not cross native callbacks.

Signal cleanup is a separate path, not an assumption about `Drop`. Preserve the reference's handling of SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGSEGV, SIGFPE, SIGBUS, SIGABRT, and SIGPIPE. Design an audited emergency path using only target-verified async-signal-safe operations, preexisting state, and supported atomic access. It must not allocate, format strings, lock Rust I/O, or unwind. Verify restoration and the observable `128 + signal` exit convention in subprocess tests, including when output is congested. Any unavoidable difference must be explicit before acceptance. SIGKILL and SIGSTOP cannot provide cleanup guarantees.

OS I/O must handle short writes, EINTR, EAGAIN/EWOULDBLOCK, EPIPE, descriptor flag restoration, query timeouts, and disappearing terminals. Probe-response parsing and keyboard parsing retain state across read boundaries. Do not introduce an external terminal crate or invoke `stty` to avoid this work.

## 7. CLI, codecs, and errors

Use a single option description table to drive parsing, help, and shell completions. Use typed option targets rather than translating C's `void *` targets. Preserve user-facing grammar and formatting, including the C numeric parser's accepted integral floating-point forms. Use `args_os` and Unix byte access so non-UTF-8 file paths do not become an accidental regression.

Translate the existing PNG decoder/encoder, DEFLATE implementation, image transforms, GIF quantizer/encoder, and JSON string escaping. Match decoded pixels, encoded bytes where deterministic, error classes, and documented resource limits. Cross-decode fixtures with the independent C test readers; self-round-tripping the Rust encoder and decoder is insufficient.

Internally use explicit error types and `Result`; map them to the established stdout/stderr destinations, messages, and exit statuses at the boundary. Keep usage errors distinct from runtime failures. Do not use `unwrap` for external input, terminal I/O, allocation sizes, or file decoding.

Model recoverable allocation failure explicitly at the reference's recovery points, using fallible reserve operations and injected failures in tests. Rust's process-wide allocator failure behavior is not automatically equivalent to every C allocation failure; do not claim system-exhaustion parity without evidence.

## 8. Compatibility tooling and acceptance

Keep the C reference outside the shipped application. A C oracle adapter can include the unmodified `boids.c` with a renamed `main`, following the existing tests, to observe static functions and state. Rust integration tests launch isolated oracle processes and compare outputs. Any adapter patch must be limited to observation or deterministic host inputs and prove it does not alter algorithms.

The [port process](PORTING.md) specifies trace formats, test layers, platform coverage, performance budgets, and cutover requirements. No module is accepted solely because it resembles the C source or its own unit tests pass. Every module needs its mapped C tests and independent comparisons where behavior is observable.

The intentional product change is identity: repository, executable, help examples, completion command names, version identity, and cast title become `rbirds`. The intro continues to say `BOIDS`. All other behavior stays within the compatibility contract. New renderers, improved parser semantics, new presets, and algorithm changes belong after the compatibility release.
