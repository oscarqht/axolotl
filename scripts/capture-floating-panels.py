#!/usr/bin/env python3
"""Capture native workspace views at runtime bounds (without GPUI's spaces header).

Transparent WKWebView layers are absent from AppKit's NSView cache, so their
native snapshots are composited back at the recorded bounds. Requires Pillow.
"""
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from PIL import Image, ImageDraw

binary = str(Path(sys.argv[1]).resolve())
output = Path(sys.argv[2]).resolve()
contrast = "--contrast-corners" in sys.argv[3:]
with tempfile.TemporaryDirectory(prefix="axolotl-native-capture-") as directory:
    raw = Path(directory) / "native.png"
    env = dict(os.environ, AXOLOTL_DATA_DIR=str(Path(directory) / "profile"))
    args = [binary, "--floating-session-probe=seed", f"--smoke-floating-snapshot={raw}"]
    if contrast:
        args.append("--smoke-contrast-corners")
    result = subprocess.run(args, env=env,
                            capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, (result.stdout, result.stderr)
    manifest = json.loads(Path(f"{raw}.json").read_text())
    base = Image.open(raw).convert("RGBA")
    sx, sy = base.width / manifest["width"], base.height / manifest["height"]
    corner_checks = []
    for surface in manifest["surfaces"]:
        bounds = surface["bounds"]
        overlay = Image.open(surface["file"]).convert("RGBA")
        if contrast and surface["file"].endswith(".header.png"):
            # Read the actual WebKit frame snapshot before combining any layers.
            # Pixels wholly outside the rounded outline must be transparent.
            radius = 10 * overlay.width / bounds["width"]
            for x in range(math.ceil(radius)):
                for y in range(math.ceil(radius)):
                    if math.hypot(radius - x - .5, radius - y - .5) <= radius + 1:
                        continue  # Ignore antialiasing at the curve itself.
                    for px, py in [(x, y), (overlay.width - 1 - x, y),
                                   (x, overlay.height - 1 - y),
                                   (overlay.width - 1 - x, overlay.height - 1 - y)]:
                        assert overlay.getpixel((px, py))[3] == 0, ("Frame paints outside rounded outline", surface["file"], px, py, overlay.getpixel((px, py)))
            corner_checks.append(bounds)
        dimensions = (round(bounds["width"] * sx), round(bounds["height"] * sy))
        if overlay.size != dimensions:
            overlay = overlay.resize(dimensions, Image.Resampling.LANCZOS)
        base.alpha_composite(overlay, (round(bounds["x"] * sx), round(bounds["y"] * sy)))
    if contrast:
        assert corner_checks, "No floating frame was captured"
        # Native NSImage color profiles can transform CSS RGB values. Compare
        # with the captured live backdrop, rather than assuming a color space.
        backdrop = base.getpixel((round(100 * sx), round(100 * sy)))[:3]
        assert backdrop[2] > backdrop[0] + 20 and backdrop[0] > backdrop[1] + 15, ("Contrast backdrop missing", backdrop)
        for bounds in corner_checks:
            left, top = round(bounds["x"] * sx), round(bounds["y"] * sy)
            width, height = round(bounds["width"] * sx), round(bounds["height"] * sy)
            # Check all four corners against the contrasting live tiled page.
            for dx, dy in [(0, 0), (1, 1), (2, 2)]:
                dx, dy = round(dx * sx), round(dy * sy)
                for x, y in [(left + dx, top + dy), (left + width - 1 - dx, top + dy),
                             (left + dx, top + height - 1 - dy),
                             (left + width - 1 - dx, top + height - 1 - dy)]:
                    pixel = base.getpixel((x, y))[:3]
                    # Allow only one 8-bit level for native color-profile dithering.
                    assert max(abs(a - b) for a, b in zip(pixel, backdrop)) <= 1, ("Panel background escapes a rounded corner", x, y, pixel, backdrop)
        # Enlarged, unretouched crops make the four outside curves easy to inspect.
        bounds = corner_checks[0]
        left, top = round(bounds["x"] * sx), round(bounds["y"] * sy)
        right, bottom = round((bounds["x"] + bounds["width"]) * sx), round((bounds["y"] + bounds["height"]) * sy)
        mx, my, ix, iy = round(8 * sx), round(8 * sy), round(16 * sx), round(16 * sy)
        crops = [(left-mx,top-my,left+ix,top+iy), (right-ix,top-my,right+mx,top+iy),
                 (left-mx,bottom-iy,left+ix,bottom+my), (right-ix,bottom-iy,right+mx,bottom+my)]
        sheet = Image.new("RGB", (548,588), "white")
        labels = ["Top-left", "Top-right", "Bottom-left", "Bottom-right"]
        for i, rect in enumerate(crops):
            x, y = 12 + (i % 2) * 268, 30 + (i // 2) * 294
            ImageDraw.Draw(sheet).text((x,y-20),labels[i],fill="#243240")
            sheet.paste(base.crop(rect).convert("RGB").resize((256,256),Image.Resampling.NEAREST),(x,y))
        sheet.save(output.with_name(output.stem + "-corners.png"))
        print("PASS: native frame transparency and all four workspace corners")
    base = base.crop((0, round(44 * sy), base.width, base.height))
    base.save(output)
    print(output)
