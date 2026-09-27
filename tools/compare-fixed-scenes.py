#!/usr/bin/env python3
"""Compare CPU reports only when their recorded fixed scenes and frame ranges match."""

import argparse
import json
import math
from pathlib import Path


def compare(reports):
    if len(reports) < 2:
        raise ValueError('at least two reports are required')
    identity = None
    rows = []
    for report in reports:
        if report.get('measurement_valid') is not True or report.get('status') != 0:
            raise ValueError('every report must be valid and its child must have exited successfully')
        if report.get('trace', {}).get('simulation_clock') != 'fixed-60-hz':
            raise ValueError('elapsed-time playback cannot be compared as a fixed scene')
        scene = report.get('fixed_scene')
        if not isinstance(scene, str) or not scene:
            raise ValueError('missing fixed scene identity')
        interval = report['interval_sample']
        frames = interval['fixture_frames']
        first, last, count = (frames[key] for key in ('first', 'last', 'count'))
        if any(type(n) is not int for n in (first, last, count)) or first < 1 or count < 1 or last - first + 1 != count:
            raise ValueError('invalid fixture frame range')
        alignment = interval['trace_alignment']
        if alignment['submitted_frames_min'] != count or alignment['submitted_frames_max'] != count:
            raise ValueError('counter window does not cover exactly the fixture frame range')
        helpers = report.get('terminal_helpers', {}).get('processes', [])
        observed = (scene, first, last, report['platform'], report['measurement_clock'],
                    report['terminal_process'], sorted(helper['process'] for helper in helpers))
        if identity is not None and observed != identity:
            raise ValueError('scene, frame range, platform or selected terminal processes differ')
        identity = observed
        cpu = {'application': interval['application_cpu_seconds'],
               'terminal': interval['terminal_cpu_seconds'],
               'helpers': interval.get('terminal_helpers', {}).get('cpu_seconds', 0)}
        if any(type(value) not in (int, float) or not math.isfinite(value) or value < 0 for value in cpu.values()):
            raise ValueError('invalid CPU duration')
        cpu['combined'] = sum(cpu.values())
        wall = interval['wall_seconds']
        if type(wall) not in (int, float) or not math.isfinite(wall) or wall <= 0:
            raise ValueError('invalid interval duration')
        rows.append({'binary_sha256': report['binary_sha256'], 'command': report['command'],
                     'wall_seconds': wall, 'frames': frames,
                     'cpu_ms_per_frame': {name: value * 1000 / count for name, value in cpu.items()},
                     'cpu_ms_per_second': {name: value * 1000 / wall for name, value in cpu.items()}})
    return {'scope': 'CPU for matching recorded inputs and frame ranges, not display smoothness or proof of visual equivalence',
            'limits': ['same terminal configuration and background load still require operator control',
                       'counter windows include boundary handshake overhead',
                       'terminal decoding and display can lag submissions',
                       'separate correctness and ordinary live presentation checks are required'],
            'fixed_scene': reports[0]['fixed_scene'], 'runs': rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path, nargs='+')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    try:
        report = compare([json.loads(path.read_text()) for path in args.reports])
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    text = json.dumps(report, indent=2) + '\n'
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end='')


if __name__ == '__main__':
    main()
