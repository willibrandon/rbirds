#!/usr/bin/env python3
"""Measure CPU and memory for rbirds, its terminal and selected helpers."""

import argparse
import hashlib
import json
import math
import os
import platform
import secrets
import shutil
import subprocess
import threading
import time
from pathlib import Path

from process_usage import Counters, cpu_delta, memory_change
from measurement_clock import MeasurementClock
from trace_alignment import align_interval, read_trace


def wait_until(stopped, deadline):
    """Wait for an elapsed-time deadline, preserving early child cancellation."""
    while not stopped.is_set():
        remaining = deadline - time.perf_counter()
        if remaining <= 0:
            return False
        # Windows waits and older Python monotonic clocks can have coarser
        # resolution than the performance counter. Recheck an early timeout;
        # retain a blocking wait even for the last fraction of a millisecond.
        if stopped.wait(max(remaining, .001)):
            return True
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--terminal-pid", type=int, default=os.getppid(),
                        help="defaults to parent; launch this as the terminal's direct child")
    parser.add_argument("--helper-pid", type=int, action="append", default=[],
                        help="also sample this terminal helper; repeat for multiple stable processes")
    parser.add_argument("--note", default="",
                        help="terminal/version, visibility, power mode, other activity")
    parser.add_argument("--sample-seconds", type=float,
                        help="also measure all selected processes over an equal elapsed-time interval")
    parser.add_argument("--warmup-seconds", type=float, default=3,
                        help="delay before the first optional interval sample (default: 3)")
    parser.add_argument("--sample-count", type=int, default=1,
                        help="number of interval samples in the same run (default: 1)")
    parser.add_argument("--sample-gap-seconds", type=float, default=0,
                        help="delay between interval samples (default: 0)")
    parser.add_argument("--trace", type=Path,
                        help="record rbirds submissions and align them with CPU intervals")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ["--"]:
        args.command.pop(0)
    if not args.command:
        parser.error("requires a command after --")
    if not math.isfinite(args.warmup_seconds) or args.warmup_seconds < 0:
        parser.error("warmup seconds must be finite and nonnegative")
    if args.sample_seconds is not None and (
            not math.isfinite(args.sample_seconds) or args.sample_seconds <= 0):
        parser.error("sample seconds must be finite and positive")
    if args.sample_count < 1:
        parser.error("sample count must be positive")
    if not math.isfinite(args.sample_gap_seconds) or args.sample_gap_seconds < 0:
        parser.error("sample gap seconds must be finite and nonnegative")
    if args.sample_seconds is None and (args.sample_count != 1 or args.sample_gap_seconds != 0):
        parser.error("repeated sampling requires --sample-seconds")
    if args.trace and args.sample_seconds is None:
        parser.error("trace alignment requires --sample-seconds")
    if args.trace and args.trace.resolve() == args.output.resolve():
        parser.error("trace and CPU report must use different paths")
    pids = [args.terminal_pid, *args.helper_pid]
    if any(pid <= 0 or pid > 0x7fffffff for pid in pids) or len(set(pids)) != len(pids):
        parser.error("terminal and helper PIDs must be distinct positive process IDs")
    binary = shutil.which(args.command[0])
    if not binary:
        parser.error(f"cannot find executable: {args.command[0]}")

    with Counters() as counters:
        report = measure(args, counters, binary)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(report["status"] or (0 if report["measurement_valid"] else 1))


