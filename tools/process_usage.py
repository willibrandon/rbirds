"""Native CPU and memory observations for terminal-perf.py (standard library only)."""

import ctypes
import os
import platform
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass(frozen=True)
class Snapshot:
    identity: tuple
    cpu_seconds: float
    alive: bool
    idle_wakes: Optional[int] = None
    memory: dict = field(default_factory=dict)


def cpu_delta(before, after):
    if not before.alive or not after.alive:
        raise RuntimeError("selected process exited during the sample")
    if before.identity != after.identity:
        raise RuntimeError("process restarted or PID was reused")
    seconds = after.cpu_seconds - before.cpu_seconds
    if seconds < 0:
        raise RuntimeError("CPU counter moved backwards")
    return seconds


def memory_change(before, after):
    # Memory can fall; only process identity/liveness and CPU must be monotonic.
    cpu_delta(before, after)
    if before.memory.keys() != after.memory.keys():
        raise RuntimeError("memory metrics changed during the sample")
    return {name: {"before_bytes": value, "after_bytes": after.memory[name],
                   "change_bytes": after.memory[name] - value}
            for name, value in before.memory.items()}


def linux_snapshot(stat, ticks_per_second, page_size):
    # comm may contain spaces, newlines and ')'. Only the last ')' ends it.
    pid, _, tail = stat.partition(' (')
    _, close, fields = tail.rpartition(')')
    values = fields.split()
    if not close or len(values) < 22:
        raise RuntimeError("incomplete /proc PID stat record")
    # These are fields 14, 15 and 22; values starts at field 3 (state).
    return Snapshot((int(pid), int(values[19])),
                    (int(values[11]) + int(values[12])) / ticks_per_second,
                    values[0] not in ('Z', 'X', 'x'),
                    memory={"resident": int(values[21]) * page_size})


class MachUsage(ctypes.Structure):
    # sys/resource.h, rusage_info_v0. CPU counters are Mach absolute ticks.
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64) for name in (
            "user", "system", "idle_wakes", "interrupt_wakes", "pageins",
            "wired", "resident", "physical", "start", "exit")]


class WindowsMemory(ctypes.Structure):
    # PROCESS_MEMORY_COUNTERS_EX: DWORD remains 32 bits on 64-bit Windows.
    _fields_ = [("size", ctypes.c_uint32), ("faults", ctypes.c_uint32)] + [
        (name, ctypes.c_size_t) for name in (
            "peak_working_set", "working_set", "peak_paged_pool", "paged_pool",
            "peak_nonpaged_pool", "nonpaged_pool", "pagefile", "peak_pagefile", "private")]


