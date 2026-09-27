#!/usr/bin/env python3
"""Verify the Rust clock's units and epoch against Python on the same native host."""

import json
from pathlib import Path
import subprocess
import sys
import threading
import time

from measurement_clock import MeasurementClock


def check(binary):
    clock = MeasurementClock()
    readings = []
    process = subprocess.Popen([str(Path(binary).resolve())], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    timeout = threading.Timer(15, process.kill)
    timeout.start()
    try:
        name = process.stdout.readline().strip()
        if name != clock.name:
            raise RuntimeError(f"clock names differ: Rust {name}, Python {clock.name}")
        for _ in range(20):
            before = clock.now_ns()
            process.stdin.write("read\n")
            process.stdin.flush()
            rust = int(process.stdout.readline())
            after = clock.now_ns()
            if not before <= rust <= after:
                raise RuntimeError(f"Rust clock outside the parent bracket: {before}, {rust}, {after}")
            readings.append([before, rust, after])
            time.sleep(.01)
        process.stdin.close()
        if process.wait(timeout=5) != 0:
            raise RuntimeError(process.stderr.read())
    finally:
        timeout.cancel()
        if process.poll() is None:
            process.kill()
            process.wait()
        process.stdout.close()
        process.stderr.close()
        process.stdin.close()
    print(json.dumps({"clock": clock.name, "readings_ns": readings}))


if __name__ == "__main__":
    check(sys.argv[1])
