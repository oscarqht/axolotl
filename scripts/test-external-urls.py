#!/usr/bin/env python3
"""Native URL delivery with an isolated profile. macOS: pass a packaged smoke .app.
Other platforms: pass the smoke executable. Never changes the default browser.
"""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def wait_for(check, message, timeout=25):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.1)
    raise AssertionError(message)


def tabs(state):
    return [tab for space in state['spaces'] for pane in space['panes'] for tab in pane['tabs']]


def main():
    target = Path(sys.argv[1]).resolve()
    bundle = target if target.suffix == '.app' else None
    binary = bundle / 'Contents/MacOS/Axolotl' if bundle else target
    with tempfile.TemporaryDirectory(prefix='axolotl-external-') as directory:
        directory = Path(directory)
        env = dict(os.environ, AXOLOTL_DATA_DIR=str(directory))
        probe = directory / 'external-probe.json'
        processes = []

        def launch(*args):
            process = subprocess.Popen([str(binary), '--external-url-probe', *args], env=env,
                                       stdout=subprocess.DEVNULL, stderr=log)
            processes.append(process)
            return process

        def state():
            return json.loads(probe.read_text())

        def urls():
            return [tab['url'] for tab in tabs(state()['workspace'])]

        def stop(process):
            (directory / 'quit-probe').touch()
            assert process.wait(timeout=15) == 0
            (directory / 'quit-probe').unlink()

        with (directory / 'launch.log').open('w') as log:
            try:
                first = launch()
                initial = wait_for(lambda: state(), 'initial window did not launch')
                if bundle:
                    assert initial['browserRegistered'], 'LaunchServices did not advertise both URL schemes'
                    assert initial['defaultBrowser']['isDefault'] is not None, initial
                stop(first)
                # Seed two spaces and a split panel through a valid saved workspace.
                saved_path = directory / 'session.json'
                saved = json.loads(saved_path.read_text()) if saved_path.exists() else initial['workspace']
                # Generate a fixture using the existing snapshot shape without replacing IDs.
                space = saved['spaces'][0]
                second = copy.deepcopy(space)
                second['id'] = 100
                second['panes'][0]['id'] = 101
                second['panes'][0]['tabs'][0]['id'] = 102
                second['panes'][0]['active_tab'] = 102
                second['focused_pane'] = 101
                # Layout is an externally tagged enum; replace leaf pane IDs recursively.
                def remap(value, old, new):
                    if isinstance(value, dict):
                        return {key: remap(item, old, new) for key, item in value.items()}
                    if isinstance(value, list):
                        return [remap(item, old, new) for item in value]
                    return new if value == old else value
                second['layout'] = remap(second['layout'], space['panes'][0]['id'], 101)
                second['panes'][0]['tabs'][0]['url'] = 'https://preserved.test/'
                # Focus the second panel in the non-first space, leaving both
                # its sibling and the first space intact.
                sibling = copy.deepcopy(second['panes'][0])
                sibling['id'] = 103
                sibling['tabs'][0]['id'] = 104
                sibling['active_tab'] = 104
                sibling['tabs'][0]['url'] = 'https://focused.test/'
                second['panes'].append(sibling)
                second['layout'] = {'split': {'id': 105, 'axis': 'horizontal', 'ratio': .5,
                                             'first': {'pane': {'id': 101}},
                                             'second': {'pane': {'id': 103}}}}
                second['focused_pane'] = 103
                saved['spaces'].append(second)
                saved['active_space'] = 100
                saved['next_id'] = 106
                saved_path.write_text(json.dumps(saved))
                cold = ['https://cold.test/a?x=1&y=2#fragment', 'http://cold.test/b']
                first = launch(*cold)
                wait_for(lambda: all(url in urls() for url in cold), 'cold CLI URLs were not restored and appended')
                restored = state()['workspace']
                assert urls()[-2:] == cold
                assert len(tabs(restored)) == len(tabs(saved)) + 2, restored
                for original in tabs(saved):
                    assert any(tab['id'] == original['id'] and tab['url'] == original['url'] for tab in tabs(restored))
                active_space = restored['active_space']
                active_pane = next(s for s in restored['spaces'] if s['id'] == active_space)['focused_pane']
                wait_for(lambda: state()['active'], 'cold launch did not activate')
                (directory / 'minimize-probe').touch()
                wait_for(lambda: not state()['active'], 'owner did not minimize')
                for index in range(2):
                    other = launch('https://running.test/a', 'https://running.test/b', 'file:///tmp/ignored')
                    assert other.wait(timeout=15) == 0
                    wait_for(lambda: urls().count('https://running.test/a') == index + 1, 'secondary launch did not reach owner')
                wait_for(lambda: state()['active'], 'minimized owner was not restored')
                if bundle:
                    incoming = ['https://apple-event.test/one', 'http://apple-event.test/two']
                    subprocess.run(['open', '-a', str(bundle), *incoming], check=True)
                    wait_for(lambda: all(url in urls() for url in incoming), 'running Apple-event delivery failed')
                    assert all(urls().count(url) == 1 for url in incoming)
                    assert urls()[-2:] == incoming
                    forced = ['https://forced-instance.test/one', 'http://forced-instance.test/two']
                    subprocess.run(['open', '-n', '-W', '--env', f'AXOLOTL_DATA_DIR={directory}',
                                    '-a', str(bundle), *forced], check=True, timeout=20)
                    wait_for(lambda: all(url in urls() for url in forced), 'forced secondary Apple-event handoff failed')
                    assert all(urls().count(url) == 1 for url in forced)
                final = state()['workspace']
                pane = next(p for s in final['spaces'] if s['id'] == active_space for p in s['panes'] if p['id'] == active_pane)
                assert all(url in [tab['url'] for tab in pane['tabs']] for url in cold + ['https://running.test/a', 'https://running.test/b'])
                assert 'file:///tmp/ignored' not in urls()
                wait_for(lambda: state()['active'], 'owner was not activated')
                stop(first)
                before = json.loads(saved_path.read_text())
                probe.unlink()
                if bundle:
                    # Keep the OS launch attached to an `open -W` process so
                    # cleanup waits for the isolated application to quit.
                    cold_process = subprocess.Popen(
                        ['open', '-W', '--env', f'AXOLOTL_DATA_DIR={directory}',
                         '-a', str(bundle), 'https://apple-cold.test/',
                         '--args', '--external-url-probe'],
                        stdout=subprocess.DEVNULL, stderr=log)
                    processes.append(cold_process)
                    wait_for(lambda: 'https://apple-cold.test/' in urls(), 'cold Apple event was lost')
                    assert urls().count('https://apple-cold.test/') == 1
                    assert len(tabs(state()['workspace'])) == len(tabs(before)) + 1
                    stop(cold_process)
                    before = json.loads(saved_path.read_text())
                    probe.unlink()
                first = launch()
                wait_for(lambda: [(t['id'], t['url']) for t in tabs(state()['workspace'])] == [(t['id'], t['url']) for t in tabs(before)], 'session changed on relaunch')
                stop(first)
                print('PASS: cold and running launches, multiple URLs, repeat requests, focused panel, minimized-window activation, invalid URL rejection, session preservation' + (', cold/running/forced-secondary macOS Apple events' if bundle else ''))
            except Exception:
                print((directory / 'launch.log').read_text(), file=sys.stderr)
                raise
            finally:
                (directory / 'quit-probe').touch()
                for process in processes:
                    if process.poll() is None:
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.terminate()
                            process.wait(timeout=10)


if __name__ == '__main__':
    main()
