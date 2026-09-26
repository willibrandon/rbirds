#!/usr/bin/env python3
"""Live frame pacing under a pseudoterminal.

Runs a program live on a PTY of a given size, reads its output as fast as a
terminal would, answers device status requests (ESC [ 5 n) as a terminal
does, types q at the end, and reports what the terminal side saw: frames a
second, the gaps between frames, the bytes a second, and the CPU the
program used. Each frame begins a synchronized update (ESC [ ? 2026 h), which
is what is timed. The first second (the intro) is left out.

    tools/pacing.py [--seconds S] [--cells COLSxROWS] [--pixels WxH]
                    [--no-answer] PROGRAM [ARGS...]

`--bench` measures how long frames take to build; this measures how they
arrive, which is what a viewer sees.
"""

import argparse
import fcntl
import os
import pty
import resource
import select
import struct
import sys
import termios
import time

BEGIN = b"\x1b[?2026h"
REQUEST = b"\x1b[5n"
ANSWER = b"\x1b[0n"


def new_matches(seen, old, pattern):
    """How many times `pattern` ends in `seen` after its first `old` bytes,
    the tail kept from the read before: a match that lies wholly in that
    tail was counted then."""
    count, start = 0, 0
    while (index := seen.find(pattern, start)) >= 0:
        count += index + len(pattern) > old
        start = index + 1
    return count


def run(argv, seconds, cols, rows, width, height, answer):
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(argv[0], argv)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, width, height))
    start = time.monotonic()
    stamps, total, tail, quit_sent = [], 0, b"", False
    while True:
        now = time.monotonic()
        if not quit_sent and now - start > seconds:
            os.write(fd, b"q")
            quit_sent = True
        if now - start > seconds + 10:
            os.kill(pid, 9)
            break
        ready, _, _ = select.select([fd], [], [], 0.05)
        if not ready:
            continue
        try:
            data = os.read(fd, 1 << 20)
        except OSError:
            break
        if not data:
            break
        at = time.monotonic()
        seen = tail + data
        if answer:
            for _ in range(new_matches(seen, len(tail), REQUEST)):
                os.write(fd, ANSWER)
        stamps.extend([at] * new_matches(seen, len(tail), BEGIN))
        total += len(data)
        tail = seen[-(len(BEGIN) - 1):]
    os.waitpid(pid, 0)
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return stamps, total, start, time.monotonic(), usage.ru_utime + usage.ru_stime


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--seconds", type=float, default=10.0)
    parser.add_argument("--cells", default="200x50")
    parser.add_argument("--pixels", default="1600x800")
    parser.add_argument("--no-answer", action="store_true")
    parser.add_argument("program", nargs=argparse.REMAINDER)
    options = parser.parse_args()
    if not options.program:
        parser.error("no program to run")
    cols, rows = map(int, options.cells.split("x"))
    width, height = map(int, options.pixels.split("x"))
    stamps, total, start, end, cpu = run(
        options.program, options.seconds, cols, rows, width, height, not options.no_answer
    )
    stamps = [s for s in stamps if start + 1.0 <= s <= start + options.seconds]
    gaps = sorted((b - a) * 1000 for a, b in zip(stamps, stamps[1:]))
    if len(gaps) < 2:
        print("fewer than three frames seen", file=sys.stderr)
        return 1

    def at(fraction):
        return gaps[min(len(gaps) - 1, int(fraction * len(gaps)))]

    span = stamps[-1] - stamps[0]
    print(
        f"{len(gaps) / span:5.1f} fps | gap ms p50 {at(0.5):5.1f} p90 {at(0.9):5.1f} "
        f"p99 {at(0.99):5.1f} max {gaps[-1]:6.1f} | over 25 ms {sum(g > 25 for g in gaps)}, "
        f"over 50 ms {sum(g > 50 for g in gaps)} of {len(gaps)} | "
        f"{total / 1024 / (end - start):5.0f} KiB/s | {cpu / (end - start):4.2f} cores"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
