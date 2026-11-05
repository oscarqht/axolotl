#!/usr/bin/env python3
"""Verify native sleep and preferences across two fresh, isolated app processes."""
import os
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


if __name__ == "__main__":
    main()
