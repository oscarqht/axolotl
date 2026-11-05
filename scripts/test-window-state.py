#!/usr/bin/env python3
"""Native macOS relaunch checks; uses a temporary profile, no Accessibility access.
Usage: cargo build -p axolotl --features smoke
       python3 scripts/test-window-state.py /absolute/path/to/axolotl
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def close(a, b):
    assert all(abs(a[key] - b[key]) <= 1 for key in a), (a, b)


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='axolotl-window-') as directory:
        path = Path(directory) / 'window-state.json'
        env = dict(os.environ, AXOLOTL_DATA_DIR=directory)

        def run(stage):
            result = subprocess.run([binary, f'--window-state-probe={stage}'],
                                    env=env, capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, result.stderr
            records = [json.loads(line.removeprefix('AXOLOTL_WINDOW '))
                       for line in result.stdout.splitlines() if line.startswith('AXOLOTL_WINDOW ')]
            assert len(records) == 2, (result.stdout, result.stderr)
            for record in records:
                # The header's 4pt gap follows either the normal 84pt traffic
                # light reservation or its fullscreen 8pt edge spacer.
                expected_x = 12 if record['fullscreen'] else 88
                assert abs(record['space_tabs_x'] - expected_x) <= 1, (stage, record)
            print(f'Checked native relaunch: {stage}', flush=True)
            return records

        _, seed = run('seed')
        normal = seed['bounds']
        close(json.loads(path.read_text())['normal'], normal)
        before, _ = run('zoom')
        close(before['bounds'], normal)
        before, after = run('unzoom')
        assert before['maximized'] and not before['fullscreen'], before
        assert not after['maximized'], after
        close(after['bounds'], normal)
        before, after = run('fullscreen')
        close(before['bounds'], normal)
        assert after['fullscreen'], after
        before, after = run('exit-fullscreen')
        assert before['fullscreen'], before
        assert not after['fullscreen'], after
        close(after['bounds'], normal)
        before, _ = run('none')
        close(before['bounds'], normal)
        # Full-screen entered from zoom must return to zoom, then normal.
        run('zoom')
        run('fullscreen')
        before, after = run('exit-fullscreen')
        assert before['fullscreen'] and after['maximized'], (before, after)
        before, after = run('unzoom')
        close(after['bounds'], normal)
        state = json.loads(path.read_text())
        state['normal'].update(x=100000, y=-100000, width=10000, height=10000)
        path.write_text(json.dumps(state))
        _, after = run('none')
        bounds, area = after['bounds'], after['visible']
        assert bounds['x'] >= area['x'] and bounds['y'] >= area['y'], after
        assert bounds['x'] + bounds['width'] <= area['x'] + area['width'] + 1, after
        assert bounds['y'] + bounds['height'] <= area['y'] + area['height'] + 1, after
        for invalid in ['{bad json', json.dumps(dict(state, version=999)),
                        json.dumps(dict(state, normal=dict(x=0, y=0, width=-1, height=20)))]:
            path.write_text(invalid)
            before, _ = run('none')
            assert not before['fullscreen'] and not before['maximized'], before
            assert before['bounds']['width'] >= 640 and before['bounds']['height'] >= 420
        print('PASS: move/resize, zoom, full-screen, zoom→full-screen, normal bounds, off-screen/oversized and invalid state across native relaunches; measured space-tab spacing entering, exiting and restoring full-screen')


if __name__ == '__main__':
    main()
