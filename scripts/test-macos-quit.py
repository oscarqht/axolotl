#!/usr/bin/env python3
"""Check native Cmd+Q/menu process exit, and restored tabs.

Usage: cargo build -p axolotl --locked --features smoke
       python3 scripts/test-macos-quit.py target/debug/axolotl
Requires a macOS GUI session. Uses AppKit's event queue and actual menu target;
does not require OS input or Accessibility permissions. Profiles are temporary.
"""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile


def session(output):
    records = [json.loads(line.removeprefix('AXOLOTL_QUIT_SESSION '))
               for line in output.splitlines() if line.startswith('AXOLOTL_QUIT_SESSION ')]
    assert len(records) == 1, output
    return records[0]


def main():
    binary = str(Path(sys.argv[1]).resolve())
    for mode in ['key', 'menu']:
        with tempfile.TemporaryDirectory(prefix=f'axolotl-quit-{mode}-') as directory:
            env = dict(os.environ, AXOLOTL_DATA_DIR=directory)

            def run(stage):
                result = subprocess.run([binary, f'--quit-probe={stage}'], env=env,
                                        capture_output=True, text=True, timeout=60)
                assert result.returncode == 0, (result.stdout, result.stderr)
                return result.stdout

            output = run(mode)
            assert f'AXOLOTL_QUIT_READY {mode}' in output, output
            expected = session(output)
            with sqlite3.connect(Path(directory) / 'app.sqlite3') as db:
                saved = json.loads(db.execute("SELECT json FROM app_state WHERE key='session'").fetchone()[0])
            assert saved == expected, (saved, expected)
            assert session(run('restore')) == expected, 'Restored tabs differ from quit checkpoint'
            # A process restart or a retained profile lock would break the next launch.
            print(f'PASS: native {mode}, clean process exit, saved session and relaunch', flush=True)


if __name__ == '__main__':
    main()
