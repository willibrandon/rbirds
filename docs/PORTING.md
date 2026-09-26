# Port process and release gates

Status: P0–P5 complete, with evidence on all four targets, natively in CI; P6 partly complete; P7 pending. See [the phase status](#phase-status) and [the evidence index](evidence/README.md).

Read [the design](DESIGN.md) and [the compatibility contract](COMPATIBILITY.md) first. Work one phase to its exit gate before treating downstream behavior as accepted. Small implementation changes may overlap in a branch, but an unmet prerequisite remains visible.

## 1. What this process guarantees

It establishes an auditable release rule: **do not label or release rbirds as a faithful port until all declared acceptance gates pass against the pinned C reference on all declared targets**. It cannot prove equality for every input or future environment. The evidence must state exactly what was compared, what passed, and any accepted limitation.

No silent fixture regeneration, skipped required tests, broad output normalization, or loosened numerical tolerances is permitted to make a failure disappear. A discovered difference is work to finish, not permission to reduce the requirement. If the owner accepts a changed requirement, record the actual change and qualify the resulting claim.

## 2. Establish the reference without changing cbirds

The reference commit is `cc446fc3cb80733371c62676533adcac2fc10002`. Keep the existing `/Users/brandon/src/cbirds` checkout untouched. From the rbirds repository, the following commands can create an isolated local copy once:

```sh
mkdir -p .reference
git clone --no-hardlinks --no-checkout ../cbirds .reference/cbirds
git -C .reference/cbirds checkout --detach cc446fc3cb80733371c62676533adcac2fc10002
git -C .reference/cbirds rev-parse HEAD
git -C .reference/cbirds status --porcelain --untracked-files=no
make -C .reference/cbirds CC=cc CFLAGS="-O3 -g"
make -C .reference/cbirds CC=cc test
.reference/cbirds/cbirds --version
```

If the sibling checkout is unavailable, clone `https://github.com/clainstone/cbirds.git` instead and check out the same commit. Do not substitute the newest tag if that commit cannot be fetched. Add `.reference/`, build output, and temporary evidence to `.gitignore` when introducing the harness.

The status output must be empty, the commit must match, and tracked files must match the manifest before building. Build flags are part of provenance. Use a fresh reference build directory per compiler/flag set; make does not automatically rebuild because `CFLAGS` changed. Never run `make clean` in the user's original checkout to prepare an experiment.

Canonical runtime controls use the normal C optimization level (`-O3 -g`, without fast-math). Use Apple Clang on Darwin and GCC on GNU/Linux, recording exact versions. Run Clang on Linux as an additional control. Record differences between C controls separately; do not let a change of compiler quietly change expected Rust results.

## 3. Evidence and bookkeeping

Introduce these artifacts during P0/P1. They are planned paths, not currently executable tooling:

| Artifact | Contents |
| --- | --- |
| `tests/fixtures/` | Small immutable C-derived CLI, codec, protocol, RNG, state and event fixtures, with provenance |
| `tools/oracle/` | Minimal C observation adapters and native ABI probes, built separately from the Rust application |
| `tests/compatibility.rs` | Rust integration harness comparing the reference, fixtures and Rust library/binary |
| `tests/terminal.rs` | Scripted PTY peer and failure/lifecycle cases |
| `tools/verify.sh` | One documented command running all local gates and reporting unavailable platform/manual gates |
| `docs/evidence/` | Accepted concise reports, target matrix, deviations and artifact digests |

The Rust harness and maintenance code use only standard/system libraries. A harness may launch the C reference; the shipped application must not. Test setup must not require network downloads once the reference and toolchains are present.

Each case has a stable ID, requirement IDs, source test/function, arguments, input bytes, seed, terminal dimensions/pixel size, theme replies, relevant environment, injected clock/event schedule, output files, comparison class, and timeout. Each run records:

- C commit/tree, fixture digest and adapter version; fail on an unexpected reference change.
- Rust commit plus dirty diff digest if applicable; Rust/Cargo versions, target, C compiler and linker/SDK/libc/OS versions, flags and profile.
- Environment including locale, TERM, COLORTERM, dimensions and reply script; deterministic cases must not depend on the developer's terminal theme.
- Commands, exit status, terminating signal where applicable, stdout/stderr, hashes of files, durations, pass/fail/skip status, and the first divergent field/frame.

Use unique temporary directories and bounded subprocess deadlines. A failed process must be reaped and its PTY closed. Preserve failed inputs and diagnostics; clean successful scratch output. For long recordings, keep small checked-in cases and retain large evidence as hashed release/CI artifacts.

Do not trust hashes alone when a mismatch occurs. Decode and report the first differing state field, pixel, protocol command, CLI byte or recorded event. The actual and expected files must remain available for diagnosis.

## 4. Required test layers

### A. Translate every C test

[The inventory](c-test-inventory.csv) contains all 112 `test_*()` functions invoked by the seven C suite entry points. It deliberately excludes helper functions merely named like tests. Keep the C file and test name as the stable key; map to one or more actual Rust test paths and requirement IDs.

Preserve each test's assertions, fixtures, boundary cases and reference tolerances. Rust API changes can make C null-pointer tests inapplicable as API calls, but their underlying invalid-input/resource guarantees still need an explicit mapping or applicability explanation. Do not count an empty test as a port.

The inventory validator must rediscover the pinned C entry-point calls and fail for missing, duplicate, extra, unmapped, ignored or unexecuted tests. Counts alone are insufficient. Platform-conditional sections, such as glibc RNG comparisons, retain their conditional coverage and must run on an applicable target. Run Rust tests in debug and release profiles to expose wrapping and conversion errors.

### B. Independent C/Rust comparisons

Run each oracle scenario in a fresh process to isolate C globals. An adapter may include `boids.c` with a renamed main, just as the original tests do. It must observe the original functions; it must not contain a second rewritten flocking implementation.

For simulation comparisons, serialize fields explicitly in a versioned, deterministic format. Represent floats as fixed-width hexadecimal bit patterns (`f32` as 8 hex digits, `f64` as 16); serialize integers in a declared width/base and arrays in index order. Never dump native structs with padding. Include:

- All RNG words, indices and draw count.
- Effective configuration, clock values, formation/autopilot/input state.
- Bird positions, directions, flock/layer, wings/glide/trails and sprite selection.
- Hawk state, selected prey and chase timers.
- Grid counts, offsets, indices and flock statistics.

Compare at initialization and each named frame/substep boundary. Expand instrumentation around the first mismatch rather than starting with a permissive end-of-run distance check. Inject time and input identically in both implementations. Compare uninstrumented C output with instrumented output on representative scenarios to ensure instrumentation has not changed behavior.

Cover every renderer, palette, preset, shape and mode at least once. Add targeted combinations: multiple flocks × avoidance extremes; max speed × tiny viewport × panel; hawks × depth × trails; custom sprite × text renderer; matrix × explicit palette/trails overrides; pause × step × resize; recording × conflicting renderer/panel flags. Generate a reproducible pairwise configuration set as additional coverage, not a replacement for those named interactions.

Use seeds `0, 1, 2, 5, 33, 42, 2147483647` for public scenarios and the C fixtures' wider unsigned seeds for the RNG itself. Include 1, 800 and 4096 birds; 1–3 flocks; 0–4 hawks; speed/avoidance endpoints; renderer size limits; normal, tiny and panel-threshold viewports. Exhaust all slider notches and accepted recording FPS values in focused tests.

For time, cover zero duration, regular 60 Hz, a faster clock, delayed frames, speed substeps, and both sides of intro, autopilot and outro thresholds. Run seeded behavioral traces beyond 60 seconds of simulated time so autopilot is exercised. Keep numerical operation order exact; any altered schedule is a different case.

For CLI comparisons, use a full cross-process argument corpus including special-option ordering, hidden aliases, negated flags, boundary values, numeric syntax, invalid bytes, nonexistent paths, write failures, and mode precedence. Compare exit code and both output streams.

For codecs, require C-encode/Rust-decode and Rust-encode/C-decode checks in addition to same-language round trips. The GIF test reader is useful independent evidence. Keep exact compressed-byte checks for deterministic encoders; visual equivalence alone does not satisfy that gate. Exercise truncation at every byte of small fixtures, malformed length/checksum/Huffman data, oversized decoded sizes and invalid chunk order. Fixed-seed mutation tests must be reproducible and bounded in memory/time.

### C. Scripted terminal and failure tests

Implement a PTY controller with explicit size, terminal replies and timed input. Use readiness/output markers rather than arbitrary sleeps wherever possible. Capture the raw byte stream and before/after terminal attributes. Keep deterministic parser/clock tests separate from live scheduling tests so a noisy scheduler cannot invalidate numerical comparisons.

Required cases include:

- Raw-mode acquisition, failed setup, successful/absent/malformed/fragmented theme replies and query deadlines.
- Default braille and explicit Kitty, every input control, split escape sequences and mouse reports, Konami input, intro interruption, pause/step, reset and population changes.
- Zero pixel dimensions, tiny windows, panel size thresholds, repeated resize, panel toggling and recovery after resize.
- Slow output, short writes, EINTR, EAGAIN, closed reader/EPIPE, stdin end-of-file and terminal loss; record the C result before defining Rust expectations.
- Quit during normal output and blocked output; do not assume both C paths run the outro identically.
- Each handled signal, SIGPIPE behavior, and injected failures after each terminal acquisition stage and sprite-upload stage.
- Controlled Rust panic after raw mode, confirming restoration; fatal-signal cases run only in isolated subprocesses with bounded teardown.
- Read/decode/open/write/close failure for custom sprites, recordings and snapshots, plus injected recoverable allocation failures.

After every exit, compare the termios fields and descriptor flags that were changed, cursor/mouse/alternate-screen/synchronized-update cleanup, and image cleanup protocol. Do not byte-compare uninitialized structure padding. Check exit code versus signal termination explicitly. A timeout or missing cleanup is a failure, not a skipped result.

A PTY does not render Kitty graphics or establish visual fidelity. Run the real-terminal checks in P6 as well.

### D. Dependency, platform and resource checks

Audit the full Cargo graph and manifest on every target, including test/build/optional dependencies and all features. Check that only the first-party package appears and that it has no dependency edges. [`cargo metadata`](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html) supplies the package and resolved graph; `--no-deps` alone is not a full graph audit. Offline mode is additional evidence, not proof that cached dependencies were absent.

Once P1 introduces Cargo, the normal checks will include:

```sh
cargo fmt --all -- --check
cargo check --all-targets --all-features --locked --offline
cargo test --all-targets --all-features --locked --offline
cargo test --release --all-targets --all-features --locked --offline
cargo clippy --all-targets --all-features --locked --offline -- -D warnings
cargo build --release --locked --offline
cargo metadata --format-version 1 --all-features --locked --offline
```

These commands are not runnable in this documentation-only repository yet. The completed verification script must also invoke oracle comparisons, ABI probes and PTY cases; Cargo unit tests alone do not complete the process.

Check a fresh Cargo home with the pinned toolchain available to show no registry/Git crate cache is required. Inspect dynamic linkage with native platform tools and reject unexpected non-system libraries. Ordinary build and `cargo install --path . --locked --offline` must succeed without building or locating the C reference. Install tests use a temporary prefix; uninstall must leave unrelated files intact.

## 5. Implementation phases

| Phase | Work | Required exit evidence |
| --- | --- | --- |
| P0: freeze and characterize | Verify manifest, isolate C source, run all C suites, capture baseline corpus, implement minimal oracle adapters, establish native target/compiler matrix and test inventory validator | Every C test runs; fixtures have reproducible provenance; adapters are observational; no missing reference assets; baseline failures are classified |
| P1: Rust foundation and platform proof | Add Cargo/toolchain/lockfile, attribution, typed options/config/RNG/grid, source asset verification; prototype FFI and ABI probes early | Empty dependency graph, offline build, exact parser/help/completion/RNG/grid comparisons, native ABI checks on all four targets; inventory rows for completed modules mapped; file/mode CLI behavior remains pending P2/P4 |
| P2: codecs and pixels | Port image storage, checksums, DEFLATE, PNG transforms/encoding/decoding, GIF quantization/LZW, font and sprite generation | All codec C tests mapped and passing; malformed-input checks; cross-decoding; exact sprite/pixel/encoded-byte fixtures |
| P3: simulation and controls | Port owned state, formations, birds/hawks, flock metrics, speed/time, wings/depth/trails, presets, input and autopilot | Exact controlled traces; boids behavior tests covering these modules; long simulated-time cases; no numerical exceptions left unclassified |
| P4: renderers and headless modes | Port cells, Kitty commands, composition/panel, GIF/cast recording, snapshots' composition and bench | Exact cell/protocol outputs, complete headless scenario corpus, recording frame/delay/metadata matches, no-TTY execution |
| P5: live terminal integration | Complete lifecycle/signals, input/resize/theme, polling/backpressure, snapshot file handling, normal/error exits | ABI checks and PTY/failure cases pass; debug/release results agree; all 112 C test mappings executed; no terminal state leak |
| P6: release qualification | Run complete matrix, real terminals, independent playback, performance budgets, installation and dependency audit | All C01–C18 requirements have evidence on applicable targets; accepted report has zero unexpected differences and zero skipped required gates |
| P7: cutover | Make Rust the normal documented executable/build, preserve reference retrieval and regression harness, assign release identity and final provenance | Fresh checkout builds/runs offline with installed tools; release checklist below complete; source and assets attributed; reference remains available for future regressions |

### Phase status

| Phase | Status | Evidence |
| --- | --- | --- |
| P0 | complete | `tools/reference.sh` verifies all 43 pinned files; the C suites pass on macOS arm64 and Linux arm64/x86_64; oracles only observe (`tools/oracle/`); `tests/inventory.rs` rediscovers the 112 C tests |
| P1 | complete | empty dependency graph; `tests/options_oracle.rs`, `tests/cli_differential.rs`; RNG and grid exact in `tests/sim_oracle.rs`; `tests/abi.rs` identical on all four targets |
| P2 | complete | `tests/c_png.rs`, `tests/c_gif.rs`, `tests/png_oracle.rs`, `tests/gif_oracle.rs`, sprite pipeline exact |
| P3 | complete | `tests/sim_oracle.rs` matches the C exactly, including 68 s recordings and a 4,700-frame live session; the two numerical findings classified and reproduced (DESIGN §5) |
| P4 | complete | `tests/cells_oracle.rs`, `tests/kitty_oracle.rs`, `tests/headless_differential.rs` (every rate; GIFs and casts match the C output) |
| P5 | complete | `tests/pty_reference.rs`, `tests/pty_rbirds.rs`, `tests/allocation_failure.rs`; debug and release agree; all 112 mappings executed |
| P6 | partly complete | done: native CI on the four targets, the translated and emulated runs, installation and dependency audit, performance budgets (`tools/perf.sh`, macOS arm64), cbirds' published recordings made again exactly on Linux (`tests/published_media.rs`), a headless terminal run (`tools/vhs/live.sh`), independent playback. Open: Kitty graphics in real Kitty and Ghostty windows, macOS Terminal and tmux |
| P7 | pending | needs the owner's release identity and decisions on [the deviations](DEVIATIONS.md) |

Each phase should consist of reviewable module-sized changes with its tests and updated mappings. Keep algorithm changes out of translation changes. Do not remove C reference access after the Rust executable first animates successfully.

When an apparent reference bug is found, minimize it, add a characterization case, and follow the deviation procedure in the compatibility contract. Investigating a difficult mismatch does not itself justify skipping it or changing the reference.

## 6. Native CI and terminal matrix

All four target triples in the design require native execution of the Rust tests, canonical C comparisons, ABI probes and PTY tests. Cross-compilation is useful additional coverage but does not replace executing them. Add the reference's Linux Clang control and C ASan/UBSan jobs; sanitizer-clean C does not establish Rust FFI safety, so ABI and lifecycle checks remain required.

Initially mirror the reference's environments: Ubuntu 24.04 x86_64 and arm64, macOS 15 arm64 and Intel. Verify runner availability when implementing CI and record any replacement environment and its OS minimum implications. Do not mark an unavailable platform green. Bind release evidence to exact environment/toolchain versions even when CI images receive updates.

Before compatibility release, record terminal application/version, OS, font, dimensions and test results for:

- Kitty and Ghostty: sprites plus all three text renderers, input, resize, panel, quit and signal recovery. Cover both Linux and macOS across the matrix, and both sprite terminals on each OS family.
- macOS Terminal and a Linux terminal without Kitty sprite support: default braille and blocks; sextants with a font that contains the glyphs.
- A tmux session: text rendering and cleanup only, matching the reference's support scope.
- An independent GIF viewer and asciinema-compatible player: duration, looping, opening/closing state, palettes and representative frames. These are validation tools, not application dependencies.

Compare C and Rust under the same terminal settings. Capture representative frames and record the outcome. Check default, hawks, multiple flocks, depth/trails, matrix, custom sprite, text shapes, panel toggling and resizing. A report that merely says “looks good” without configuration is incomplete.

## 7. Performance and responsiveness budget

Performance parity is a separate release gate. The following are initial project acceptance budgets, not measurements or promises already achieved:

| Measurement | Gate |
| --- | --- |
| Median steady-state frame construction time | Rust no more than 1.15× C on each canonical workload |
| Median startup/sprite construction time | Rust no more than 1.20× C |
| Peak resident memory | Rust no more than `max(1.25 × C, C + 8 MiB)` |
| Deterministic bytes per frame and encoded fixture size | Exact match, apart from the declared identity fields |
| Steady-state allocation | No new per-frame allocation in the simulation/grid/output reuse paths; document allocations that already occur in C |
| Input/quit service under controlled backpressure | Match the C state transitions and satisfy the PTY watchdog; target first input service within 250 ms on a normally scheduled host |

For performance, build C with canonical flags and Rust with `--release`. Use the same host, CPU/power conditions, seed, renderer, size, configuration and frame count. Run two warmups and at least ten interleaved C/Rust samples, saving all observations. If run-to-run variability exceeds 5%, investigate or repeat on a quieter host; a noisy run cannot justify relaxing a budget.

Benchmark 800 and 4096 birds across all renderers, with a default case and a speed-12/hawks-4/flocks-3/depth/trails case. Measure startup separately because `--bench` does not represent all live startup costs. Record median, spread, peak memory, output bytes and binary size; use additional instrumentation for allocation counts and tail latency. Do not claim faster live rendering solely from `--bench`, which constructs frames without terminal output.

The backpressure controller should expose read/write readiness and log event service; keep scheduler stalls distinguishable from application stalls. If C itself misses the proposed responsiveness budget, capture that evidence and resolve the target before acceptance. Do not bake an unsupported timing guarantee into release notes.

## 8. Failure triage and fixture updates

1. Reproduce with the pinned C/Rust revisions, recorded target, seed and host inputs.
2. Find the first mismatch: parsing/configuration, random draw, cell traversal, float operation, sprite pixel, protocol byte, lifecycle state, or external scheduling.
3. Reduce the case while preserving that mismatch. Keep the original failing artifact.
4. Fix the Rust translation or harness. If the pinned C behavior itself is invalid, document the precise proposed deviation.
5. Run the affected layer, its downstream comparisons and the required gate. Broaden tests when the fix changes shared behavior.

Fixtures are generated by C only during explicit baseline work. A normal test run must never overwrite expected output. An update includes the old/new digest, exact generator command and environment, rationale, and a decoded/readable diff. A Rust mismatch alone is not a rationale to regenerate C fixtures.

Changing the upstream commit creates a new reference manifest and a reviewed behavioral delta; it must not overwrite the old baseline invisibly. Test fixes retain a trail showing whether the fixture, adapter, reference or Rust implementation changed.

## 9. Release checklist and claim

- [ ] P0–P6 evidence is complete on all supported targets; no required checks are ignored, skipped, timed out, or merely cross-compiled.
- [ ] Every C01–C18 requirement links to executed evidence; all 112 C tests map to substantive Rust checks or explicit applicability decisions.
- [ ] Every supported CLI option, alias, key, renderer, mode, palette, shape and preset is exercised, including the identified interactions.
- [ ] Exact, numerical, recording, ABI, lifecycle, failure, visual and performance checks pass.
- [ ] The dependency graph is empty beyond the first-party package, native linkage is approved system linkage, and a fresh offline build/install succeeds.
- [ ] No production C code or external helper executable is required; no fixtures or oracle programs are inadvertently installed.
- [ ] Upstream license, artwork and source attribution are preserved.
- [ ] Deviations are absent, or explicitly accepted and disclosed with the affected compatibility claim narrowed accordingly.
- [ ] Release notes identify the cbirds commit, rbirds revision/version, Rust toolchain, tested OS/terminal matrix, artifact hashes and remaining support limits.
- [ ] A fresh checkout reproduces the verification process, and the pinned C reference is still retrievable.

An appropriate claim is: “rbirds preserves cbirds 1.4.0 behavior at commit cc446fc3cb80733371c62676533adcac2fc10002 across the documented compatibility suite on the recorded Linux and macOS targets, except for the listed identity changes and disclosed deviations.” Do not claim universal or mathematically proven equivalence.

## 10. Starting the implementation

The next task is P0: construct the reference harness and immutable fixtures, validate the existing local baseline on the full matrix, and make test inventory coverage executable. Then implement P1. The current documentation work has not performed those phases or established Rust parity.

For each implementation change, report the phase, requirements addressed, C/Rust test mappings added, checks actually run, first unresolved difference if any, and remaining gates. “Compiles” and “birds are visible” are useful milestones, not completion criteria.