def measure(args, counters, binary):
    clock = MeasurementClock()
    session = secrets.randbits(64)
    environment = os.environ.copy()
    if args.trace:
        args.trace.parent.mkdir(parents=True, exist_ok=True)
        environment["RBIRDS_TRACE"] = str(args.trace.resolve())
        environment["RBIRDS_TRACE_SESSION"] = str(session)
    def helper_sample(before, wall):
        samples = []
        for pid, first in before.items():
            result = {"pid": pid}
            try:
                last = counters.snapshot(pid)
                cpu = cpu_delta(first, last)
                result.update(cpu_seconds=cpu, cpu_ms_per_second=cpu / wall * 1000,
                              memory=memory_change(first, last))
            except (OSError, RuntimeError) as error:
                result["error"] = str(error)
            samples.append(result)
        result = {"processes": samples}
        if all("error" not in sample for sample in samples):
            cpu = sum(sample["cpu_seconds"] for sample in samples)
            result.update(cpu_seconds=cpu, cpu_ms_per_second=cpu / wall * 1000)
        return result

    # Check the units against an independent counter in this process. This
    # catches treating ARM ticks as nanoseconds (Intel often has a 1:1 ratio).
    observed = counters.snapshot(os.getpid()).cpu_seconds
    if abs(observed - time.process_time()) > .05:
        raise RuntimeError("CPU counter units disagree with process_time")

    terminal_name = counters.name(args.terminal_pid)
    helper_names = {pid: counters.name(pid) for pid in args.helper_pid}
    binary_hash = hashlib.sha256(Path(binary).read_bytes()).hexdigest()
    helpers_before = {pid: counters.snapshot(pid) for pid in args.helper_pid}
    before = counters.snapshot(args.terminal_pid)
    for snapshot in [before, *helpers_before.values()]:
        if not snapshot.alive:
            raise RuntimeError("selected process already exited before launch")
    started = time.perf_counter()
    process = subprocess.Popen(args.command, env=environment)
    stopped = threading.Event()
    intervals = []

    def sample_intervals():
        for index in range(args.sample_count):
            interval = {"error": "interval sample did not complete"}
            intervals.append(interval)
            try:
                delay = args.sample_gap_seconds if index else args.warmup_seconds
                if wait_until(stopped, time.perf_counter() + delay):
                    phase = "between interval samples" if index else "during warmup"
                    raise RuntimeError(f"child exited {phase}")
                first_lo = clock.now_ns()
                app_before = counters.snapshot(process.pid)
                term_before = counters.snapshot(args.terminal_pid)
                helper_before = {pid: counters.snapshot(pid) for pid in args.helper_pid}
                first_hi = clock.now_ns()
                sample_start = time.perf_counter()
                if wait_until(stopped, sample_start + args.sample_seconds):
                    raise RuntimeError("child exited before the interval sample finished")
                last_lo = clock.now_ns()
                app_after = counters.snapshot(process.pid)
                term_after = counters.snapshot(args.terminal_pid)
                sample_wall = time.perf_counter() - sample_start
                app_cpu = cpu_delta(app_before, app_after)
                term_cpu = cpu_delta(term_before, term_after)
                interval.update({
                    "start_seconds_after_launch": sample_start - started,
                    "wall_seconds": sample_wall,
                    "application_cpu_seconds": app_cpu,
                    "terminal_cpu_seconds": term_cpu,
                    "application_cpu_ms_per_second": app_cpu / sample_wall * 1000,
                    "terminal_cpu_ms_per_second": term_cpu / sample_wall * 1000,
                    "application_memory": memory_change(app_before, app_after),
                    "terminal_memory": memory_change(term_before, term_after),
                })
                if args.helper_pid:
                    interval["terminal_helpers"] = helper_sample(helper_before, sample_wall)
                last_hi = clock.now_ns()
                if not first_lo <= first_hi < last_lo <= last_hi:
                    raise RuntimeError("measurement clock did not advance across the CPU window")
                interval["counter_read_bounds_ns"] = {
                    "before": [first_lo, first_hi], "after": [last_lo, last_hi]}
                interval.pop("error", None)
            except Exception as error:
                # Retain completed windows and the failure, then stop sampling.
                interval["error"] = f"{type(error).__name__}: {error}"
                break

    sampler = None
    try:
        counters.prepare_child(process)
        if args.sample_seconds is not None:
            sampler = threading.Thread(target=sample_intervals, daemon=True)
            sampler.start()
        application_cpu = counters.wait(process)
    finally:
        if process.returncode is None:
            process.kill()
            process.wait()
        wall = time.perf_counter() - started
        stopped.set()
        if sampler:
            sampler.join()
    report = {
        "scope": "CPU during child lifetime, including startup; not presentation timing",
        "platform": platform.platform(), "note": args.note,
        "python": platform.python_version(),
        "elapsed_clock": time.get_clock_info("perf_counter").implementation,
        "elapsed_clock_resolution_seconds": time.get_clock_info("perf_counter").resolution,
        "measurement_clock": clock.name,
        "excluded_costs": ["compositor", "GPU", "unselected helper processes"],
        "memory_scope": "process gauges at interval endpoints; not peaks, allocation totals or system pressure",
        "command": args.command,
        "binary_sha256": binary_hash,
        "status": process.returncode, "wall_seconds": wall, "application_pid": process.pid,
        "terminal_pid": args.terminal_pid, "terminal_process": terminal_name,
        "application_cpu_seconds": application_cpu,
        "application_cpu_ms_per_second": application_cpu / wall * 1000,
        **counters.metadata,
    }
    try:
        after = counters.snapshot(args.terminal_pid)
        terminal_cpu = cpu_delta(before, after)
        report.update(terminal_cpu_seconds=terminal_cpu,
                      terminal_cpu_ms_per_second=terminal_cpu / wall * 1000,
                      terminal_memory=memory_change(before, after))
        if after.idle_wakes is not None:
            report["terminal_idle_wakeups"] = after.idle_wakes - before.idle_wakes
    except (OSError, RuntimeError) as error:
        report["terminal_error"] = str(error)
    if args.helper_pid:
        helpers = helper_sample(helpers_before, wall)
        for sample in helpers["processes"]:
            sample["process"] = helper_names[sample["pid"]]
        report["terminal_helpers"] = helpers
    if args.sample_seconds is not None:
        if args.sample_count == 1:
            report["interval_sample"] = intervals[0]
        else:
            report["requested_interval_samples"] = args.sample_count
            report["sample_gap_seconds"] = args.sample_gap_seconds
            report["interval_samples"] = intervals
    report["measurement_valid"] = (
        "terminal_error" not in report
        and (args.sample_seconds is None or len(intervals) == args.sample_count)
        and all("error" not in sample for sample in intervals)
        and all("cpu_seconds" in sample["terminal_helpers"]
                for sample in [report, *intervals] if "terminal_helpers" in sample))
    if args.trace:
        report["trace"] = {"path": str(args.trace.resolve()), "session": session,
                           "scope": "completed output submissions, not displayed frames"}
        try:
            trace = read_trace(args.trace, session, process.pid, clock.name)
            for interval in intervals:
                if "error" not in interval:
                    interval["trace_alignment"] = align_interval(interval, trace)
        except (OSError, ValueError, KeyError, TypeError, StopIteration) as error:
            report["trace"]["error"] = f"{type(error).__name__}: {error}"
            report["measurement_valid"] = False
    return report


if __name__ == "__main__":
    main()
