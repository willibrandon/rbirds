#!/usr/bin/env python3
"""Measure rbirds, a macOS terminal and selected helper processes during playback."""

import argparse
import ctypes
import hashlib
import json
import math
import os
import platform
import shutil
import subprocess
import threading
import time
from pathlib import Path


class Usage(ctypes.Structure):
    # sys/resource.h, rusage_info_v0. CPU counters are Mach absolute ticks.
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "user", "system", "idle_wakes", "interrupt_wakes", "pageins",
            "wired", "resident", "physical", "start", "exit")]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


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
                        help="delay before the optional interval sample (default: 3)")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ["--"]:
        args.command.pop(0)
    if platform.system() != "Darwin" or not args.command:
        parser.error("requires macOS and a command after --")
    if not math.isfinite(args.warmup_seconds) or args.warmup_seconds < 0:
        parser.error("warmup seconds must be finite and nonnegative")
    if args.sample_seconds is not None and (
            not math.isfinite(args.sample_seconds) or args.sample_seconds <= 0):
        parser.error("sample seconds must be finite and positive")
    if (any(pid <= 0 or pid == args.terminal_pid for pid in args.helper_pid)
            or len(set(args.helper_pid)) != len(args.helper_pid)):
        parser.error("helper PIDs must be distinct positive processes other than the terminal")
    binary = shutil.which(args.command[0])
    if not binary:
        parser.error(f"cannot find executable: {args.command[0]}")

    libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    libproc.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    libproc.proc_pid_rusage.restype = ctypes.c_int
    system = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
    system.mach_timebase_info.argtypes = [ctypes.POINTER(Timebase)]
    system.mach_timebase_info.restype = ctypes.c_int
    timebase = Timebase()
    if system.mach_timebase_info(ctypes.byref(timebase)) != 0 or not timebase.denom:
        raise RuntimeError("cannot read Mach timebase")
    tick_seconds = timebase.numer / timebase.denom / 1e9

    def usage(pid):
        result = Usage()
        if libproc.proc_pid_rusage(pid, 0, ctypes.byref(result)) != 0:
            raise OSError(ctypes.get_errno(), "proc_pid_rusage", str(pid))
        return result

    def cpu_delta(before, after):
        if before.exit or after.exit:
            raise RuntimeError("selected process exited during the sample")
        if before.start != after.start:
            raise RuntimeError("process restarted or PID was reused")
        ticks = after.user + after.system - before.user - before.system
        if ticks < 0:
            raise RuntimeError("CPU counter moved backwards")
        return ticks * tick_seconds

    def helper_sample(before, wall):
        samples = []
        for pid, first in before.items():
            result = {"pid": pid}
            try:
                cpu = cpu_delta(first, usage(pid))
                result.update(cpu_seconds=cpu, cpu_ms_per_second=cpu / wall * 1000)
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
    own = usage(os.getpid())
    observed = (own.user + own.system) * tick_seconds
    if abs(observed - time.process_time()) > .05:
        raise RuntimeError("CPU counter units disagree with process_time")

    terminal_name = subprocess.check_output(
        ["ps", "-p", str(args.terminal_pid), "-o", "comm="], text=True).strip()
    helper_names = {pid: subprocess.check_output(
        ["ps", "-p", str(pid), "-o", "comm="], text=True).strip()
        for pid in args.helper_pid}
    helpers_before = {pid: usage(pid) for pid in args.helper_pid}
    before = usage(args.terminal_pid)
    started = time.monotonic()
    process = subprocess.Popen(args.command)
    stopped = threading.Event()
    interval = {}

    def sample_interval():
        try:
            if stopped.wait(args.warmup_seconds):
                raise RuntimeError("child exited during warmup")
            app_before = usage(process.pid)
            term_before = usage(args.terminal_pid)
            helper_before = {pid: usage(pid) for pid in args.helper_pid}
            sample_start = time.monotonic()
            if stopped.wait(args.sample_seconds):
                raise RuntimeError("child exited before the interval sample finished")
            app_after = usage(process.pid)
            term_after = usage(args.terminal_pid)
            sample_wall = time.monotonic() - sample_start
            interval.update({
                "start_seconds_after_launch": sample_start - started,
                "wall_seconds": sample_wall,
                "application_cpu_ms_per_second":
                    cpu_delta(app_before, app_after) / sample_wall * 1000,
                "terminal_cpu_ms_per_second":
                    cpu_delta(term_before, term_after) / sample_wall * 1000,
            })
            if args.helper_pid:
                interval["terminal_helpers"] = helper_sample(helper_before, sample_wall)
        except (OSError, RuntimeError) as error:
            interval["error"] = str(error)

    sampler = None
    if args.sample_seconds is not None:
        sampler = threading.Thread(target=sample_interval, daemon=True)
        sampler.start()
    try:
        _, status, child = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
    finally:
        if process.returncode is None:
            process.kill()
            process.wait()
        stopped.set()
        if sampler:
            sampler.join()
    wall = time.monotonic() - started
    after = usage(args.terminal_pid)
    terminal_cpu = cpu_delta(before, after)
    helpers = helper_sample(helpers_before, wall) if args.helper_pid else None
    application_cpu = child.ru_utime + child.ru_stime
    report = {
        "scope": "CPU during child lifetime, including startup; not presentation timing",
        "platform": platform.platform(), "note": args.note,
        "excluded_costs": ["compositor", "GPU", "unselected helper processes"],
        "command": args.command,
        "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
        "status": process.returncode, "wall_seconds": wall,
        "terminal_pid": args.terminal_pid, "terminal_process": terminal_name,
        "mach_timebase": [timebase.numer, timebase.denom],
        "application_cpu_seconds": application_cpu, "terminal_cpu_seconds": terminal_cpu,
        "application_cpu_ms_per_second": application_cpu / wall * 1000,
        "terminal_cpu_ms_per_second": terminal_cpu / wall * 1000,
        "terminal_idle_wakeups": after.idle_wakes - before.idle_wakes,
    }
    if helpers is not None:
        for sample in helpers["processes"]:
            sample["process"] = helper_names[sample["pid"]]
        report["terminal_helpers"] = helpers
    if args.sample_seconds is not None:
        report["interval_sample"] = interval
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(process.returncode)


if __name__ == "__main__":
    main()