class Counters:
    def __init__(self):
        self.platform = platform.system()
        self.handles = {}
        if self.platform == "Darwin":
            self.lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
            self.lib.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
            self.lib.proc_pid_rusage.restype = ctypes.c_int
            system = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
            system.sysctlbyname.argtypes = [ctypes.c_char_p, ctypes.c_void_p,
                                           ctypes.POINTER(ctypes.c_size_t),
                                           ctypes.c_void_p, ctypes.c_size_t]
            system.sysctlbyname.restype = ctypes.c_int
            frequency = ctypes.c_uint64()
            size = ctypes.c_size_t(ctypes.sizeof(frequency))
            if system.sysctlbyname(b"hw.tbfrequency", ctypes.byref(frequency),
                                   ctypes.byref(size), None, 0) != 0:
                raise OSError(ctypes.get_errno(), "sysctl hw.tbfrequency")
            if size.value != ctypes.sizeof(frequency) or not frequency.value:
                raise RuntimeError("invalid hardware timebase frequency")
            # proc_pid_rusage returns kernel ticks even under Rosetta, where
            # mach_timebase_info instead describes translated nanosecond ticks.
            self.tick_seconds = 1 / frequency.value
            self.metadata = {"cpu_counter": "Mach ticks",
                             "hardware_timebase_frequency_hz": frequency.value,
                             "memory_counter": "rusage_info_v0 resident and physical footprint"}
        elif self.platform == "Linux":
            self.ticks_per_second = os.sysconf("SC_CLK_TCK")
            self.page_size = os.sysconf("SC_PAGE_SIZE")
            self.metadata = {"cpu_counter": "/proc PID stat",
                             "clock_ticks_per_second": self.ticks_per_second,
                             "memory_counter": "/proc PID stat resident pages",
                             "memory_page_size_bytes": self.page_size}
        elif self.platform == "Windows":
            from ctypes import wintypes as w
            self.filetime = w.FILETIME
            self.lib = ctypes.WinDLL("kernel32", use_last_error=True)
            signatures = {
                "OpenProcess": ([w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
                "GetProcessTimes": ([w.HANDLE] + [ctypes.POINTER(w.FILETIME)] * 4, w.BOOL),
                "WaitForSingleObject": ([w.HANDLE, w.DWORD], w.DWORD),
                "CloseHandle": ([w.HANDLE], w.BOOL),
                "K32GetProcessMemoryInfo":
                    ([w.HANDLE, ctypes.POINTER(WindowsMemory), w.DWORD], w.BOOL),
                "QueryFullProcessImageNameW":
                    ([w.HANDLE, w.DWORD, w.LPWSTR, ctypes.POINTER(w.DWORD)], w.BOOL),
            }
            for name, (args, result) in signatures.items():
                function = getattr(self.lib, name)
                function.argtypes, function.restype = args, result
            self.metadata = {"cpu_counter": "GetProcessTimes (100 ns)",
                             "memory_counter": "PROCESS_MEMORY_COUNTERS_EX working set and private commit"}
        else:
            raise RuntimeError("requires native Windows, Linux or macOS process counters")

    def __enter__(self):
        return self

    def __exit__(self, *unused):
        for handle in self.handles.values():
            self.lib.CloseHandle(handle)
        self.handles.clear()

    def _handle(self, pid):
        if pid not in self.handles:
            # Query-only access plus SYNCHRONIZE to distinguish exited processes.
            handle = self.lib.OpenProcess(0x1000 | 0x100000, False, pid)
            if not handle:
                raise ctypes.WinError(ctypes.get_last_error())
            self.handles[pid] = handle
        return self.handles[pid]

    def snapshot(self, pid):
        if self.platform == "Darwin":
            result = MachUsage()
            if self.lib.proc_pid_rusage(pid, 0, ctypes.byref(result)) != 0:
                raise OSError(ctypes.get_errno(), "proc_pid_rusage", str(pid))
            return Snapshot((pid, result.start),
                            (result.user + result.system) * self.tick_seconds,
                            not result.exit, result.idle_wakes,
                            {"resident": result.resident, "physical_footprint": result.physical})
        if self.platform == "Linux":
            stat = Path(f"/proc/{pid}/stat").read_text(errors="replace")
            return linux_snapshot(stat, self.ticks_per_second, self.page_size)
        handle = self._handle(pid)
        created, exited, kernel, user = (self.filetime() for _ in range(4))
        if not self.lib.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                       ctypes.byref(kernel), ctypes.byref(user)):
            raise ctypes.WinError(ctypes.get_last_error())
        state = self.lib.WaitForSingleObject(handle, 0)
        if state not in (0, 258):  # WAIT_OBJECT_0, WAIT_TIMEOUT
            raise ctypes.WinError(ctypes.get_last_error())
        memory = {}
        if state == 258:
            usage = WindowsMemory()
            usage.size = ctypes.sizeof(usage)
            if not self.lib.K32GetProcessMemoryInfo(handle, ctypes.byref(usage), usage.size):
                raise ctypes.WinError(ctypes.get_last_error())
            memory = {"working_set": usage.working_set, "private_commit": usage.private}
        def ticks(value):
            return (value.dwHighDateTime << 32) | value.dwLowDateTime
        return Snapshot((pid, ticks(created)), (ticks(kernel) + ticks(user)) / 1e7,
                        state == 258, memory=memory)

    def name(self, pid):
        if self.platform == "Darwin":
            return subprocess.check_output(
                ["ps", "-p", str(pid), "-o", "comm="], text=True).strip()
        if self.platform == "Linux":
            return os.readlink(f"/proc/{pid}/exe")
        from ctypes import wintypes as w
        buffer = ctypes.create_unicode_buffer(32768)
        size = w.DWORD(len(buffer))
        if not self.lib.QueryFullProcessImageNameW(self._handle(pid), 0, buffer,
                                                 ctypes.byref(size)):
            raise ctypes.WinError(ctypes.get_last_error())
        return buffer.value

    def prepare_child(self, process):
        if self.platform == "Windows":
            # Popen retains the process object even if a very short child has
            # already exited. Keep our own query handle through wait and reporting.
            self._handle(process.pid)

    def wait(self, process):
        if self.platform == "Windows":
            process.wait()
            return self.snapshot(process.pid).cpu_seconds
        _, status, child = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
        return child.ru_utime + child.ru_stime
