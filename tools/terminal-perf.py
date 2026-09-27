#!/usr/bin/env python3
"""Measure rbirds and one macOS terminal process during a real terminal session."""

import argparse
import ctypes
import hashlib
import json
import os
import platform
import shutil
import subprocess
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
    parser.add_argument("--note", default="",
                        help="terminal/version, visibility, power mode, other activity")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ["--"]:
        args.command.pop(0)
    if platform.system() != "Darwin" or not args.command:
        parser.error("requires macOS and a command after --")
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

    # Check the units against an independent counter in this process. This
    # catches treating ARM ticks as nanoseconds (Intel often has a 1:1 ratio).
    own = usage(os.getpid())
    observed = (own.user + own.system) * tick_seconds
    if abs(observed - time.process_time()) > .05:
        raise RuntimeError("CPU counter units disagree with process_time")

    terminal_name = subprocess.check_output(
        ["ps", "-p", str(args.terminal_pid), "-o", "comm="], text=True).strip()
    before = usage(args.terminal_pid)
    started = time.monotonic()
    process = subprocess.Popen(args.command)
    try:
        _, status, child = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
    finally:
        if process.returncode is None:
            process.kill()
            process.wait()
    wall = time.monotonic() - started
    after = usage(args.terminal_pid)
    terminal_cpu = (after.user + after.system - before.user - before.system) * tick_seconds
    application_cpu = child.ru_utime + child.ru_stime
    report = {
        "scope": "CPU during child lifetime, including startup; not presentation timing",
        "platform": platform.platform(), "note": args.note,
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
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    raise SystemExit(process.returncode)


if __name__ == "__main__":
    main()
