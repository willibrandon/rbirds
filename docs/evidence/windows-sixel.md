# Windows and Sixel tests

Tested on September 26, 2026 UTC, in the `feature/windows-sixel` working tree.
The branch starts at `5eee8a75429e96cb020bb8016c32c214008e4d3a`.

| Environment | Tools | Result |
| --- | --- | --- |
| Windows 11 Pro x64, build 26200 | Rust 1.96.0 MSVC, PowerShell 7 | `tools/verify.ps1` passed |
| Debian x86_64 in WSL | Rust 1.96.0, GCC 14.2.0, glibc 2.41 | `tools/unix.ps1 verify -Distribution Debian` passed |
| GitHub Actions workflow | `actionlint .github/workflows/ci.yml` | Passed; hosted jobs have not run for this branch |

Both verification scripts passed formatting, all-target checks, Clippy with
warnings treated as errors, debug and release tests, release builds, dependency
checks, native library checks, and installation followed by uninstallation.
Linux also ran the C comparisons and verified that all 112 C test mappings ran.

The full Windows runs passed 158 tests per profile. The full Linux runs passed
308 per profile. The final allocation-failure test and Sixel frame-composition
test were checked again in debug and release after those runs. Windows' one
ignored test is a child-process fixture invoked explicitly by six parent tests.
It is not a skipped console test.

## New tests

| Behavior | Test and assertion |
| --- | --- |
| Sixel pixels, runs and last band | `colours_runs_and_partial_bands_decode_to_the_expected_raster` independently decodes the complete image and compares every pixel |
| Alpha and old pixels | `alpha_is_flattened_and_a_second_frame_erases_old_birds` checks translucent, opaque and transparent pixels across successive frames |
| Composition, panel and resize | `frame_queue_composes_birds_repaints_after_panel_removal_and_resizes` checks complete expected rasters through the shared frame queue |
| Bad images and allocation failure | `malformed_images_are_rejected_without_queuing_a_partial_frame` and `sixel_plane_allocation_failure_preserves_output_and_can_recover` check errors, unchanged queued bytes and recovery |
| Capabilities and dimensions | `terminal_capabilities_and_virtual_cell_sizes_are_parsed_precisely` checks valid replies, false capability matches, missing fields, zero sizes and overflow |
| Live Sixel startup | `sixel_negotiates_virtual_pixels_renders_frames_and_restores_mode` runs the binary in a PTY, fragments a reply, checks two frames at 800x480, and checks both prior mode states |
| Unsupported terminals | `unsupported_or_sizeless_sixel_fails_cleanly` checks exit 1, restored terminal attributes and no image output |
| Windows query input | `native_sixel_queries_read_console_replies_and_reject_unsupported_terminals` answers queries through real console input records and checks the request bytes and returned dimensions/error |
| Windows input and cleanup | `native_console_input_resize_and_panic_restore_modes` checks Unicode, arrows, mouse input, resize, the exact intended panic, and restored modes/code pages |
| Windows exit and snapshots | `native_console_ctrl_break_exits_and_restores_modes` checks status 130; `native_console_frames_and_unicode_snapshot_restore_modes` reads the saved PNG and checks restoration |
| Blocked Windows output | `native_output_backpressure_preserves_bytes_and_services_input` compares all 200,000 output bytes and checks input while blocked; `native_blocked_output_can_be_cancelled` checks cancellation and restoration |
| CLI and default renderer | `help_completions_and_invalid_choice_include_sixel`, `sixel_is_an_explicit_renderer_and_the_default_stays_braille`, and `sixel_bench_is_headless_and_reports_encoded_bytes` check text, status, defaults and benchmark output |
| Unicode files and recording | `unicode_recording_paths_work_and_sixel_keeps_full_colour_gif_and_braille_cast` compares recorded files and checks a failed destination; `ill_formed_utf16_is_a_usage_error` checks Windows argument rejection |

The tests are in `tests/sixel.rs`, `tests/pty_sixel.rs`,
`tests/portable_cli.rs`, `tests/allocation_failure.rs`, and
`src/platform/windows_tests.rs`.

The 63 portable boids tests run on Windows using the C input states described in
[the fixture notes](../../tests/fixtures/README.md). The x86_64 Linux run checked
those states against a fresh C capture. Windows does not compile or run the
POSIX C suite.

## Scripts and file checks

- All four new PowerShell scripts parsed successfully.
- `reference.ps1` and the WSL reference command verified the pinned commit and
  all 43 file hashes.
- `perf.ps1 -Samples 1 -Frames 2 -Birds 3 -Render sixel` completed and wrote raw
  output, CSV samples, a summary and environment details. The memory sample was
  nonzero. This was a script check, not a performance claim.
- `git diff --check` passed. Existing edited files retain CRLF in the Windows
  checkout. Shell scripts use LF so they run in WSL; `.gitattributes` records that.
- The Windows Terminal reference checkout was left clean.

## Still to test

Hosted CI, native Windows ARM64, and this branch on macOS hardware have not run
here. The existing Unix CI jobs remain configured alongside the new Windows x64
job. Appearance and input in a real Windows Terminal window still need the manual
checks in [WINDOWS.md](../WINDOWS.md#tests). The console tests use hidden consoles
and scripted replies; they do not check the emulator's image renderer.
