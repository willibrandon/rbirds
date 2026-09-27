#!/usr/bin/env python3
"""Summarize RBIRDS_TRACE files from live runs on any supported platform."""

import argparse
import json
import math
from pathlib import Path


def distribution(values):
    values = sorted(values)
    if not values:
        return None
    return {name: values[max(0, math.ceil(len(values) * quantile) - 1)]
            for name, quantile in [("p50", .5), ("p95", .95), ("p99", .99), ("p999", .999), ("max", 1)]}


def summarize(path, warmup):
    with path.open() as source:
        summary = json.loads(next(source))
        frames = [json.loads(line) for line in source]
    if summary.get("kind") != "summary" or summary.get("version") != 1:
        raise ValueError(f"{path}: unsupported or incomplete trace")
    if len(frames) != summary["samples"] or not frames:
        raise ValueError(f"{path}: missing frame samples")
    intervals = [(b["start_us"] - a["start_us"]) / 1000
                 for a, b in zip(frames, frames[1:]) if a["start_us"] >= warmup * 1e6]
    steady = [frame for frame in frames if frame["start_us"] >= warmup * 1e6]
    longest = current = 0
    for interval in intervals:
        current = current + 1 if interval > 25 else 0
        longest = max(longest, current)
    return {
        "file": str(path), "scope": "application and output transport, not screen presentation",
        "renderer": summary["renderer"], "viewport": summary["viewport"],
        "live_cpu_ms_per_second": summary["cpu_us"] / summary["wall_us"] * 1000,
        "live_wall_seconds": summary["wall_us"] / 1e6,
        "live_cpu_ms_per_frame": summary["cpu_us"] / 1000 / (len(frames) + summary["omitted"]),
        "sampled_frames": len(frames), "omitted_frames": summary["omitted"],
        "interval_ms_after_warmup": distribution(intervals),
        "stage_ms_after_warmup": {
            stage.removesuffix("_us"): distribution([frame[stage] / 1000 for frame in steady])
            for stage in ("update_us", "compose_us", "encode_us", "flush_us", "wake_late_us")
            if stage in frames[0]
        },
        "gaps_over_25ms": sum(value > 25 for value in intervals),
        "gaps_over_50ms": sum(value > 50 for value in intervals),
        "gaps_over_100ms": sum(value > 100 for value in intervals),
        "longest_run_of_gaps_over_25ms": longest,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("traces", type=Path, nargs="+")
    parser.add_argument("--warmup", type=float, default=2)
    args = parser.parse_args()
    for path in args.traces:
        print(json.dumps(summarize(path, args.warmup), indent=2))


if __name__ == "__main__":
    main()
