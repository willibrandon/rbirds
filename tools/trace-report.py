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
        samples = [json.loads(line) for line in source]
    if summary.get("kind") != "summary" or summary.get("version") != 1:
        raise ValueError(f"{path}: unsupported or incomplete trace")
    if len(samples) != summary["samples"] or not samples:
        raise ValueError(f"{path}: missing frame samples")
    frames = [sample for sample in samples if sample.get("drawn", True)]
    total_drawn = summary.get("drawn_frames", len(samples) + summary["omitted"])
    # An intentional pause is not a missed animation deadline. Keep the idle
    # samples so frame intervals cannot bridge that pause.
    gaps = [(b["start_us"] - a["start_us"]) / 1000
            if a.get("drawn", True) and b.get("drawn", True) else None
            for a, b in zip(samples, samples[1:]) if a["start_us"] >= warmup * 1e6]
    intervals = [gap for gap in gaps if gap is not None]
    steady = [frame for frame in frames if frame["start_us"] >= warmup * 1e6]
    longest = current = 0
    for interval in gaps:
        current = current + 1 if interval is not None and interval > 25 else 0
        longest = max(longest, current)
    over_budget = [frame.get("drawn", True) and sum(frame[key] for key in
                       ("wake_late_us", "update_us", "compose_us", "encode_us", "flush_us"))
                   > 1e6 / 60 for frame in samples if frame["start_us"] >= warmup * 1e6]
    budget_run = longest_budget_run = 0
    for late in over_budget:
        budget_run = budget_run + 1 if late else 0
        longest_budget_run = max(longest_budget_run, budget_run)
    return {
        "file": str(path), "scope": "application and output transport, not screen presentation",
        "renderer": summary["renderer"], "viewport": summary["viewport"],
        "live_cpu_ms_per_second": summary["cpu_us"] / summary["wall_us"] * 1000,
        "live_wall_seconds": summary["wall_us"] / 1e6,
        "live_cpu_ms_per_frame": summary["cpu_us"] / 1000 / total_drawn if total_drawn else None,
        "sampled_frames": len(frames), "sampled_idle_ticks": len(samples) - len(frames),
        "omitted_ticks": summary["omitted"],
        "interval_ms_after_warmup": distribution(intervals),
        "stage_ms_after_warmup": {
            stage.removesuffix("_us"): distribution([frame[stage] / 1000 for frame in steady])
            for stage in ("update_us", "compose_us", "encode_us", "flush_us", "wake_late_us")
            if stage in samples[0]
        },
        "gaps_over_25ms": sum(value > 25 for value in intervals),
        "gaps_over_50ms": sum(value > 50 for value in intervals),
        "gaps_over_100ms": sum(value > 100 for value in intervals),
        "longest_run_of_gaps_over_25ms": longest,
        "frames_over_60hz_budget": sum(over_budget),
        "longest_run_over_60hz_budget": longest_budget_run,
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
