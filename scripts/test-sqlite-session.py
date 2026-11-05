#!/usr/bin/env python3
"""Import legacy app data and relaunch the native app against an isolated profile.
Usage: cargo build -p axolotl --features smoke
       python3 scripts/test-sqlite-session.py /absolute/path/to/axolotl
"""
import base64
import copy
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time


def run(binary, directory, stage):
    result = subprocess.run(
        [binary, f'--persistence-exit-probe={stage}'],
        env=dict(os.environ, AXOLOTL_DATA_DIR=str(directory)),
        capture_output=True, text=True, timeout=45,
    )
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert sum(line.startswith('AXOLOTL_PERSISTENCE_EXIT ') for line in
               result.stdout.splitlines()) == 1, (result.stdout, result.stderr)


def read_state(directory):
    with sqlite3.connect(directory / 'app.sqlite3') as db:
        assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
        assert db.execute('PRAGMA user_version').fetchone()[0] == 1
        state = {key: json.loads(value) for key, value in
                 db.execute('SELECT key,json FROM app_state')}
        visits = db.execute('SELECT id,title,url,visited_at FROM visits ORDER BY id').fetchall()
        assert db.execute("SELECT value FROM migration_metadata WHERE key='legacy_import'").fetchone()[0] == 'complete'
    return state, visits


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='axolotl-sqlite-restart-') as temporary:
        root = Path(temporary)
        seed = root / 'seed'
        run(binary, seed, 'seed')
        state, _ = read_state(seed)
        session = state['session']
        # Dormant tabs/pins and a second space exercise all workspace wire fields
        # without depending on the public internet during the native relaunch.
        session['pinned_tabs'] = [{
            'tab': {'id': 100, 'url': 'https://pin.example.test/',
                    'title': 'Pinned page', 'custom_title': 'Fixed pin'},
            'icon': '📌', 'width': 530.0, 'height': 710.0,
        }]
        first = session['spaces'][0]
        first['panes'][0]['tabs'].append({
            'id': 101, 'url': 'https://dormant.example.test/',
            'title': 'Dormant title', 'custom_title': 'Fixed tab',
        })
        original_pane = first['panes'][0]['id']
        first['panes'].append({
            'id': 102, 'active_tab': 103,
            'tabs': [{'id': 103, 'url': 'about:blank', 'title': '', 'custom_title': 'Split page'}],
        })
        first['layout'] = {'split': {'id': 104, 'axis': 'horizontal', 'ratio': 0.37,
                                    'first': {'pane': {'id': original_pane}},
                                    'second': {'pane': {'id': 102}}}}
        second = copy.deepcopy(first)
        second.update(id=105, name='Second', custom_title='Second workspace', icon='🧪',
                      focused_pane=106, layout={'pane': {'id': 106}},
                      panes=[{'id': 106, 'active_tab': 107,
                              'tabs': [{'id': 107, 'url': 'https://space.example.test/',
                                        'title': 'Inactive page', 'custom_title': 'Second tab'}]}])
        session['spaces'].append(second)
        session['next_id'] = 108
        directory = root / 'legacy'
        (directory / 'sleep').mkdir(parents=True)
        settings = {'auto_sleep': False, 'sleep_minutes': 7,
                    'search_engine': 'duckduckgo', 'homepage_url': 'about:blank',
                    'history_retention_days': None}
        sleep = {'100': {'sleeping': True, 'keep_alive': True}}
        image = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=')
        (directory / 'sleep' / '100.png').write_bytes(image)
        protection = {'enabled': False, 'exceptions': ['example.test']}
        window = {'version': 1, 'normal': {'x': 80, 'y': 80, 'width': 900, 'height': 650},
                  'maximized': False, 'fullscreen': False, 'display': None}
        files = {'session.json': session, 'settings.json': settings,
                 'sleep/state.json': sleep, 'protection.json': protection,
                 'window-state.json': window}
        for relative, value in files.items():
            (directory / relative).write_text(json.dumps(value))
        history = [(5, 'Preserved title', 'https://history.example.test/', int(time.time())),
                   (8, 'Repeat visit', 'https://history.example.test/', int(time.time()))]
        with sqlite3.connect(directory / 'history.sqlite3') as db:
            db.execute('CREATE TABLE visits(id INTEGER PRIMARY KEY AUTOINCREMENT, '
                       'title TEXT NOT NULL,url TEXT NOT NULL,visited_at INTEGER NOT NULL)')
            db.executemany('INSERT INTO visits VALUES (?,?,?,?)', history)
            db.execute('PRAGMA user_version=1')
        backups = {relative: (directory / relative).read_bytes() for relative in
                   [*files, 'history.sqlite3']}
        for attempt in range(2):
            result = subprocess.run([binary, '--persistence-exit-probe=inspect'],
                                    env=dict(os.environ, AXOLOTL_DATA_DIR=str(directory)),
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, (result.stdout, result.stderr)
            live = [json.loads(line.removeprefix('AXOLOTL_PERSISTENCE_STATE '))
                    for line in result.stdout.splitlines() if line.startswith('AXOLOTL_PERSISTENCE_STATE ')]
            assert len(live) == 1, (result.stdout, result.stderr)
            live = live[0]
            assert live['settings'] == settings, live
            assert live['protection'] == protection, live
            assert live['sleep'] == sleep, live
            assert live['sleep_images'] == [100], live
            assert live['sleep_live_views'] == [], live
            assert live['session']['pinned_tabs'] == session['pinned_tabs'], live
            assert live['session']['spaces'][1] == second, live
            assert (directory / 'sleep' / '100.png').read_bytes() == image
            saved, visits = read_state(directory)
            assert visits == history, ('Lost or duplicated history', visits)
            assert saved['settings'] == settings, saved['settings']
            assert saved['protection'] == protection, saved['protection']
            assert saved['sleep'] == sleep, saved['sleep']
            restored = saved['session']
            assert restored['active_space'] == session['active_space']
            assert restored['spaces'][0]['layout'] == first['layout']
            assert restored['spaces'][1] == second
            assert restored['pinned_tabs'] == session['pinned_tabs']
            assert restored['spaces'][0]['panes'][0]['tabs'][1] == first['panes'][0]['tabs'][1]
            assert saved['window']['normal']['width'] == 900, saved['window']
            for relative, original in backups.items():
                assert (directory / relative).read_bytes() == original, f'Backup rewritten: {relative}'
            print(f'Checked native SQLite migration/restart {attempt + 1}', flush=True)
        # A committed clear must remain clear even with the legacy DB retained.
        with sqlite3.connect(directory / 'app.sqlite3') as db:
            db.execute('DELETE FROM visits')
        result = subprocess.run([binary, '--persistence-exit-probe=inspect'],
                                env=dict(os.environ, AXOLOTL_DATA_DIR=str(directory)),
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, (result.stdout, result.stderr)
        assert read_state(directory)[1] == [], 'Legacy history was reimported after clear'
        result = subprocess.run([binary, '--persistence-exit-probe=wake-pins'],
                                env=dict(os.environ, AXOLOTL_DATA_DIR=str(directory)),
                                capture_output=True, text=True, timeout=30)
        assert result.returncode == 0, (result.stdout, result.stderr)
        assert not (directory / 'sleep' / '100.png').exists(), 'Waking failed to clear screenshot cache'
        assert read_state(directory)[0]['sleep']['100'] == {'sleeping': False, 'keep_alive': True}
        print('PASS: automatic one-time import, native restarts, layout/tab/pin/space metadata, '
              'settings, sleeping view restoration/wake/cache cleanup, protection, window geometry, unchanged backups, history IDs and no reimport after clear')


if __name__ == '__main__':
    main()
