#!/usr/bin/env python3
"""Capture native workspace views at runtime bounds (without GPUI's spaces header).

Transparent WKWebView layers are absent from AppKit's NSView cache, so their
native snapshots are composited back at the recorded bounds. Requires Pillow.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image

binary = str(Path(sys.argv[1]).resolve())
output = Path(sys.argv[2]).resolve()
with tempfile.TemporaryDirectory(prefix="axolotl-native-capture-") as directory:
    raw = Path(directory) / "native.png"
    env = dict(os.environ, AXOLOTL_DATA_DIR=str(Path(directory) / "profile"))
    result = subprocess.run([binary, "--floating-session-probe=seed",
                             f"--smoke-floating-snapshot={raw}"], env=env,
                            capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, (result.stdout, result.stderr)
    manifest = json.loads(Path(f"{raw}.json").read_text())
    base = Image.open(raw).convert("RGBA")
    sx, sy = base.width / manifest["width"], base.height / manifest["height"]
    for surface in manifest["surfaces"]:
        bounds = surface["bounds"]
        overlay = Image.open(surface["file"]).convert("RGBA")
        dimensions = (round(bounds["width"] * sx), round(bounds["height"] * sy))
        if overlay.size != dimensions:
            overlay = overlay.resize(dimensions, Image.Resampling.LANCZOS)
        base.alpha_composite(overlay, (round(bounds["x"] * sx), round(bounds["y"] * sy)))
    base = base.crop((0, round(44 * sy), base.width, base.height))
    base.save(output)
    print(output)
