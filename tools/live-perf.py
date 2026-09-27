#!/usr/bin/env python3
"""Measure the live Unix path through a PTY; this does not measure screen presentation."""

import argparse
import errno
import fcntl
import hashlib
import json
import math
import os
import platform
import pty
import select
import struct
import subprocess
import termios
import time
from pathlib import Path


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def longest_gap_run(intervals, threshold=25):
    longest = current = 0
    for interval in intervals:
        current = current + 1 if interval is not None and interval > threshold else 0
        longest = max(longest, current)
    return longest


def run(args):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ,
                struct.pack("HHHH", args.rows, args.cols,
                            args.cols * args.cell_width, args.rows * args.cell_height))
    command = [str(args.binary.resolve()), "--seed", str(args.seed), "--frames", str(args.frames)]
    if args.render != "default":
        command += ["--render", args.render]
    command += args.arguments
    started = time.monotonic()
    environment = {**os.environ, "TERM": "xterm-256color", "COLORTERM": "truecolor"}
    if args.trace:
        args.trace.parent.mkdir(parents=True, exist_ok=True)
        args.trace.unlink(missing_ok=True)
        environment["RBIRDS_TRACE"] = str(args.trace.resolve())
    process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=subprocess.PIPE,
                               env=environment,
                               start_new_session=True)
    os.close(slave)
    os.set_blocking(master, False)
    os.set_blocking(process.stderr.fileno(), False)
    if args.pid_file:
        args.pid_file.write_text(str(process.pid))
    frames = []
    total_bytes = 0
    carry = b""
    errors = bytearray()
    usage = None
    status = None
    master_open = True
    stderr_open = True
    events = []
    sent_input = resized = paused = False
    paused_until = 0
    queries = {
        b"\x1b]10;?\x1b\\": b"\x1b]10;rgb:dddd/dddd/dddd\x1b\\",
        b"\x1b]11;?\x1b\\": b"\x1b]11;rgb:1111/1111/1111\x1b\\",
        b"\x1b[c": b"\x1b[?62;4c",
        b"\x1b[16t": f"\x1b[6;{args.cell_height};{args.cell_width}t".encode(),
        b"\x1b[?80$p": b"\x1b[?80;2$y",
    }
    for index, rgb in enumerate(["eeee/5555/5555", "5555/eeee/5555", "eeee/eeee/5555",
                                 "5555/5555/eeee", "eeee/5555/eeee", "5555/eeee/eeee"], 1):
        queries[f"\x1b]4;{index};?\x1b\\".encode()] = f"\x1b]4;{index};rgb:{rgb}\x1b\\".encode()
    marker = b"\x1b[?2026l"
    try:
        while usage is None or master_open or stderr_open:
            now = time.monotonic()
            if now - started > args.timeout:
                raise TimeoutError(f"Live run exceeded {args.timeout} seconds")
            elapsed = now - started
            if usage is None and master_open and args.input_at is not None and not sent_input and elapsed >= args.input_at:
                os.write(master, args.input.encode())
                sent_input = True
                events.append({"at": elapsed, "input": args.input})
            if usage is None and master_open and args.resize_at is not None and not resized and elapsed >= args.resize_at:
                fcntl.ioctl(master, termios.TIOCSWINSZ,
                            struct.pack("HHHH", args.resize_rows, args.resize_cols,
                                        args.resize_cols * args.cell_width, args.resize_rows * args.cell_height))
                resized = True
                events.append({"at": elapsed, "resize": [args.resize_cols, args.resize_rows]})
            if args.pause_at is not None and not paused and elapsed >= args.pause_at:
                paused_until = now + args.pause_for
                paused = True
                events.append({"at": elapsed, "stop_reading_seconds": args.pause_for})
            readers = ([master] if master_open and now >= paused_until else [])
            if stderr_open:
                readers.append(process.stderr)
            ready, _, _ = select.select(readers, [], [], 0.02)
            if process.stderr in ready:
                data = os.read(process.stderr.fileno(), 65536)
                stderr_open = bool(data)
                errors.extend(data)
            if master in ready:
                try:
                    data = os.read(master, 1024 * 1024)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b""
                master_open = bool(data)
                total_bytes += len(data)
                stream = carry + data
                previous = len(carry)
                for query, reply in queries.items():
                    at = stream.find(query)
                    if at >= 0 and at + len(query) > previous:
                        os.write(master, reply)
                at = 0
                while True:
                    at = stream.find(marker, at)
                    if at < 0:
                        break
                    at += len(marker)
                    if at > previous:
                        frames.append(time.monotonic() - started)
                carry = stream[-64:]
            if usage is None:
                child, status, usage = os.wait4(process.pid, os.WNOHANG)
                if not child:
                    usage = None
        process.returncode = os.waitstatus_to_exitcode(status)
        if process.returncode:
            raise RuntimeError(f"Exit {process.returncode}: {errors.decode(errors='replace')}")
    finally:
        if usage is None:
            process.kill()
            process.wait()
        os.close(master)
        process.stderr.close()
    elapsed = time.monotonic() - started
    expected = args.frames
    drawn_ticks = None
    idle_ticks = 0
    if args.trace and args.trace.exists():
        with args.trace.open() as source:
            summary = json.loads(next(source))
            samples = [json.loads(line) for line in source]
        if (summary.get("kind") != "summary" or summary.get("version") not in (1, 2)
                or len(samples) != summary["samples"]
                or summary["samples"] + summary["omitted"] != args.frames):
            raise RuntimeError("Trace does not cover the requested loop ticks")
        expected = summary.get("drawn_frames", args.frames)
        idle_ticks = args.frames - expected
        drawn_ticks = [i for i, sample in enumerate(samples) if sample.get("drawn", True)]
    # Restoration emits one final synchronized-update terminator. Check it
    # separately so skipped paused ticks cannot look like lost frames.
    if len(frames) != expected + 1:
        raise RuntimeError(f"Received {len(frames)} terminators; expected {expected} frames plus restoration. Use --trace for paused playback.")
    frames = frames[:expected]
    gaps = [(b - a) * 1000 if drawn_ticks is None or (
                i + 1 < len(drawn_ticks) and drawn_ticks[i + 1] == drawn_ticks[i] + 1) else None
            for i, (a, b) in enumerate(zip(frames, frames[1:])) if a >= args.warmup]
    intervals = [gap for gap in gaps if gap is not None]
    cpu_seconds = usage.ru_utime + usage.ru_stime
    report = {
        "scope": "PTY transport, not terminal painting",
        "command": command, "platform": platform.platform(),
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "events": events,
        "viewport": [args.cols, args.rows, args.cell_width, args.cell_height],
        "wall_seconds": elapsed, "process_cpu_seconds": cpu_seconds,
        "cpu_ms_per_second_including_startup": cpu_seconds * 1000 / elapsed,
        "cpu_ms_per_frame_including_startup": cpu_seconds * 1000 / len(frames) if frames else None,
        "received_frames": len(frames), "received_bytes": total_bytes,
        "loop_ticks": args.frames, "idle_ticks": idle_ticks,
        "interval_scope": "contiguous drawn ticks covered by trace" if drawn_ticks is not None else "all received frames",
        "warmup_seconds": args.warmup,
        "interval_ms": {"p50": percentile(intervals, .5), "p95": percentile(intervals, .95),
                        "p99": percentile(intervals, .99), "p999": percentile(intervals, .999),
                        "max": max(intervals) if intervals else None},
        "gaps_over_25ms": sum(value > 25 for value in intervals),
        "gaps_over_50ms": sum(value > 50 for value in intervals),
        "gaps_over_100ms": sum(value > 100 for value in intervals),
        "longest_run_of_gaps_over_25ms": longest_gap_run(gaps),
        "frame_received_seconds": frames,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key != "frame_received_seconds"}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/rbirds"))
    parser.add_argument("--render", choices=["default", "braille", "kitty", "sixel", "blocks", "sextants"], default="default")
    parser.add_argument("--frames", type=int, default=1200)
    parser.add_argument("--cols", type=int, default=160)
    parser.add_argument("--rows", type=int, default=50)
    parser.add_argument("--cell-width", type=int, default=8)
    parser.add_argument("--cell-height", type=int, default=16)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--warmup", type=float, default=2)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--pid-file", type=Path)
    parser.add_argument("--trace", type=Path)
    parser.add_argument("--input-at", type=float)
    parser.add_argument("--input", default="h", help="text to inject (default: h toggles the panel)")
    parser.add_argument("--resize-at", type=float)
    parser.add_argument("--resize-cols", type=int, default=120)
    parser.add_argument("--resize-rows", type=int, default=40)
    parser.add_argument("--pause-at", type=float)
    parser.add_argument("--pause-for", type=float, default=.25)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.arguments[:1] == ["--"]:
        args.arguments.pop(0)
    if args.frames < 2 or min(args.cols, args.rows, args.cell_width, args.cell_height,
                             args.resize_cols, args.resize_rows) < 1:
        parser.error("frames must be at least 2 and dimensions must be positive")
    if max(args.cols * args.cell_width, args.rows * args.cell_height,
           args.resize_cols * args.cell_width, args.resize_rows * args.cell_height) > 65535:
        parser.error("pixel dimensions must fit the terminal's 16-bit size fields")
    if args.timeout <= 0 or args.warmup < 0 or args.pause_for < 0:
        parser.error("timeout must be positive; warmup and pause duration cannot be negative")
    run(args)


if __name__ == "__main__":
    main()
