"""Isolated LaunchServices runs and app-owned WebKit resource samples (macOS)."""
from __future__ import annotations
import ctypes
import json
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import uuid


def command(args, default=''):
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL, timeout=10).strip()
    except (OSError, subprocess.SubprocessError):
        return default


def hardware():
    import platform
    import hashlib
    import re
    def plist_version(path):
        try:
            return plistlib.loads(Path(path).read_bytes()).get('CFBundleVersion', 'unknown')
        except (OSError, ValueError):
            return 'unknown'
    raw = command(['ioreg', '-rd1', '-c', 'IOPlatformExpertDevice'])
    identifier = re.search(r'"IOPlatformUUID"\s*=\s*"([^"]+)"', raw)
    fingerprint = hashlib.sha256(('axolotl-benchmark:'+identifier.group(1)).encode()).hexdigest() if identifier else 'unavailable'
    return {'device_fingerprint': fingerprint, 'model': command(['sysctl', '-n', 'hw.model']), 'os_version': command(['sw_vers', '-productVersion']),
            'os_build': command(['sw_vers', '-buildVersion']), 'architecture': platform.machine(),
            'logical_cpus': int(command(['sysctl', '-n', 'hw.logicalcpu'], '0')),
            'memory_bytes': int(command(['sysctl', '-n', 'hw.memsize'], '0')),
            'webkit_version': plist_version('/System/Library/Frameworks/WebKit.framework/Resources/Info.plist'),
            'safari_version': plist_version('/Applications/Safari.app/Contents/Info.plist')}


class Resources:
    def __init__(self, pid):
        self.pid = pid
        self.lib = ctypes.CDLL('/usr/lib/libproc.dylib')
        self.lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
        responsible = ctypes.CDLL('/usr/lib/libSystem.B.dylib').responsibility_get_pid_responsible_for_pid
        responsible.argtypes = [ctypes.c_int]
        responsible.restype = ctypes.c_int
        self.owner = self.coalition(pid)
        if not self.owner or responsible(pid) != pid:
            raise RuntimeError('independent app responsibility/coalition not available')
        self.samples = []
        self.stop_event = threading.Event()
        self.error = None
        self.thread = threading.Thread(target=self._loop, daemon=True)

    def coalition(self, pid):
        ids = (ctypes.c_uint64 * 16)()
        size = self.lib.proc_pidinfo(pid, 20, 0, ids, ctypes.sizeof(ids))
        return tuple(ids[:2]) if size >= 16 else None

    def snapshot(self):
        rows = {}
        for line in command(['ps', '-axo', 'pid=,rss=,time=,comm=']).splitlines():
            fields = line.split(None, 3)
            if len(fields) != 4:
                continue
            pid, rss, cpu, name = fields
            pid = int(pid)
            if pid == self.pid or ('com.apple.WebKit.' in name and self.coalition(pid) == self.owner):
                seconds = sum(float(p) * 60**i for i, p in enumerate(reversed(cpu.split(':'))))
                rows[pid] = {'cpu_seconds': seconds, 'rss_mib': int(rss)/1024, 'group': 'app' if pid == self.pid else 'webkit'}
        if self.pid not in rows:
            raise RuntimeError('app process missing from resource snapshot; ps failed or app exited')
        return {'at': time.monotonic(), 'processes': rows}

    def start(self):
        self.samples = [self.snapshot()]
        self.thread.start()

    def _loop(self):
        while not self.stop_event.wait(1):
            try:
                self.samples.append(self.snapshot())
            except Exception as error:
                self.error = str(error)
                return

    def stop(self):
        self.stop_event.set()
        self.thread.join(timeout=12)
        if self.thread.is_alive(): self.error = 'resource sampler did not stop before deadline'
        try: self.samples.append(self.snapshot())
        except Exception as error: self.error = str(error)
        elapsed = self.samples[-1]['at'] - self.samples[0]['at']
        result = {'scope': 'independent_app_and_webkit_coalition', 'duration_seconds': elapsed,
                  'sample_count': len(self.samples), 'error': self.error,
                  'cpu_accounting': '1s snapshots; exited processes may lose up to one sample interval of CPU time'}
        for group in ['app', 'webkit', 'combined']:
            cpu = 0
            started = exited = 0
            rss = []
            for sample in self.samples:
                rss.append(sum(p['rss_mib'] for p in sample['processes'].values() if group == 'combined' or p['group'] == group))
            for before, after in zip(self.samples, self.samples[1:]):
                old, new = before['processes'], after['processes']
                for pid, row in new.items():
                    if group == 'combined' or row['group'] == group:
                        cpu += max(0, row['cpu_seconds'] - old.get(pid, {'cpu_seconds': 0})['cpu_seconds'])
                        started += pid not in old
                exited += sum(group == 'combined' or old[p]['group'] == group for p in old.keys()-new.keys())
            result[group] = {'cpu_seconds_observed': cpu, 'cpu_percent_one_core': 100*cpu/elapsed if elapsed else None,
                             'rss_mib_end': rss[-1], 'rss_mib_peak_sampled': max(rss), 'started_processes': started, 'exited_processes': exited}
        return result


