#!/usr/bin/env python3
"""Profile a release executable in an isolated macOS app bundle.
Usage: python3 scripts/profile-ui.py target/release/axolotl > report.jsonl
       python3 scripts/profile-ui.py target/release/axolotl --check space-menu
Uses LaunchServices for a distinct app/WebKit resource coalition, an isolated
smoke profile, and deterministic native fixtures. Never loads browsing data.
"""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

libproc = ctypes.CDLL('/usr/lib/libproc.dylib')
libproc.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
responsible = ctypes.CDLL('/usr/lib/libSystem.B.dylib').responsibility_get_pid_responsible_for_pid
responsible.argtypes = [ctypes.c_int]
responsible.restype = ctypes.c_int


def coalition(pid):
    ids = (ctypes.c_uint64 * 16)()
    count = libproc.proc_pidinfo(pid, 20, 0, ids, ctypes.sizeof(ids))
    return tuple(ids[:2]) if count >= 16 else None


def cpu_time(value):
    return sum(float(p) * 60**i for i, p in enumerate(reversed(value.split(':'))))


def snapshot(pid):
    owner = coalition(pid)
    rows = subprocess.check_output(['ps', '-axo', 'pid=,rss=,time=,comm='], text=True)
    result = {}
    for line in rows.splitlines():
        fields = line.split(None, 3)
        if len(fields) != 4:
            continue
        child, rss, cpu, command = fields
        child = int(child)
        if child == pid or ('com.apple.WebKit.' in command and owner and coalition(child) == owner):
            result[child] = {'rss_kib': int(rss), 'cpu_seconds': cpu_time(cpu), 'kind': 'app' if child == pid else command.rsplit('/', 1)[-1]}
    return result


def lines_from(path, launcher):
    with path.open() as stream:
        while True:
            line = stream.readline()
            if line:
                yield line
            elif launcher.poll() is not None:
                break
            else:
                time.sleep(0.02)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', type=Path)
parser.add_argument('--check', default='performance', choices=['performance', 'responsiveness', 'pins', 'history', 'sleep', 'docking', 'space-drag', 'popup-transition', 'link-popup', 'space-shortcuts', 'notifications', 'settings', 'space-menu'], help='smoke mode (default: performance)')
args = parser.parse_args()
if not args.binary.is_file() or b'AXOLOTL_SMOKE ' not in args.binary.read_bytes():
    parser.error('provide an axolotl executable built with --features smoke')
print(json.dumps({'benchmark': {'binary_sha256': hashlib.sha256(args.binary.read_bytes()).hexdigest(), 'check': args.check, 'launch': 'LaunchServices'}}), flush=True)
passed = False
with tempfile.TemporaryDirectory(prefix='axolotl-perf-') as directory:
    root = Path(directory)
    bundle = root / 'Axolotl Performance.app'
    contents = bundle / 'Contents'
    (contents / 'MacOS').mkdir(parents=True)
    shutil.copy2(args.binary.resolve(), contents / 'MacOS/Axolotl')
    with (contents / 'Info.plist').open('wb') as stream:
        plistlib.dump({'CFBundleExecutable': 'Axolotl', 'CFBundleIdentifier': f'dev.axolotl.perf.{uuid.uuid4().hex}', 'CFBundleName': 'Axolotl Performance', 'CFBundlePackageType': 'APPL', 'NSHighResolutionCapable': True, 'NSAppTransportSecurity': {'NSAllowsArbitraryLoadsInWebContent': True}}, stream)
    subprocess.run(['codesign', '--force', '--sign', '-', str(bundle)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    stdout, stderr = root / 'stdout', root / 'stderr'
    stdout.touch()
    stderr.touch()
    launcher = subprocess.Popen(['open', '-W', '-n', '--stdout', str(stdout), '--stderr', str(stderr), str(bundle), '--args', '--smoke-test', f'--smoke-{args.check}'])
    idle = None
    for line in lines_from(stdout, launcher):
        if line.startswith('AXOLOTL_SMOKE '):
            report = json.loads(line.removeprefix('AXOLOTL_SMOKE '))
            passed = report['result'] == 'passed'
            print(json.dumps({'smoke': report}), flush=True)
        elif line.startswith('AXOLOTL_PERF '):
            data = json.loads(line.removeprefix('AXOLOTL_PERF '))
            if data['phase'].endswith('_idle_begin'):
                pid = data['pid']
                if responsible(pid) != pid:
                    # This PID came from our isolated smoke process, which has
                    # no user profile. Do not leave it running after failure.
                    subprocess.run(['kill', '-TERM', str(pid)], check=False)
                    launcher.wait(timeout=10)
                    raise RuntimeError('LaunchServices did not assign an independent responsibility identity; resource attribution would be unreliable')
                idle = (time.monotonic(), snapshot(pid), pid)
            elif data['phase'].endswith('_idle') and idle:
                elapsed = time.monotonic() - idle[0]
                after = snapshot(idle[2])
                before = idle[1]
                metrics = {}
                for kind in ['app', 'webkit', 'combined']:
                    group = {pid: row for pid, row in after.items() if kind == 'combined' or (row['kind'] == 'app') == (kind == 'app')}
                    metrics[kind] = {
                        'rss_mib': sum(row['rss_kib'] for row in group.values()) / 1024,
                        'cpu_percent_one_core': 100 * sum(max(0, row['cpu_seconds'] - before.get(pid, row)['cpu_seconds']) for pid, row in group.items()) / elapsed,
                        'process_count': len(group),
                    }
                data['resources'] = metrics
                data['resource_interval_seconds'] = elapsed
                data['coalition'] = coalition(idle[2])
                data['webkit_attribution'] = 'independent LaunchServices app coalition; responsibility identity verified'
                if data['coalition'] is None:
                    data['resources']['webkit'] = None
                    data['resources']['combined'] = None
            print(json.dumps(data), flush=True)
        else:
            print(json.dumps({'output': line.rstrip()}), flush=True)
    launcher.wait()
    sys.stderr.write(stderr.read_text())
sys.exit(0 if passed else 1)
