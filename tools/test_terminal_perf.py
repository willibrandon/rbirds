#!/usr/bin/env python3
"""Correctness checks for native counters, not performance or display budgets."""

import importlib.util
import json
import math
import os
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch
from pathlib import Path

from process_usage import Counters, Snapshot, cpu_delta, linux_snapshot


ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('terminal_perf', ROOT / 'tools/terminal-perf.py')
terminal_perf = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(terminal_perf)


class SamplingDeadline(unittest.TestCase):
    def test_early_timeouts_do_not_shorten_the_sample(self):
        stopped = Mock()
        stopped.is_set.return_value = False
        stopped.wait.return_value = False
        with patch.object(terminal_perf.time, 'perf_counter',
                          side_effect=[10.0, 10.187, 10.1995, 10.2]):
            self.assertFalse(terminal_perf.wait_until(stopped, 10.2))
        waits = [call.args[0] for call in stopped.wait.call_args_list]
        self.assertEqual(len(waits), 3)
        self.assertAlmostEqual(waits[0], .2)
        self.assertAlmostEqual(waits[1], .013)
        self.assertEqual(waits[2], .001)

    def test_child_exit_interrupts_the_remaining_wait(self):
        stopped = Mock()
        stopped.is_set.return_value = False
        stopped.wait.side_effect = [False, True]
        with patch.object(terminal_perf.time, 'perf_counter', side_effect=[10.0, 10.187]):
            self.assertTrue(terminal_perf.wait_until(stopped, 10.2))
        self.assertEqual(stopped.wait.call_count, 2)

    def test_already_stopped_child_never_starts_a_sample(self):
        stopped = Mock()
        stopped.is_set.return_value = True
        self.assertTrue(terminal_perf.wait_until(stopped, 10.2))
        stopped.wait.assert_not_called()


class NativeCounters(unittest.TestCase):
    def test_native_cpu_matches_independent_process_clock(self):
        with Counters() as counters:
            before = counters.snapshot(os.getpid())
            start = time.process_time()
            while time.process_time() - start < .15:
                pass
            elapsed = time.process_time() - start
            after = counters.snapshot(os.getpid())
            observed = cpu_delta(before, after)
            self.assertAlmostEqual(observed, elapsed, delta=.05)
            self.assertTrue(counters.name(os.getpid()))
            print(json.dumps({**counters.metadata, "process_clock_seconds": elapsed,
                              "native_counter_seconds": observed}), flush=True)

    def test_short_child_retains_exit_status_and_lifetime_cpu(self):
        with Counters() as counters:
            process = subprocess.Popen([sys.executable, '-c', 'raise SystemExit(7)'])
            try:
                # Exercise opening an already-exited Windows child while Popen
                # retains its handle; do not reap it through poll/wait on Unix.
                time.sleep(.2)
                counters.prepare_child(process)
                seconds = counters.wait(process)
                self.assertEqual(process.returncode, 7)
                self.assertGreaterEqual(seconds, 0)
                self.assertTrue(math.isfinite(seconds))
            finally:
                if process.returncode is None:
                    process.kill()
                    process.wait()

    def test_linux_names_and_descendant_counters_do_not_shift_cpu_fields(self):
        fields = ['S'] + ['0'] * 19
        fields[11], fields[12], fields[13], fields[14], fields[19] = (
            '125', '75', '999999', '999999', '800')
        stat = '42 (odd ) process\n(name) ' + ' '.join(fields)
        sample = linux_snapshot(stat, 100)
        self.assertEqual(sample, Snapshot((42, 800), 2, True))
        fields[0] = 'Z'
        self.assertFalse(linux_snapshot('42 (gone) ' + ' '.join(fields), 100).alive)
        with self.assertRaises(RuntimeError):
            linux_snapshot('42 (incomplete) S 0', 100)

    def test_invalid_process_transitions_never_produce_cpu_deltas(self):
        before = Snapshot((10, 100), 1, True)
        for after in [Snapshot((10, 101), 2, True), Snapshot((11, 100), 2, True),
                      Snapshot((10, 100), 2, False), Snapshot((10, 100), .5, True)]:
            with self.subTest(after=after), self.assertRaises(RuntimeError):
                cpu_delta(before, after)


