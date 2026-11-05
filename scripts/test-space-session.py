#!/usr/bin/env python3
"""Check native space editing and session restore across separate app processes."""
import json
import os
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
        path = Path(directory) / 'session.json'
        saved = json.loads(path.read_text())
        assert saved['spaces'][0]['icon'] == '🚀', saved
        assert saved['spaces'][0]['custom_title'] == 'Restart space', saved
        run('restore')
        for space in saved['spaces']:
            space.pop('icon', None)
        path.write_text(json.dumps(saved))
        run('legacy')
        print('PASS: title and emoji saved through native editor, restored after restart, older sessions default to no icon')


if __name__ == '__main__':
    main()
