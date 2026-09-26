# Windows and Sixel

rbirds runs natively on Windows with the MSVC toolchain. Use `--render sixel` for
pixel graphics in Windows Terminal, or leave it off for braille.

Windows x64 is tested. The bindings also permit ARM64, but it has not been tested
on hardware. GNU/MinGW and 32-bit Windows are unsupported.

## Run in Windows Terminal

Install Rust through rustup with the MSVC toolchain, Visual Studio's C++ build
tools and Windows SDK. The repository selects Rust 1.96.0 automatically. In a
PowerShell tab in Windows Terminal 1.22 or newer:

```powershell
cargo build --release
.\target\release\rbirds.exe --render sixel --panel
# A smaller flock or window reduces Sixel encoding and terminal painting work.
.\target\release\rbirds.exe --render sixel --birds 200
# Portable text fallback; also the default when --render is omitted.
.\target\release\rbirds.exe --render braille
.\target\release\rbirds.exe --help
```

Keys, mouse repulsion, resize, snapshots, GIF/cast recording and benchmarking use
the shared application code. Press `q` for the normal flight out; Ctrl+C or
Ctrl+Break exits with status 130. Windows Unicode file paths work, including
non-ASCII and supplementary characters. Ill-formed UTF-16 arguments are rejected
with a usage error; arbitrary non-UTF-8 Unix path bytes remain supported on Unix.

Help and diagnostic messages use Unicode console output without changing the
console code page. Redirected output stays UTF-8. The live renderer saves the
code pages, selects UTF-8 while running, and restores them on exit.

VS Code's integrated terminal is a different emulator from Windows Terminal.
Use braille there unless its version advertises Sixel and replies to the cell-size
query. For pixel graphics, run the executable in a Windows Terminal tab. Rust
Analyzer now has a native `platform/windows.rs`; `cargo check --all-targets`
checks the Windows application and tests. Reload the workspace only if diagnostics
remain stale after that command succeeds.

## Terminal support

Windows Terminal 1.24.11911.0 supports Sixel. Its source at commit `fda72a070`
has these pieces:

- `src/terminal/parser/stateMachine.cpp`: APC strings, including Kitty graphics'
  `ESC _ G`, go through the unsupported-string path. Passthrough code does not
  provide an image renderer.
- `src/terminal/adapter/SixelParser.cpp`: Sixel decoding, a 256-register palette,
  transparent pixels, and default **10×20 virtual pixels per cell**.
- `src/terminal/adapter/adaptDispatch.cpp`: Sixel capability `4` in primary device
  attributes, cell/pixel size reports, and Sixel display mode.
- `src/buffer/out/ImageSlice.*` and the Atlas rendering engine: image storage and
  painting for Sixel.

Kitty **keyboard** protocol support does not imply Kitty **graphics** support.
Adding graphics to Windows Terminal would require parser, image lifetime,
placement, compositing and rendering work in Windows Terminal itself.

