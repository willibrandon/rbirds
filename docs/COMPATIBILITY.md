# Compatibility contract

Status: every requirement has test evidence on the four targets, except where the ledger says otherwise. See [the evidence index](evidence/README.md). Checks of how it looks in real terminals are still open.

The results below cover Linux and macOS. Windows has native tests described in
[WINDOWS.md](WINDOWS.md). Sixel and Windows have no cbirds equivalent; D-004 and
D-005 list those differences. Results for this branch are in the evidence index.

Reference: [cbirds 1.4.0, commit cc446fc3cb80733371c62676533adcac2fc10002](https://github.com/clainstone/cbirds/tree/cc446fc3cb80733371c62676533adcac2fc10002). Inspect that revision, not a moving upstream branch. [The manifest](reference-manifest.json) pins its tracked file hashes.

## 1. Comparison rules

| Class | Required comparison |
| --- | --- |
| Exact | Integer/RNG state, indexes, configuration, event ordering, decoded pixels, cell state, protocol bytes and deterministic encoded files match exactly. |
| Normalized exact | Apply only the field-specific changes listed below, then compare exactly. Every normalization must have tests that prove unrelated differences survive. |
| Controlled numerical | Inject identical duration/event inputs and compare raw `f32`/`f64` bits on the same target against the canonical C build. Diagnose differences before proceeding; no blanket tolerance. |
| Lifecycle | Assert state transitions, resource restoration, responsiveness, file effects and exit behavior under controlled I/O and subprocess execution. |
| Measured | Compare performance and live timing under the budgets in the port process. Timing numbers are never used as exact golden output. |
| Visual | Check real terminal output and playback in addition to automated gates. Human similarity does not override a failed exact comparison. |

Allowed normalized fields are limited to:

1. Product identity: command names in known help/completion/error locations, version product/version fields, and the cast header's title. Record the rbirds version actually used. Do not replace arbitrary substrings in user paths or file contents.
2. The cast header's wall-clock `timestamp`. Recording event timestamps and output bytes remain exact.
3. Harness-generated absolute temporary paths, only where the fixture declares a path field.
4. Measured durations, derived FPS, and live performance-panel statistics. Validate their field format and meaning separately. Output byte counts remain exact for the same deterministic frames.
5. Sixel's added help text, example, completion choices and invalid-render diagnostic (D-004). `tests/cli_differential.rs` applies those literal additions to expected C output. The rest of the output must match.

OS-provided error text is compared against the same-target C execution; cross-OS wording need not match. Freeze the environment for each comparison and preserve behavior under additional environments as separate cases. Do not strip all stderr, ANSI sequences, whitespace, metadata, or numeric fields to obtain a match.

## 2. Required behavior ledger

A completed row links to automated test cases and their evidence through [the evidence index](evidence/README.md). The C test inventory complements this ledger; neither replaces the other.

| ID | Behavior and important boundaries | Evidence | Status |
| --- | --- | --- | --- |
| C01 | CLI forms, validation, aliases, duplicate flags, unknown-option suggestions, argument order, help/version early returns, stdout/stderr and exit status | CLI differential corpus + all `options_test.c` tests; `tests/cli_differential.rs`, `tests/options_oracle.rs`, `tests/c_options.rs` | passing |
| C02 | Defaults, presets, notches, perception snapping, reset and matrix overrides | Configuration traces + boids control tests; `tests/sim_oracle.rs` (notch sweep, presets, keys), `tests/c_boids.rs` | passing |
| C03 | RNG sequence, seed-zero equivalence, large internal seeds, draw count/order | Exact RNG vectors and state traces; `tests/c_boids.rs::test_a_seed_draws_the_same_numbers_everywhere` (glibc cross-check on Linux), every RNG word in every `tests/sim_oracle.rs` dump | passing |
| C04 | Spatial cell mapping, border clamping, stable membership, neighbor order, reuse on rebuild | Grid oracle + brute-force tests; `tests/c_spatial_grid.rs`, grid digests in `tests/sim_oracle.rs`, `test_engine_matches_brute_force` | passing |
| C05 | Flocking, boundaries, panel repulsion, leash, population growth/shrink, flocks and avoidance | Numerical traces + long-run behavioral assertions; `tests/sim_oracle.rs` recordings and live sessions, `tests/c_boids.rs` | passing |
| C06 | Hawk selection, chase commitment, lead, dive, spacing and edge recovery | Hawk/boid traces and reference scenarios; `tests/sim_oracle.rs` (hawk scenarios), `test_hawks_hunt_and_the_flock_flees` and siblings | passing |
| C07 | Time, speed substeps, zero duration, pause, step, intro/outro, idle autopilot | Injected clock/event scenarios; `tests/sim_oracle.rs` (zero, 240 Hz, late frames, substeps, intro, autopilot, outro), recording rates | passing |
| C08 | Wings, glides, trails, depth, formations, matrix rain, palette/shapes and custom sprites | State, sprite pixel and frame comparisons; `tests/sim_oracle.rs`, `tests/headless_differential.rs` (shapes, custom sprite, matrix, depth, trails), `tests/png_oracle.rs` sprite pipeline | passing; visual check pending |
| C09 | PNG formats, transforms, DEFLATE, checksums, limits and malformed input | All PNG tests + bidirectional C/Rust decoding; `tests/c_png.rs`, `tests/png_oracle.rs` | passing |
| C10 | GIF palette, LZW, frame count, delay, loop metadata, errors and compression | GIF tests + byte and independently decoded comparisons; `tests/c_gif.rs`, `tests/gif_oracle.rs`, every rate in `tests/headless_differential.rs` | passing; read by ffprobe, ImageMagick and Pillow |
| C11 | Braille/sextants/blocks, glyph maps, alpha thresholds, colors, unchanged cells, cursor movement, panel exclusion | Cell state and exact emitted bytes; `tests/c_cells.rs`, `tests/cells_oracle.rs`, text frames in `tests/sim_oracle.rs` | passing |
| C12 | Kitty uploads/chunking/base64, placements, IDs, deletion, synchronized updates | Protocol tests and scripted terminal transcripts; `tests/c_kitty.rs`, `tests/kitty_oracle.rs`, Kitty frames in `tests/sim_oracle.rs`, PTY Kitty case | passing; real Kitty and Ghostty pending |
| C13 | Live defaults, theme queries/fallback, truecolor detection, dimensions and resize | Scripted PTY + real terminals; `tests/pty_reference.rs`, `tests/pty_rbirds.rs` (theme replies, fragments, resizes, tiny windows) | passing; real terminals pending |
| C14 | Keys, fragmented escape sequences, mouse reports, malformed input, hidden sequence | Parser oracle + PTY input scripts; `tests/sim_oracle.rs` (keys, split sequences, mouse, Konami), input tests in `tests/c_boids.rs` | passing |
| C15 | Terminal setup/cleanup, partial setup, signals, panic/error paths, blocked/closed output | ABI probes + failure injection + subprocess/PTY checks; `tests/abi.rs`, `tests/pty_reference.rs`, `tests/pty_rbirds.rs`, `tests/allocation_failure.rs` | passing (see D-001) |
| C16 | GIF/cast recording, snapshots, headless defaults, mode selection and diagnostics | End-to-end files and decoded frames; `tests/headless_differential.rs`, recording tests in `tests/c_boids.rs`, PTY snapshot cases | passing |
| C17 | Bench output, configuration, byte counts, release performance and allocation behavior | Same-host C/Rust measurements; `tests/headless_differential.rs` (bench output, byte counts) | passing; budgets met on macOS arm64 |
| C18 | Offline/no-dependency build, supported targets, clean install/uninstall and provenance | Dependency audit, native CI and packaging smoke checks; `tools/verify.sh` (dependency graph, linkage, install), Rosetta and Docker runs | passing, natively in CI on all four targets |

## 3. CLI surface

The option table in `boids.c` is authoritative. This inventory includes its 29 rows and the parser's special options.

| Option | Range, choices, default, or special behavior |
| --- | --- |
| `-n`, `--birds` | 1–4096; default 800 |
| `-s`, `--size` | 4–64 pixels; default 30 |
| `-g`, `--flocks`, `--groups` | 1–3; default 1; `groups` is a hidden alias |
| `-k`, `--hawks` | 0–4; default 0 |
| `--preset` | `murmuration`, `swarm`, `storm` |
| `--seed` | 0–2147483647; absent-seed behavior is mode-specific |
| `--boundary`, `--separation`, `--alignment` | 0–12; default 4 each |
| `--turning` | 0–12; default 8 |
| `--perception` | 12–60 pixels; default 36; maps to the notch grid |
| `--speed` | 0–12; default 1, or 0.4×; endpoints 0.2× and 2.6× |
| `--avoidance` | 0–12; default 4; warning with one flock and a nondefault value |
| `-c`, `--color`, `--palette` | `theme`, `ember`, `ice`, `acid`, `matrix`, `aurora`, `prism`, `potion`, `dusk`, `ash`; hidden `palette` alias |
| `--shape` | `bird`, `arrow`, `plane`, `dot` |
| `--sprite` | Custom PNG path; checked before mode dispatch; preserve file bytes and colors |
| `-e`, `--trails` | Flag, initially off |
| `--depth` | Flag, initially off |
| `-l`, `--panel` | Flag, initially off |
| `--render` | `braille`, `sextants`, `blocks`, `kitty`, `sixel`; preserve internal unset state until mode selection |
| `--matrix` | Enables rain, matrix palette, trails and alignment override |
| `--bench`, `--frames` | 0–1000000; zero does not mean a one-frame run |
| `--snapshot` | Output PNG path; requires a live run |
| `--record` | Output path; suffix controls cast versus GIF |
| `--record-fps` | 2–120 accepted; default 25; GIF chooses a representable rate no higher than 50 |
| `--record-seconds` | 1–120; default 6 |
| `--record-size` | 40–400 columns by 14–120 rows; default 96×26; strict `COLSxROWS` grammar |
| `--unlock-fps` | Removes frame delay, retains elapsed-time motion |
| `-h`, `--help` | Short versus full help; early-return behavior matters |
| `-V`, `--version` | Successful early return with product/version output |
| `--completion` | `bash`, `zsh`, `fish`; preserve option discovery, descriptions and quoting |

Preserve attached short values, clustered short flags, `--name=value`, `--name value`, generated `--no-NAME` forms for eligible flags, and the bare `--` rule. There are no positional arguments. Test accepted flag negations against the parser, including aliases, rather than synthesizing a new interpretation.

Important source-level cases that a conventional Rust CLI parser could change:

- Numeric options use `strtod` followed by range and integrality checks. Characterize decimal, exponent, hexadecimal, leading whitespace/sign, underflow/overflow, trailing bytes, NaN and infinity. An integer-only Rust parser would reject some valid C inputs.
- Preset expansion restores certain explicit slider values only when they differ from shipped defaults. For example, `--preset murmuration --boundary 4` retains the preset boundary rather than forcing 4; test reversed argument order too. Preserve measured behavior despite the nearby comment's broader description of override precedence.
- Matrix overrides happen after ordinary option application and warnings. A requested value can be superseded; order is observable.
- `--bench N` with N greater than zero takes precedence over `--record`; `--bench 0` does not. Record/snapshot/frame-limit combinations must follow the C implementation.
- Usage failures exit 2; runtime failures generally exit 1; success exits 0. Verify early-exit combinations, not just individual options.
- Preserve non-UTF-8 Unix paths where C accepts them, and characterize invalid option bytes without lossy conversion or a Rust panic.

## 4. Interactive and rendering surface

Keep the default live renderer braille regardless of terminal name. Kitty sprites are explicitly selected and retain the reference's terminal-support scope: Kitty and Ghostty. Do not claim new support for tmux passthrough or other graphics terminals as part of the port.

Use `--render sixel` for Sixel on any supported OS. Startup checks terminal support
and asks for the graphics cell size. Windows Terminal needs version 1.22 or newer.
Each frame repaints its background using a 256-color palette. Snapshots and GIFs
keep full color; casts use braille. See [WINDOWS.md](WINDOWS.md#sixel-output).

Preserve the BOIDS intro, mouse repulsion, the approximately 40/60-second quit flight, and the idle behavior starting at 60 seconds. The actual implementation determines which input bytes clear the intro and reset idle time; mouse reports and escape sequences are not interchangeable with ordinary keys.

| Input | Behavior |
| --- | --- |
| `b/B`, `s/S`, `a/A`, `t/T`, `p/P`, `v/V` | Decrease/increase the corresponding notch, clamped to bounds |
| `g/G` | Avoidance, effective only with multiple flocks |
| Space, `.` | Pause/resume and grant one simulation frame |
| `0`, Tab | Reset shipped slider defaults; cycle presets |
| `+`, `=`, `-` | Grow/shrink population using the existing formulas and limits |
| `h`, `e` | Toggle panel and trails |
| `k`, `K` | Add/remove a hawk subject to limits and initialized sprite state |
| `q` | Quit with the existing outro, including the backpressure-path behavior |
| Arrow sequence plus `b`, `a` | Preserve the existing Konami behavior and parser rules |

The panel excludes its region from drawing and simulation, hides below the minimum viewport, and changes row count with multiple flocks. Preserve UTF-8 glyphs, display-column widths, numeric formatting, redraw behavior and turning/speed semantics. Do not equate UTF-8 byte length with cell width.

Test light/dark/custom themes, missing and fragmented theme replies, truecolor and indexed colors, zero/partial pixel dimensions, missing terminal size, tiny windows, panel threshold sizes, repeated resize and recovery. Preserve fallback constants and query deadlines from the source.

Keep sprite draw order, heading quantization, wing phases, alpha handling, trail sampling, far-layer placement and tint, hawk colors, and custom-sprite behavior. Compare complete sprite catalogs, not only the default bird. Match braille numbering, sextant code points, block behavior, changed-cell optimization and terminal background preservation.

## 5. Files, modes and codec boundaries

- Headless benchmark defaults to Kitty-style frame construction, a 200×50 cell / 1600×800 pixel viewport, fixed 60 Hz simulation time, and seed 1 unless specified. It must not enter raw mode or emit frames to a terminal.
- GIF recording defaults to sprites; an explicit text renderer records reconstructed cell pixels. GIF recording suppresses the panel and its reserved space. Frame count follows the actual representable GIF rate, including noninteger rates.
- GIF rates minimize rate error over allowed hundredth-second delays, rather than simply rounding `100 / requested_fps`. Test all accepted rates from 2 through 120 and the rounding diagnostic.
- A case-sensitive `.cast` suffix selects cast only under the exact source condition (`name_length > 5`). Thus the filename `.cast` itself does not select cast. Test names, not merely extensions through a convenience API.
- Cast forces braille and truecolor, including when another renderer was requested, and disables the panel. Keep JSON escaping, four-decimal event times, opening/closing control sequences, and recorded frame count.
- Recordings use seed 1 by default and their encoded clock. Live initialization follows its separate source path. `--frames` and `--snapshot` do not acquire new meanings in headless modes.
- Snapshot captures the final live state using the selected renderer; a failed requested snapshot makes the run fail. Test error destinations and terminal restoration around snapshot writing.
- PNG support includes grayscale at 1/2/4/8/16 bits, indexed color at 1/2/4/8 bits, RGB/gray-alpha/RGBA at 8/16 bits, tRNS, all standard filters, and Adam7. Preserve high-byte reduction of 16-bit samples and the reference's handling of ancillary chunks.
- Preserve 4 MB custom-sprite input handling, 16,384-pixel dimension and 64-million-pixel decoder limits, exact expected inflated raster length, checksums, chunk/order validation, and malformed DEFLATE behavior where defined. Reject oversized allocations before multiplication overflow. Boundary tests must distinguish file-size limits from decoded-image limits.
- GIF must retain palette construction, LZW behavior, block packing, loop metadata, frame delays and failure propagation. PNG/GIF encoders must be checked both bytewise and through an independent reader.

## 6. Limits and deviations

The only initially accepted output changes are the `rbirds` identity fields described above. Runtime semantics, the BOIDS lettering, and upstream artwork remain unchanged. These documents do not authorize arbitrary bug fixes disguised as migration work.

If reference behavior is undefined, unsafe, internally contradictory, or dependent on unrecoverable OS failure, capture a minimized reproduction and classify it. Preserve well-defined behavior; never reproduce memory corruption to satisfy a comparison. Propose a safe contract for that specific case and record its acceptance before marking the affected requirement complete.

Every deviation record must include: ID, reference commit, target, input, expected/actual behavior, first divergence, reason, affected requirements, regression tests, acceptance decision, and claim affected. An accepted scope change can support a qualified compatibility release; it cannot be presented as proof of identical behavior. Routine implementation choices within this contract need no additional approval.

No guarantee covers all possible inputs, all terminal emulators, uncatchable signals, arbitrary future OS/library behavior, or untested architectures. The enforceable promise is that all declared gates pass on the recorded reference, targets and corpus, with no undisclosed differences.
