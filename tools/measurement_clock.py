"""Named system clocks shared with rbirds traces, independent of Python's timer epoch."""

import ctypes
import platform
import time


class MeasurementClock:
    def __init__(self):
        if platform.system() == "Windows":
            self.name = "QueryPerformanceCounter"
            self.lib = ctypes.WinDLL("kernel32", use_last_error=True)
            for name in ("QueryPerformanceCounter", "QueryPerformanceFrequency"):
                function = getattr(self.lib, name)
                function.argtypes = [ctypes.POINTER(ctypes.c_int64)]
                function.restype = ctypes.c_int32
            frequency = ctypes.c_int64()
            if not self.lib.QueryPerformanceFrequency(ctypes.byref(frequency)):
                raise ctypes.WinError(ctypes.get_last_error())
            self.frequency = frequency.value
            if self.frequency <= 0:
                raise RuntimeError("invalid QPC frequency")
        elif platform.system() in ("Darwin", "Linux"):
            self.name = "clock_gettime(CLOCK_MONOTONIC)"
        else:
            raise RuntimeError("measurement clock requires Windows, Linux or macOS")

    def now_ns(self):
        if self.name == "QueryPerformanceCounter":
            ticks = ctypes.c_int64()
            if not self.lib.QueryPerformanceCounter(ctypes.byref(ticks)):
                raise ctypes.WinError(ctypes.get_last_error())
            value = ticks.value * 1_000_000_000 // self.frequency
        else:
            value = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
        if not 0 <= value <= 0xffff_ffff_ffff_ffff:
            raise RuntimeError("measurement clock is outside the trace range")
        return value