class Reports(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='rbirds-counters-')
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name)

    def sleeper(self):
        process = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
        def cleanup():
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
        self.addCleanup(cleanup)
        return process

    def run_report(self, child, options=(), terminal=None):
        output = self.path / 'report.json'
        command = [sys.executable, str(ROOT / 'tools/terminal-perf.py'),
                   '--output', str(output), '--terminal-pid', str(terminal or os.getpid()),
                   *options, '--', sys.executable, '-c', child]
        result = subprocess.run(command, capture_output=True, text=True, timeout=20)
        self.assertTrue(output.exists(), result.stderr)
        report = json.loads(output.read_text())
        self.assertEqual(len(report['binary_sha256']), 64)
        return result, report

    def test_lifetime_interval_and_helpers_use_native_counters(self):
        helper = self.sleeper()
        child = ("import time; start=time.process_time(); "
                 "exec('while time.process_time()-start < .1: pass'); "
                 "time.sleep(.7); print(time.process_time())")
        result, report = self.run_report(child, ['--helper-pid', str(helper.pid),
                                                '--warmup-seconds', '.05',
                                                '--sample-seconds', '.2'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(report['measurement_valid'])
        self.assertEqual(report['status'], 0)
        self.assertAlmostEqual(report['application_cpu_seconds'], float(result.stdout), delta=.1)
        interval = report['interval_sample']
        self.assertGreaterEqual(interval['wall_seconds'], .2)
        print(json.dumps({'sample_wall_seconds': interval['wall_seconds'],
                          **{key: report[key] for key in
                             ['python', 'elapsed_clock', 'elapsed_clock_resolution_seconds']}}),
              flush=True)
        for sample in [report, interval]:
            for key in ['application_cpu_ms_per_second', 'terminal_cpu_ms_per_second']:
                self.assertGreaterEqual(sample[key], 0)
                self.assertTrue(math.isfinite(sample[key]))
            helpers = sample['terminal_helpers']
            self.assertEqual(helpers['processes'][0]['pid'], helper.pid)
            self.assertEqual(helpers['cpu_seconds'], helpers['processes'][0]['cpu_seconds'])

    def test_incomplete_interval_is_retained_and_fails_the_command(self):
        result, report = self.run_report('pass', ['--warmup-seconds', '10',
                                                 '--sample-seconds', '20'])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(report['status'], 0)
        self.assertFalse(report['measurement_valid'])
        self.assertIn('error', report['interval_sample'])

    def test_repeated_windows_measure_the_same_processes_with_the_requested_gap(self):
        helper = self.sleeper()
        result, report = self.run_report('import time; time.sleep(1.5)', [
            '--helper-pid', str(helper.pid), '--warmup-seconds', '.05',
            '--sample-seconds', '.2', '--sample-count', '3', '--sample-gap-seconds', '.1'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(report['measurement_valid'])
        self.assertNotIn('interval_sample', report)
        self.assertEqual(report['requested_interval_samples'], 3)
        self.assertEqual(report['sample_gap_seconds'], .1)
        self.assertEqual(len(report['interval_samples']), 3)
        previous_end = None
        for interval in report['interval_samples']:
            self.assertGreaterEqual(interval['wall_seconds'], .2)
            start = interval['start_seconds_after_launch']
            if previous_end is not None:
                self.assertGreaterEqual(start - previous_end, .1)
            previous_end = start + interval['wall_seconds']
            for key in ['application_cpu_ms_per_second', 'terminal_cpu_ms_per_second']:
                self.assertGreaterEqual(interval[key], 0)
                self.assertTrue(math.isfinite(interval[key]))
            self.assertEqual(interval['terminal_helpers']['processes'][0]['pid'], helper.pid)
            self.assertIn('cpu_seconds', interval['terminal_helpers'])

    def test_exit_between_windows_preserves_completed_samples_and_fails_the_command(self):
        result, report = self.run_report('import time; time.sleep(.7)', [
            '--warmup-seconds', '0', '--sample-seconds', '.1',
            '--sample-count', '3', '--sample-gap-seconds', '5'])
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(report['status'], 0)
        self.assertFalse(report['measurement_valid'])
        self.assertEqual(report['requested_interval_samples'], 3)
        self.assertEqual(len(report['interval_samples']), 2)
        first, failed = report['interval_samples']
        self.assertGreaterEqual(first['wall_seconds'], .1)
        self.assertNotIn('error', first)
        self.assertIn('child exited between interval samples', failed['error'])
        self.assertNotIn('application_cpu_ms_per_second', failed)

    def test_invalid_repeat_options_do_not_launch_the_child(self):
        for options in [
                ['--sample-count', '0'], ['--sample-count', '-1'],
                ['--sample-gap-seconds', '-1'], ['--sample-gap-seconds', 'nan'],
                ['--sample-gap-seconds', 'inf'], ['--sample-count', '2'],
                ['--sample-gap-seconds', '1']]:
            with self.subTest(options=options):
                output = self.path / 'invalid.json'
                result = subprocess.run([
                    sys.executable, str(ROOT / 'tools/terminal-perf.py'),
                    '--output', str(output), *options, '--', sys.executable,
                    '-c', 'print("child launched")'], capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stdout, '')
                self.assertFalse(output.exists())

    def test_child_failure_is_preserved(self):
        result, report = self.run_report('raise SystemExit(7)')
        self.assertEqual(result.returncode, 7)
        self.assertEqual(report['status'], 7)
        self.assertTrue(report['measurement_valid'])

    def test_exited_helper_cannot_be_reported_as_zero_cpu(self):
        helper = self.sleeper()
        # The child starts only after the harness samples the helper. Kill only
        # this test-owned process, then leave time for the interval to finish.
        child = (f'import os,signal,time; os.kill({helper.pid},signal.SIGTERM); '
                 'time.sleep(.5)')
        result, report = self.run_report(child, ['--helper-pid', str(helper.pid),
                                                '--warmup-seconds', '0',
                                                '--sample-seconds', '.1'])
        self.assertEqual(result.returncode, 1)
        self.assertFalse(report['measurement_valid'])
        self.assertNotIn('cpu_seconds', report['terminal_helpers'])
        self.assertIn('error', report['terminal_helpers']['processes'][0])

    def test_exited_terminal_retains_an_invalid_report(self):
        terminal = self.sleeper()
        child = (f'import os,signal,time; os.kill({terminal.pid},signal.SIGTERM); '
                 'time.sleep(.1)')
        result, report = self.run_report(child, terminal=terminal.pid)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(report['measurement_valid'])
        self.assertIn('terminal_error', report)
        self.assertNotIn('terminal_cpu_seconds', report)


if __name__ == '__main__':
    unittest.main(verbosity=2)
