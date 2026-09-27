# Local shared-image transport in iTerm

The output-cursor change in `3f324d3` removed repeated memory copies, but dense
iTerm Kitty playback still spent about 41 ms per frame flushing compressed
images through the PTY. A local macOS terminal can read the same RGBA pixels
from POSIX shared memory. This avoids frame compression, Base64 encoding and
bulk PTY transport while retaining the existing composition and alternating
image IDs. The capability is negotiated, so remote and unsupported connections
keep inline output.

## Behavior and lifecycle

A one-pixel query must receive an exact successful reply and its object must
have been consumed. Four private names correspond to the two image banks and
two text layers. Each name includes the process ID and a random identifier;
PID coincidences across SSH hosts cannot reasonably qualify a remote connection.
Creation uses exclusive access and mode 0600. An unread object is never replaced.
If an object is still pending, that frame uses the existing inline encoder.
This bounds outstanding resources without queuing more frames or changing pixels.

The query waits for the complete graphics reply. If it times out, the live
input parser carries any partial reply and ignores its remaining bytes through
the string terminator. A delayed error cannot become keyboard shortcuts.

The terminal unlinks each object after copying it. The application also tracks
objects for normal exit, errors, panic unwinding and caught signals. Darwin's
`shm_unlink` is a generated, non-cancelable syscall stub; its failure path sets
errno without allocation or locking. The application resolves that stub and
its errno access before creating any image. Its emergency registry uses fixed
storage and lock-free atomics. Creation and registry publication briefly block
signals on the application's single writer thread. This design is specific to
macOS and does not assume the same libc behavior on another OS. SIGKILL cannot
provide cleanup guarantees.

The source audit used Apple's
[syscall table](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/syscalls.master),
[stub generator](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/xcodescripts/create-syscalls.pl)
and [non-cancelable error path](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/custom/errno.c).
The native header probe covers the new function signatures, scalar type and
constants. Mach-O inspection also confirmed that the cleanup import was lazy,
which motivated resolving it before signal cleanup could need it.

## CPU and presentation

Two forward/reverse pairs per scene compared the candidate with `3f324d3` on
an M4 Pro, macOS 26.5.2 and iTerm2 3.6.6. Each used a foreground 100-by-32-cell,
1400-by-1088-pixel window, seed 42, 1,200 loop ticks, a three-second warm-up and
a fifteen-second CPU interval. No capture, profiler, build or tests ran during
these intervals. All eight samples were valid. Median CPU milliseconds per
elapsed second were:

| Scene | Process | Inline | Shared memory |
|---|---|---:|---:|
| Default Kitty | rbirds | 400.49 | 248.51 |
| Default Kitty | iTerm main process | 785.22 | 418.56 |
| Default Kitty | Combined | 1185.71 | 667.06 |
| Dense Kitty | rbirds | 464.97 | 692.80 |
| Dense Kitty | iTerm main process | 1301.79 | 455.71 |
| Dense Kitty | Combined | 1766.76 | 1148.51 |

The dense scene used 4,096 birds, speed 12, four hawks, three flocks, depth,
trails and the ember palette. Combined CPU fell 43.7% at defaults and 35.0%
in the dense scene. Dense application CPU per second rose because it now
advances about 60 frames/s instead of 16. Whole-run application CPU per frame
fell from 25.24 to 11.04 ms, about 56%. Those lifetime values are separate
from the equal-interval CPU rates. The selected decoder helper stayed alive
and used zero measured CPU. iTerm was shared with existing sessions; compositor
and GPU work remain outside these counters.

Separate ten-second visible captures recorded:

| Scene | Samples | Changed images/s | p99 gap, ms | Maximum gap, ms |
|---|---:|---:|---:|---:|
| Default Kitty | 859 | 60.00 | 34.76 | 38.84 |
| Dense Kitty | 924 | 59.94 | 28.96 | 37.37 |

Neither capture contained a nearly blank or bright-background sample under
the retained analyzer's thresholds, and both screenshots were inspected.
The capture process used 0.66 and 0.77 CPU seconds respectively. These are
ScreenCaptureKit observations at the window's logical resolution, not physical
scanout measurements or a guarantee against future stalls. Some builds and tests
ran during these initial captures; their CPU traces are not quiet comparisons.
The dense CPU runs'
median application frame interval was 16.67 ms, versus about 61 ms before;
their p99 intervals were 16.73 and 16.72 ms, with maxima of 18.63 and 19.92 ms.

The measured executable preceded stricter string-reply handling, random name
initialization, eager resolution of the cleanup syscall, delayed-reply filtering
and additional tests. Those changes affect startup, input and cleanup; the frame
path without input is unchanged. A final-build dense capture measured 59.78
changes/s across 890 samples, with a 27.73 ms p99 gap and a 29.84 ms maximum.
It contained no nearly blank or bright-background sample and used 0.69 capture
CPU seconds. Its application trace had a 16.70 ms p99 interval and a 19.26 ms
maximum. No build or test ran during this final capture.

A separate final-build control check paused, stepped, hid the panel, resized
from 100-by-32 to 120-by-36 cells, restored the panel and resumed. All 1,395
samples passed the background and blank-frame checks. Both screenshots were
inspected, and the finite run exited successfully. The intentional pauses and
resize make this a control check rather than steady-cadence evidence.

The original protocol probe and its corrected failures are retained: Darwin's
variadic `shm_open` requires the correct declaration on Apple arm64. With that
declaration, both the compressed inline upload and raw shared upload displayed
the same static RGBA surface with zero differing checked content pixels.

## Verification status

An independent C reader checks exact cropped pixels, permissions, page padding,
refusal to overwrite an unread object and reuse after consumption. Subprocess
tests check panic and SIGTERM cleanup. A terminal double exercises successful
negotiation and cleanup after normal quit, a signal and terminal loss. It also
sends full and partial replies after the timeout and checks continued inline
playback. Other tests cover refused, unanswered and unconsumed probes,
fragmented error replies, every late-reply byte boundary, identical inline
fallback bytes, stacking order and allocation behavior.

Native debug and release checks pass, including ABI, terminal lifecycle, C
simulation behavior and steady-state allocation tests. Relevant release tests
also pass under Intel macOS through Rosetta. Formatting, Clippy and the release
build pass. The archive retains initial failures and their corrections: a C
reader originally assumed unrounded object sizes, a terminal double tried to
consume an image after exit cleanup, and a fragmented-reply test used an
incorrect event label. The corrected checks pass on both Mac architectures.

The [output archive](live-iterm-shm-2026-09-27-macos-arm64.tar.gz) contains CPU
intervals, traces, captures, test-window screenshots, drivers, source changes,
executable hashes, the syscall audit and validation logs. Native Windows and
Linux desktop playback were not measured, and their renderer paths do not use
this macOS transport. Dense Sixel and text presentation remain separate open
areas. No installed executable or terminal preference was replaced.
