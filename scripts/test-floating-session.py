#!/usr/bin/env python3
"""Run separate native processes to verify floating geometry and visibility restore."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix="axolotl-floating-session-") as directory:
    env = dict(os.environ, AXOLOTL_DATA_DIR=directory)

    def run(stage):
        result = subprocess.run([binary, f"--floating-session-probe={stage}"],
                                env=env, capture_output=True, text=True, timeout=45)
        assert result.returncode == 0, (result.stdout, result.stderr)
        records = [json.loads(line.removeprefix("AXOLOTL_FLOATING_SESSION "))
                   for line in result.stdout.splitlines()
                   if line.startswith("AXOLOTL_FLOATING_SESSION ")]
        assert len(records) == 1, (result.stdout, result.stderr)
        return records[0]

    before = run("seed")
    with sqlite3.connect(Path(directory) / "app.sqlite3") as db:
        saved = json.loads(db.execute("SELECT json FROM app_state WHERE key='session'").fetchone()[0])
    assert saved == before["workspace"], "Native session saver lost state"
    after = run("restore")
    assert before == after, "Panels, tabs, edge/order/pin/geometry or visibility changed on restart"
    print("PASS: six panels, four edges, separate chips, free geometry, pin state, two spaces, native visibility and SQLite restart restoration")
