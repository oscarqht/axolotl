#!/usr/bin/env python3
"""Check native space editing and session restore across separate app processes."""
import json
import os
import sqlite3
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix='axolotl-space-') as directory:
        env = dict(os.environ, AXOLOTL_DATA_DIR=directory)

        def run(stage):
            result = subprocess.run([binary, f'--space-session-probe={stage}'],
                                    env=env, capture_output=True, text=True, timeout=45)
            assert result.returncode == 0, (result.stdout, result.stderr)
            records = [json.loads(line.removeprefix('AXOLOTL_SPACE_SESSION '))
                       for line in result.stdout.splitlines()
                       if line.startswith('AXOLOTL_SPACE_SESSION ')]
            assert len(records) == 1, (result.stdout, result.stderr)
            print(f'Checked native space session: {stage}', flush=True)

        run('seed')
        path = Path(directory) / 'app.sqlite3'
        with sqlite3.connect(path) as db:
            saved = json.loads(db.execute("SELECT json FROM app_state WHERE key='session'").fetchone()[0])
        assert saved['spaces'][0]['pinned'] is True, saved
        assert saved['spaces'][0]['icon'] == '🚀', saved
        assert saved['spaces'][0]['custom_title'] == 'Restart space', saved
        run('restore')
        for space in saved['spaces']:
            space.pop('icon', None)
            space.pop('pinned', None)
        # A fresh profile exercises automatic import of an older session.
        with tempfile.TemporaryDirectory(prefix='axolotl-space-legacy-') as legacy:
            original = json.dumps(saved)
            backup = Path(legacy) / 'session.json'
            backup.write_text(original)
            env['AXOLOTL_DATA_DIR'] = legacy
            run('legacy')
            assert backup.read_text() == original, 'Legacy backup was rewritten'
            with sqlite3.connect(Path(legacy) / 'app.sqlite3') as db:
                assert db.execute("SELECT count(*) FROM app_state WHERE key='session'").fetchone()[0] == 1
        print('PASS: pin status, title and emoji saved through native editor, restored after restart, older sessions default to no icon and unpinned')


if __name__ == '__main__':
    main()
