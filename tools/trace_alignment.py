"""Join completed output submissions to bounded CPU counter-read windows."""

from bisect import bisect_left, bisect_right
import json


def read_trace(path, session, pid, clock):
    with path.open() as source:
        summary = json.loads(next(source))
        samples = [json.loads(line) for line in source]
    if (not isinstance(summary, dict) or summary.get("kind") != "summary" or summary.get("version") != 2
            or summary.get("session") != session or summary.get("pid") != pid
            or summary.get("measurement_clock") != clock):
        raise ValueError("trace is not from this child, session and measurement clock")
    if summary.get("omitted") != 0 or summary.get("samples") != len(samples):
        raise ValueError("trace has missing or omitted samples")
    begin, end = summary["begin_ns"], summary["end_ns"]
    if type(begin) is not int or type(end) is not int or not 0 <= begin <= end:
        raise ValueError("invalid trace coverage")
    submitted = []
    previous = begin
    for sample in samples:
        if not isinstance(sample, dict):
            raise ValueError("invalid frame sample")
        stamp = sample.get("submitted_ns")
        if (sample.get("kind") != "frame" or type(sample.get("drawn")) is not bool
                or type(stamp) is not int or not previous <= stamp <= end):
            raise ValueError("invalid or nonmonotonic submission timestamp")
        previous = stamp
        if sample["drawn"]:
            submitted.append(stamp)
    if summary.get("drawn_frames") != len(submitted):
        raise ValueError("trace submission count does not match its summary")
    return begin, end, submitted


def align_interval(interval, trace):
    begin, end, submitted = trace
    bounds = interval["counter_read_bounds_ns"]
    first_lo, first_hi = bounds["before"]
    last_lo, last_hi = bounds["after"]
    if not begin <= first_lo <= first_hi < last_lo <= last_hi <= end:
        raise ValueError("trace does not cover the CPU counter window")
    # Each counter is observed somewhere inside its bracket. Treat events
    # equal to a bracket edge conservatively too, including clock rounding.
    least = bisect_left(submitted, last_lo) - bisect_right(submitted, first_hi)
    most = bisect_right(submitted, last_hi) - bisect_left(submitted, first_lo)
    result = {"submitted_frames_min": least, "submitted_frames_max": most}
    if least > 0:
        cpu = {"application": interval["application_cpu_seconds"],
               "terminal": interval["terminal_cpu_seconds"]}
        if "terminal_helpers" in interval:
            cpu["helpers"] = interval["terminal_helpers"]["cpu_seconds"]
        cpu["combined"] = sum(cpu.values())
        result["cpu_ms_per_submitted_frame"] = {
            name: [seconds * 1000 / most, seconds * 1000 / least]
            for name, seconds in cpu.items()}
    return result
