#!/usr/bin/env python3
"""Run, archive and compare the versioned Axolotl native benchmark (macOS).

Build: cargo build -p axolotl --release --locked --features smoke
Run:   python3 scripts/benchmark.py run --output /tmp/axolotl-baseline
Diff:  python3 scripts/benchmark.py compare BASELINE/report.json CANDIDATE/report.json
"""
from __future__ import annotations
import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import platform
import sys
from benchmarks.protocol import digest, native_digest, load_quality, validate_metrics, power_probe
from benchmarks.report import summarize, build_summary, render_markdown, compare_reports, render_comparison

ROOT = Path(__file__).resolve().parents[1]


def write_report(directory, report):
    report['summary'] = build_summary(report)
    (directory/'report.json').write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')
    (directory/'report.md').write_text(render_markdown(report))


def observation(id, value, unit, scope, required=False):
    return {'id': id, 'samples': [value], 'summary': summarize([value]), 'unit': unit, 'scope': scope,
            'status': 'available', 'direction': 'lower', 'required': required}


def run_one(binary, directory, args, size, repeat, manifest, expected_native):
    from benchmarks.mac import NativeRun, Resources
    from benchmarks.energy import EnergySession
    row = {'session_size': size, 'repeat': repeat, 'status': 'failed', 'metrics': [], 'resources': [], 'energy': {}, 'quality': {}}
    env = {'AXOLOTL_BENCH_SUITE': args.suite, 'AXOLOTL_BENCH_SIZE': size,
           'AXOLOTL_BENCH_ITERATIONS': str(args.iterations), 'AXOLOTL_BENCH_WARMUP': str(args.warmup),
           'AXOLOTL_BENCH_ENERGY_SECONDS': str(args.energy_seconds)}
    active = None
    completed = False
    def finish_phase():
        nonlocal active
        if active is None: return
        phase, monitor, energy = active
        active = None
        if energy:
            try: result = energy.stop()
            except Exception as error:
                result = {'battery': {'status': 'unavailable', 'reason': str(error)}, 'soc_power': {'status': 'unavailable', 'reason': str(error)}}
            row['energy'][phase] = result
            battery = result['battery']
            if battery['status'] == 'measured' and battery.get('percent_points_per_hour') is not None:
                row['metrics'].append(observation(f'energy.{phase}.battery_gauge_rate', battery['percent_points_per_hour'], 'percentage_points/hour', 'whole_device_battery_gauge', required=True))
            elif args.battery:
                row['quality']['energy_measurements_incomplete'] = True
                row['metrics'].append({'id': f'energy.{phase}.battery_gauge_rate', 'samples': [], 'unit': 'percentage_points/hour', 'scope': 'whole_device_battery_gauge', 'required': True, 'status': 'unavailable', 'reason': battery.get('reason', 'insufficient discharge at gauge resolution')})
            components = result['soc_power'].get('components', {})
            for rail, data in components.items():
                row['metrics'].append(observation(f'energy.{phase}.{rail}.estimated_power', data['mean_milliwatts'], 'mW', 'whole_soc_estimated_rail'))
            if args.power and (result['soc_power'].get('warning') or result['soc_power'].get('coverage_fraction', 0) < .8):
                row['quality']['energy_measurements_incomplete'] = True
                row['quality'][f'{phase}.power_coverage_insufficient'] = result['soc_power'].get('coverage_fraction', 0)
            if args.power and not components:
                row['quality']['energy_measurements_incomplete'] = True
                row['metrics'].append({'id': f'energy.{phase}.soc.estimated_power', 'samples': [], 'unit': 'mW', 'scope': 'whole_soc_estimated_rail', 'required': True, 'status': 'unavailable', 'reason': result['soc_power'].get('reason', 'no power samples')})
        if monitor:
            try: resource = monitor.stop()
            except Exception as error: resource = {'error': str(error)}
            resource['phase'] = phase
            row['resources'].append(resource)
            if not resource.get('error'):
                for group in ['app', 'webkit', 'combined']:
                    for field, unit in [('cpu_percent_one_core', '%_one_core'), ('rss_mib_peak_sampled', 'MiB')]:
                        value = resource[group][field]
                        if value is not None: row['metrics'].append(observation(f'resource.{phase}.{group}.{field}', value, unit, 'app_owned_coalition_sampled'))
            else: row['quality'][f'{phase}.resources_unavailable'] = resource['error']
    try:
        with NativeRun(binary, directory, env, args.timeout) as native:
            for message in native.messages():
                kind = message.get('type')
                if kind == 'run_start':
                    row['native_state'] = message
                    if message.get('native_sha256') != expected_native or message.get('suite_version') != manifest['suite_version']:
                        raise ValueError('native benchmark stimulus differs from checkout; rebuild with --features smoke')
                elif kind == 'metric':
                    metric = message['metric']; metric['summary'] = summarize(metric.get('samples', [])); row['metrics'].append(metric)
                elif kind == 'quality': row['quality'].update({k: v for k, v in message.items() if k != 'type'})
                elif kind == 'phase_start':
                    if active is not None: raise ValueError('overlapping native workload phases')
                    phase = message['id']; monitor = energy = None
                    try:
                        monitor = Resources(native.pid); monitor.start()
                    except Exception as error: row['quality'][f'{phase}.resources_unavailable'] = str(error)
                    active = (phase, monitor, None)
                    if args.suite == 'energy':
                        energy = EnergySession(directory/phase, battery=args.battery, power=args.power)
                        active = (phase, monitor, energy)
                        energy.start()
                    native.acknowledge(phase, True)
                elif kind == 'phase_end':
                    if active is None or active[0] != message['id']: raise ValueError('unmatched native phase end')
                    if message.get('inactive_ticks', 0):
                        row['quality']['comparable'] = False
                        row['quality']['energy_foreground_interference'] = message['inactive_ticks']
                    row.setdefault('phases', []).append(message)
                    finish_phase(); native.acknowledge(message['id'], False)
                elif kind == 'run_end': completed = message.get('status') == 'passed'
                elif kind == 'smoke':
                    row['smoke'] = message
                    row['status'] = 'passed' if completed and message.get('result') == 'passed' else 'failed'
            if not row.get('smoke'): row['error'] = 'native process exited without a final smoke result'
    except Exception as error:
        row['status'] = 'failed'
        row['error'] = str(error)
    finally:
        try: finish_phase()
        except Exception as error: row['quality']['collector_cleanup_error'] = str(error)
    missing = validate_metrics(row, manifest, args.suite)
    if missing:
        row['status'] = 'failed'; row['quality']['missing_required_metrics'] = missing
    return row


