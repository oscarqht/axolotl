"""Stimulus identity, required coverage and measurement preflights."""
import hashlib
from pathlib import Path
import re
import subprocess


def digest(root, paths, sort=True):
    result = hashlib.sha256()
    for path in sorted(paths) if sort else paths:
        path = Path(path)
        result.update(str(path.relative_to(root)).encode())
        result.update(b'\0')
        result.update(path.read_bytes())
    return result.hexdigest()


def native_digest(root, manifest):
    return digest(root, [root/p for p in manifest['native_sources']], sort=False)


def load_quality(text, cpus):
    values = re.findall(r'\d+(?:\.\d+)?', text)
    if not values or not cpus:
        return {'comparable': False, 'host_load_unavailable': True}
    load = float(values[0])
    return {'comparable': load <= cpus, 'host_load_1m': load,
            'host_load_limit': cpus}


def validate_metrics(row, manifest, suite):
    suites = ['actions', 'web'] if suite == 'standard' else [suite]
    present = {m['id'] for m in row['metrics'] if m.get('status') == 'available' and m.get('samples')}
    missing = sorted({key for section in suites for key in manifest['required'][section]}-present)
    for key in missing:
        row['metrics'].append({'id': key, 'status': 'unavailable', 'samples': [],
                               'unit': 'ms', 'scope': 'required_workload', 'required': True,
                               'reason': 'required workload did not produce samples'})
    return missing


def power_probe():
    from .energy import parse_power
    try:
        result = subprocess.run(['/usr/bin/sudo', '-n', '/usr/bin/powermetrics', '--samplers', 'cpu_power,gpu_power,ane_power', '--format', 'text', '--sample-rate', '1000', '--sample-count', '1'],
                                capture_output=True, text=True, timeout=8)
        if result.returncode:
            return {'status': 'unavailable', 'reason': 'powermetrics requires existing noninteractive sudo authorization or supported samplers'}
        return parse_power(result.stdout)
    except (OSError, subprocess.SubprocessError):
        return {'status': 'unavailable', 'reason': 'powermetrics probe failed or timed out'}
