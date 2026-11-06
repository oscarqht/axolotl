"""Regression checks for validity, isolation and native-run failure handling."""
import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch, MagicMock

from . import mac
from .protocol import load_quality, native_digest, validate_metrics

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('benchmark_cli', ROOT/'scripts/benchmark.py')
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)


class ProtocolTests(unittest.TestCase):
    def test_host_load_is_guarded(self):
        self.assertFalse(load_quality('{ 83.6 70.0 60.0 }', 10)['comparable'])
        self.assertTrue(load_quality('{ 1.2 2.0 3.0 }', 10)['comparable'])
        self.assertFalse(load_quality('', 10)['comparable'])
        self.assertFalse(load_quality('1.0', 0)['comparable'])

    def test_missing_required_is_unavailable_not_zero(self):
        row = {'metrics': [{'id': 'a', 'status': 'available', 'samples': [0]}]}
        missing = validate_metrics(row, {'required': {'actions': ['a', 'b']}}, 'actions')
        self.assertEqual(missing, ['b'])
        self.assertEqual(row['metrics'][-1]['samples'], [])
        self.assertTrue(row['metrics'][-1]['required'])

    def test_stimulus_hash_detects_changed_page(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            page = root/'page.html'
            page.write_text('one')
            manifest = {'native_sources': ['page.html']}
            before = native_digest(root, manifest)
            page.write_text('two')
            self.assertNotEqual(before, native_digest(root, manifest))

    def test_snapshot_failure_is_not_zero_resources(self):
        collector = mac.Resources.__new__(mac.Resources)
        collector.pid = 42
        with patch.object(mac, 'command', return_value=''):
            with self.assertRaisesRegex(RuntimeError, 'missing'):
                collector.snapshot()

    def test_continuous_stdout_does_not_bypass_timeout(self):
        with TemporaryDirectory() as temp:
            native = mac.NativeRun('/tmp/not-an-app', temp, {}, 1)
            native.stdout = Path(temp)/'stdout'
            native.stdout.write_text('AXOLOTL_BENCH {"type":"quality"}\n'*10)
            native.launcher = SimpleNamespace(poll=lambda: None)
            with patch.object(mac.time, 'monotonic', side_effect=[0, 0, 2]):
                messages = native.messages()
                self.assertEqual(next(messages)['type'], 'quality')
                with self.assertRaises(TimeoutError): next(messages)

    def test_partial_stdout_record_is_buffered(self):
        native = mac.NativeRun('/tmp/app', '/tmp/output', {}, 10)
        native.stdout = MagicMock()
        stream = native.stdout.open.return_value.__enter__.return_value
        stream.readline.side_effect = ['AXOLOTL_BENCH {"type":', '"quality"}\n', '']
        native.launcher = SimpleNamespace(poll=lambda: 0)
        self.assertEqual(list(native.messages()), [{'type': 'quality'}])

    def test_truncated_protocol_record_is_rejected(self):
        native = mac.NativeRun('/tmp/app', '/tmp/output', {}, 10)
        native.stdout = MagicMock()
        stream = native.stdout.open.return_value.__enter__.return_value
        stream.readline.side_effect = ['AXOLOTL_BENCH {"type":', '']
        native.launcher = SimpleNamespace(poll=lambda: 0)
        with self.assertRaisesRegex(ValueError, 'truncated'):
            list(native.messages())

    def test_cleanup_matches_only_unique_bundle(self):
        native = mac.NativeRun('/tmp/input', '/tmp/output', {}, 1)
        native.executable = Path('/tmp/owned/Axolotl')
        with patch.object(mac, 'command', return_value='11 /Applications/Axolotl\n22 /tmp/owned/Axolotl\n33 /tmp/other/Axolotl'):
            self.assertEqual(native.owned_pids(), [22])

    def run_messages(self, messages, resource=None):
        class FakeNative:
            pid = 42
            def __init__(self, *args): pass
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def messages(self): return iter(messages)
            def acknowledge(self, *args): pass
        if resource is None:
            class resource:
                def __init__(self, *args): pass
                def start(self): pass
                def stop(self): raise RuntimeError('sampler failed')
        args = SimpleNamespace(suite='actions', iterations=1, warmup=0, energy_seconds=3, timeout=10, battery=False, power=False)
        manifest = {'suite_version': 1, 'required': {'actions': []}}
        with patch.object(mac, 'NativeRun', FakeNative), patch.object(mac, 'Resources', resource):
            return cli.run_one(Path('/tmp/app'), Path('/tmp/out'), args, 'small', 1, manifest, 'expected')

    def test_stale_binary_rejected(self):
        row = self.run_messages([{'type': 'run_start', 'native_sha256': 'old', 'suite_version': 1}])
        self.assertEqual(row['status'], 'failed')
        self.assertIn('rebuild', row['error'])

    def test_unmatched_phase_rejected(self):
        row = self.run_messages([{'type': 'phase_end', 'id': 'web'}])
        self.assertIn('unmatched', row['error'])

    def test_collectors_fail_without_fabricated_samples(self):
        row = self.run_messages([{'type': 'phase_start', 'id': 'actions'},
                                 {'type': 'phase_end', 'id': 'actions'},
                                 {'type': 'run_end', 'status': 'passed'},
                                 {'type': 'smoke', 'result': 'passed'}])
        self.assertEqual(row['status'], 'passed')
        self.assertEqual(row['metrics'], [])
        self.assertIn('sampler failed', row['resources'][0]['error'])

    def test_failed_cleanup_does_not_hide_original_error(self):
        row = self.run_messages([{'type': 'phase_start', 'id': 'actions'},
                                 {'type': 'phase_start', 'id': 'web'}])
        self.assertIn('overlapping', row['error'])
        self.assertIn('sampler failed', row['resources'][0]['error'])


if __name__ == '__main__': unittest.main()
