#!/usr/bin/env python3
"""Verify last edits flush on native close and quit before the next session poll."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
for action in ['close', 'quit']:
    with tempfile.TemporaryDirectory(prefix=f'axolotl-exit-{action}-') as directory:
        for stage in [action, 'restore']:
            result = subprocess.run([binary, f'--persistence-exit-probe={stage}'],
                                    env=dict(os.environ, AXOLOTL_DATA_DIR=directory),
                                    capture_output=True, text=True, timeout=30)
            assert result.returncode == 0, (result.stdout, result.stderr)
            assert f'AXOLOTL_PERSISTENCE_EXIT {stage}' in result.stdout, (result.stdout, result.stderr)
            with sqlite3.connect(Path(directory) / 'app.sqlite3') as db:
                state = {k: json.loads(v) for k, v in db.execute('SELECT key,json FROM app_state')}
                tab = state['session']['spaces'][-1]['panes'][0]['tabs'][0]
                assert tab['custom_title'] == 'Last edit before exit', state
                assert state['sleep'][str(tab['id'])]['keep_alive'], state
                assert state['settings']['search_engine'] == 'bing', state
        print(f'PASS: edit and immediate native {action}, then restore', flush=True)
