#!/usr/bin/env python3
"""Verify native sleep and preferences across two fresh, isolated app processes."""
import json
import os
import sqlite3
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix="axolotl-sleep-restart-") as directory:
        env = dict(os.environ, AXOLOTL_DATA_DIR=directory)
        for args in [[], ["--sleep-restore"]]:
            result = subprocess.run(
                [binary, "--smoke-sleep-session"] + args,
                env=env, capture_output=True, text=True, timeout=35,
            )
            assert result.returncode == 0, (result.stdout, result.stderr)
            records = [line for line in result.stdout.splitlines()
                       if line.startswith("AXOLOTL_SMOKE ")]
            assert len(records) == 1, (result.stdout, result.stderr)
            print(records[0], flush=True)
            screenshots = list((Path(directory) / "sleep").glob("*.png"))
            assert bool(screenshots) == (not args), "Screenshot cache lifecycle failed"
            with sqlite3.connect(Path(directory) / "app.sqlite3") as db:
                state = {key: json.loads(value) for key, value in
                         db.execute("SELECT key,json FROM app_state")}
                assert state["settings"]["auto_sleep"] is False, state
                assert state["settings"]["sleep_minutes"] == 7, state
                session = state["session"]
                ids = {str(tab["id"]) for space in session["spaces"]
                       for pane in space["panes"] for tab in pane["tabs"]}
                ids.update(str(pin["tab"]["id"]) for pin in session["pinned_tabs"])
                assert set(state["sleep"]) <= ids, "Sleep metadata references missing tabs"
                assert any(entry["keep_alive"] for entry in state["sleep"].values()), state
                assert any(entry["sleeping"] for entry in state["sleep"].values()) == (not args), state
            for legacy in ["session.json", "settings.json", "sleep/state.json"]:
                assert not (Path(directory) / legacy).exists(), f"Legacy writer still active: {legacy}"


if __name__ == "__main__":
    main()