class NativeRun:
    def __init__(self, binary, directory, env, timeout):
        self.binary, self.directory, self.env, self.timeout = Path(binary).resolve(), Path(directory), env, timeout
        self.temp = None
        self.pid = None
        self.launcher = None

    def __enter__(self):
        self.temp = tempfile.TemporaryDirectory(prefix='axolotl-benchmark-')
        root = Path(self.temp.name)
        self.executable = root/'Axolotl Benchmark.app/Contents/MacOS/Axolotl'
        self.executable.parent.mkdir(parents=True)
        shutil.copy2(self.binary, self.executable)
        info = {'CFBundleExecutable': 'Axolotl', 'CFBundleIdentifier': f'dev.axolotl.benchmark.{uuid.uuid4().hex}',
                'CFBundleName': 'Axolotl Benchmark', 'CFBundlePackageType': 'APPL', 'NSHighResolutionCapable': True,
                'NSAppTransportSecurity': {'NSAllowsArbitraryLoadsInWebContent': True}}
        (self.executable.parent.parent/'Info.plist').write_bytes(plistlib.dumps(info))
        bundle = self.executable.parents[2]
        subprocess.run(['codesign', '--force', '--sign', '-', str(bundle)], check=True, capture_output=True, timeout=30)
        self.stdout, self.stderr = self.directory/'native.stdout.log', self.directory/'native.stderr.log'
        self.stdout.touch(); self.stderr.touch()
        self.ack = root/'ack'; self.ack.mkdir()
        args = ['open', '-W', '-n', '--stdout', str(self.stdout.resolve()), '--stderr', str(self.stderr.resolve())]
        for key, value in dict(self.env, AXOLOTL_DATA_DIR=str(root/'profile'), AXOLOTL_BENCH_ACK_DIR=str(self.ack)).items():
            args += ['--env', f'{key}={value}']
        self.launcher = subprocess.Popen(args+[str(bundle), '--args', '--smoke-test', '--smoke-benchmark'])
        return self

    def acknowledge(self, phase, start):
        if phase not in {'actions', 'web', 'energy.idle', 'energy.active'}:
            raise ValueError('unexpected native phase')
        (self.ack/f'{phase}.{"start" if start else "end"}').touch()

    def messages(self):
        deadline = time.monotonic()+self.timeout
        with self.stdout.open() as stream:
            pending = ''
            while True:
                if time.monotonic() > deadline:
                    raise TimeoutError('native benchmark exceeded its deadline')
                line = stream.readline()
                if line:
                    pending += line
                    if not pending.endswith('\n'): continue
                    line, pending = pending, ''
                    if line.startswith('AXOLOTL_BENCH '):
                        value = json.loads(line.removeprefix('AXOLOTL_BENCH '))
                        if value.get('type') == 'run_start':
                            self.pid = value['state']['pid']
                        yield value
                    elif line.startswith('AXOLOTL_SMOKE '):
                        yield {'type': 'smoke', **json.loads(line.removeprefix('AXOLOTL_SMOKE '))}
                elif self.launcher.poll() is not None:
                    if pending.startswith(('AXOLOTL_BENCH ', 'AXOLOTL_SMOKE ')):
                        raise ValueError('truncated native benchmark output')
                    break
                else:
                    if time.monotonic() > deadline:
                        raise TimeoutError('native benchmark exceeded its deadline')
                    time.sleep(.01)

    def owned_pids(self):
        rows = command(['ps', '-axo', 'pid=,comm='])
        if not rows: raise RuntimeError('cannot verify benchmark process cleanup: ps unavailable')
        return [int(fields[0]) for line in rows.splitlines()
                if len(fields := line.split(None, 1)) == 2
                and Path(fields[1]).resolve() == self.executable.resolve()]

    def __exit__(self, *_):
        import os
        # Only terminate executables inside this invocation's unique temp bundle.
        for pid in self.owned_pids():
            try: os.kill(pid, signal.SIGTERM)
            except ProcessLookupError: pass
        if self.launcher:
            try: self.launcher.wait(timeout=10)
            except subprocess.TimeoutExpired:
                for pid in self.owned_pids():
                    try: os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError: pass
                self.launcher.terminate()
                self.launcher.wait(timeout=5)
        remaining = self.owned_pids()
        if remaining: raise RuntimeError('owned native benchmark process did not exit')
        self.temp.cleanup()