def run(args):
    if platform.system() != 'Darwin': raise ValueError('the runner/energy collector currently requires macOS; native fixtures remain smoke-gated')
    from benchmarks.energy import read_battery, EnergyPrerequisiteError
    from benchmarks.mac import hardware, command
    binary = args.binary.resolve()
    if not binary.is_file() or b'AXOLOTL_BENCH ' not in binary.read_bytes():
        raise ValueError('build the release executable with --features smoke first')
    if not 1 <= args.iterations <= 100 or not 0 <= args.warmup <= 20 or not 1 <= args.runs <= 10:
        raise ValueError('iterations:1..100, warmup:0..20, runs:1..10')
    if not 3 <= args.energy_seconds <= 3600: raise ValueError('energy-seconds must be 3..3600')
    if (args.battery or args.power) and args.suite != 'energy': raise ValueError('--battery/--power require --suite energy')
    battery = read_battery()
    if args.battery and (args.energy_seconds < 300 or not battery.get('discharging')):
        raise EnergyPrerequisiteError('--battery requires >=300 seconds and an unplugged, discharging battery; recommend 1800 seconds because gauge resolution is 1 percentage point')
    if args.power:
        probe = power_probe()
        if probe['status'] != 'estimated': raise EnergyPrerequisiteError(probe['reason'])
    machine = hardware()
    initial_load = load_quality(command(['sysctl', '-n', 'vm.loadavg']), machine['logical_cpus'])
    if not initial_load['comparable'] and not args.allow_busy:
        raise ValueError('host load is unavailable or exceeds logical CPU count; retry when idle, or use --allow-busy for a non-comparable diagnostic')
    manifest = json.loads((ROOT/'scripts/benchmarks/suite.json').read_text())
    expected_native = native_digest(ROOT, manifest)
    if args.timeout is None: args.timeout = 2*args.energy_seconds+180 if args.suite == 'energy' else 300
    if args.timeout < 10: raise ValueError('timeout must be at least 10 seconds')
    sizes = args.sessions.split(',')
    if not sizes or any(s not in ['small', 'large'] for s in sizes) or len(set(sizes)) != len(sizes): raise ValueError('sessions must be small, large, or small,large')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()): raise ValueError('output directory must be empty; baselines are never overwritten')
    fixtures = [ROOT/'crates/desktop/assets/benchmark-page.html', ROOT/'crates/desktop/src/benchmark_fixture.rs']
    sources = list((ROOT/'crates/desktop/src').glob('benchmark*.rs'))+list((ROOT/'scripts/benchmarks').glob('*.py'))+[Path(__file__).resolve()]
    sources = [p for p in sources if not p.name.startswith('test_')]+[ROOT/'scripts/benchmarks/suite.json']
    report = {'schema_version': 1, 'suite_version': 1, 'label': args.label,
              'created_at': dt.datetime.now(dt.timezone.utc).isoformat(), 'status': 'running',
              'metadata': {'hardware': machine, 'native_sha256': expected_native, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                           'git_revision': command(['git', '-C', str(ROOT), 'rev-parse', 'HEAD']),
                           'git_dirty': bool(command(['git', '-C', str(ROOT), 'status', '--porcelain'])),
                           'fixture_sha256': digest(ROOT, fixtures), 'benchmark_sha256': digest(ROOT, sources),
                           'configuration': {'iterations': args.iterations, 'warmup': args.warmup, 'runs': args.runs,
                                             'session_sizes': sizes, 'suites': [args.suite], 'energy_seconds': args.energy_seconds if args.suite == 'energy' else None,
                                             'host_load_limit': 'logical_cpu_count', 'battery': args.battery, 'power': args.power, 'app_nap': 'natural' if args.suite == 'energy' else 'suppressed'},
                           'conditions': {'power_source': battery['source']},
                           'environment': {'battery_at_start': battery, 'thermal': command(['pmset', '-g', 'therm']), 'load_average': command(['sysctl', '-n', 'vm.loadavg'])}}, 'runs': []}
    write_report(output, report)
    for repeat in range(1, args.runs+1):
        for size in sizes:
            directory = output/f'{size}-{repeat}'; directory.mkdir()
            print(f'Benchmark {size}, run {repeat}/{args.runs}', flush=True)
            power_before = read_battery()
            before = load_quality(command(['sysctl', '-n', 'vm.loadavg']), machine['logical_cpus'])
            row = run_one(binary, directory, args, size, repeat, manifest, expected_native)
            after = load_quality(command(['sysctl', '-n', 'vm.loadavg']), machine['logical_cpus'])
            row['quality']['host_load'] = {'before': before, 'after': after}
            if not before['comparable'] or not after['comparable']: row['quality']['comparable'] = False
            power_after = read_battery()
            row['quality']['power_sources'] = [power_before['source'], power_after['source']]
            if any(p['source'] != battery['source'] or not p['available'] for p in [power_before, power_after]):
                row['quality']['comparable'] = False
                row['quality']['power_source_changed_or_unavailable'] = True
            state = row.get('native_state', {}).get('state', {})
            conditions = {'viewport': state.get('viewport'), 'scale_factor': state.get('scale_factor')}
            if not report['runs']: report['metadata']['conditions'].update(conditions)
            elif any(report['metadata']['conditions'].get(k) != v for k,v in conditions.items()): row['quality']['comparable'] = False; row['quality']['window_geometry_changed'] = True
            report['runs'].append(row); write_report(output, report)
            print(f'  {row["status"]}: {len(row["metrics"])} metrics', flush=True)
    report['status'] = 'passed' if all(r['status'] == 'passed' for r in report['runs']) else 'failed'
    if report['status'] == 'passed' and any(r['quality'].get('comparable') is False or r['quality'].get('energy_measurements_incomplete') or any(key.endswith('resources_unavailable') or key == 'collector_cleanup_error' for key in r['quality']) for r in report['runs']): report['status'] = 'incomplete'
    write_report(output, report)
    print(f'Report: {output / "report.md"}')
    return {'passed': 0, 'incomplete': 3}.get(report['status'], 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    execute = commands.add_parser('run')
    execute.add_argument('--binary', type=Path, default=ROOT/'target/release/axolotl')
    execute.add_argument('--output', type=Path, required=True)
    execute.add_argument('--label', default='baseline')
    execute.add_argument('--suite', choices=['standard', 'actions', 'web', 'energy'], default='standard')
    execute.add_argument('--sessions', default='small,large')
    execute.add_argument('--iterations', type=int, default=20)
    execute.add_argument('--warmup', type=int, default=3)
    execute.add_argument('--runs', type=int, default=3)
    execute.add_argument('--energy-seconds', type=int, default=300)
    execute.add_argument('--battery', action='store_true')
    execute.add_argument('--power', action='store_true')
    execute.add_argument('--timeout', type=int, help='per-process deadline; default 300s or energy workload duration + 180s')
    execute.add_argument('--allow-busy', action='store_true', help='allow diagnostic runs on a busy host; quality excludes them from comparisons')
    doctor = commands.add_parser('doctor')
    doctor.add_argument('--power', action='store_true', help='also probe existing noninteractive powermetrics access')
    comparison = commands.add_parser('compare')
    comparison.add_argument('baseline', type=Path); comparison.add_argument('candidate', type=Path)
    comparison.add_argument('--output', type=Path)
    comparison.add_argument('--relative-threshold', type=float, default=.1)
    comparison.add_argument('--absolute-threshold-ms', type=float, default=1.)
    args = parser.parse_args()
    try:
        if args.command == 'run': return run(args)
        if args.command == 'doctor':
            if platform.system() != 'Darwin': raise ValueError('native runner requires macOS')
            from benchmarks.mac import hardware, command
            from benchmarks.energy import read_battery
            machine = hardware()
            result = {'hardware': machine, 'battery': read_battery(), 'load_quality': load_quality(command(['sysctl', '-n', 'vm.loadavg']), machine['logical_cpus'])}
            if args.power: result['soc_power'] = power_probe()
            print(json.dumps(result, indent=2)); return 0
        result = compare_reports(json.loads(args.baseline.read_text()), json.loads(args.candidate.read_text()), args.relative_threshold, args.absolute_threshold_ms)
        text = render_comparison(result); print(text)
        if args.output: args.output.write_text(text)
        return {'incompatible': 2, 'incomplete': 3, 'regression': 4}.get(result['status'], 0)
    except (ValueError, OSError, RuntimeError) as error:
        parser.error(str(error))


if __name__ == '__main__': sys.exit(main())