Microsoft introduced Sixel in [Terminal Preview 1.22](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release/)
and promoted it to stable 1.22 with the [Preview 1.23 release](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-23-release/).
The [Kitty graphics request](https://github.com/microsoft/terminal/issues/17309)
is separate. Support here is based on capability replies, not the terminal name.

## Sixel output

`--render sixel` is explicit on every platform. Startup asks for primary device
attributes (`CSI c`) and requires capability `4`, then asks for graphics cell
dimensions (`CSI 16 t`). Missing or invalid replies fail clearly. Windows
Terminal's virtual raster must be used instead of the font's physical pixel
size. Window resizing recomputes the raster from columns/rows and the negotiated
cell size; changing another emulator's cell size during a run requires restarting.

The renderer composes a full image and encodes six-row bands with run-length
compression. Its fixed 256-color palette contains a color cube, greys and the
picture background. Alpha is flattened onto that background and every frame
repaints it, so moving birds do not leave old pixels behind. This uses more
bandwidth than retained Kitty sprites and quantizes colors; it is not a
byte-for-byte Kitty replacement. The panel is drawn over the raster, with
synchronized updates around each frame.

Sixel display mode (`DECSDM`, private mode 80) clips the image at the viewport
instead of scrolling. rbirds queries and restores its prior state; if an emulator
does not report it, cleanup assumes the usual disabled state. No private-mode
save/restore extension is assumed. GIFs and PNG snapshots retain the full-color
composition; `.cast` always uses braille, as in the original application.

## Console handling

The Windows module uses Win32 console APIs and the Microsoft C runtime, with no
new crates. Input records become the bytes used by the shared key parser; resize
and mouse positions use the visible console window, not the scrollback buffer. Input/output modes
and code pages are saved and restored. These choices follow Microsoft's
[SetConsoleMode documentation](https://learn.microsoft.com/en-us/windows/console/setconsolemode)
and [VT sequence documentation](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences).

A single writer thread holds at most one 64 KiB chunk so keys remain serviceable
while output is blocked. Shutdown allows a short drain, then cancels a blocked
write, and gives up on a write that can't be cancelled after a second. If output
cannot drain, it cannot carry screen-cleanup sequences; native console
modes/code pages are still restored. Normal exit, handled Ctrl events,
and Rust unwinding restore the terminal. Forced termination and closing the
console are not restoration guarantees. Windows uses native math/CRT behavior;
the Unix C differential guarantees are not extended across operating systems.

## Development

PowerShell scripts require **PowerShell 7 or newer** (`pwsh`).

| Unix workflow | PowerShell entry point | Scope |
| --- | --- | --- |
| `tools/verify.sh` | `tools/verify.ps1` | Native format, check, Clippy, debug/release tests, release build, dependency/unsafe/import audits, install/uninstall |
| `tools/reference.sh` | `tools/reference.ps1` | Isolated pinned C checkout, clean-tree and all 43 SHA-256 checks; existing mismatches fail without checkout/reset |
| `tools/perf.sh` | `tools/perf.ps1` | Native samples, median/range, encoded byte count, sampled OS peak working set, raw reports and environment metadata; includes Sixel |
| Unix C performance comparison | `tools/perf.ps1 -CompareReference` | Runs `perf.sh` in WSL; C has no Sixel workload |
| Whole Unix verification | `tools/verify.ps1 -AllLocal -Distribution Debian` | Native Windows gates plus WSL Unix gates |
| Docker architectures / VHS | `tools/unix.ps1 linux` / `tools/unix.ps1 vhs` | Existing Unix workflows in WSL, with their original prerequisites |

WSL needs Git, the pinned Rust toolchain with rustfmt/Clippy, GCC, make and the
usual Unix tools. VHS additionally needs VHS, ttyd, ImageMagick and a suitable
font. Docker needs a working daemon/integration inside WSL. Native Windows
verification needs `dumpbin.exe` from the Visual C++ tools for its PE import audit.
Builds are offline after the toolchains are installed; fetching the C reference
may use the network. Shell scripts keep LF through `.gitattributes`.

`perf.ps1 -Samples 3 -Frames 60 -Birds 200 -Render sixel` is a short native
measurement. It reports encoding/simulation time, not Windows Terminal painting
latency. Use a quiet host for meaningful results; this smoke workload is not a
release performance budget.

Memory counters are sampled while the process is alive. The CSV's
`ObservedPeakWorkingSetBytes` may miss allocations in the final sampling interval;
zero means no live sample was obtained. It is not the Unix peak RSS measure.

## Tests

- `tests/sixel.rs`: independent raster decoding, palette colors, partial bands,
  runs, alpha/background, erasing a previous frame, malformed images and queries;
  complete frame composition through panel removal and resize.
- `tests/pty_sixel.rs` (Unix): fragmented negotiation, virtual pixel dimensions,
  two emitted frames, prior mode restoration, unsupported/missing-size failures,
  and terminal attribute restoration through the real executable.
- `src/platform/windows_tests.rs`: private hidden console subprocesses exercise
  Win32 input, Unicode, mouse/arrows, resize, panic and Ctrl+Break restoration,
  Unicode snapshots, native query replies, exact bytes under backpressure and cancellation.
- `tests/portable_cli.rs`: help/completions, diagnostics, Sixel benchmark, Unicode
  GIF/cast paths, recording equivalence and file errors. Windows additionally
  checks rejection of malformed UTF-16.
- Portable translated tests run natively; boids tests use recorded C input states
  as described in [the fixture provenance](../tests/fixtures/README.md). Unix
  continues fresh C comparisons and all POSIX lifecycle tests.

CI includes a Windows x64 job alongside the four existing Unix runners. A local
pass does not establish that a hosted job ran. Windows ARM64 and visual rendering
in actual emulators remain manual validation items. Before release, record the
terminal version/font and check:

1. Sixel in Windows Terminal: birds, trails, hawks, panel toggling, mouse input,
   repeated resize, no stale pixels, `q`, Ctrl+C and usable shell afterwards.
2. Braille/blocks/sextants on Windows and existing Unix terminals, including
   missing-glyph behavior of the selected font.
3. Sixel on other compatible emulators, including their size and mode replies;
   Kitty/Ghostty's existing Kitty renderer on Unix.
