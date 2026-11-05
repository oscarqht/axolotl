#!/usr/bin/env python3
"""Regenerate our own tiny fixture artwork. Requires Pillow and FFmpeg/libvpx."""
from pathlib import Path
import subprocess
import tempfile
from PIL import Image, ImageDraw

output = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix='axolotl-alpha-frames-') as temporary:
    frames = Path(temporary)
    for index in range(24):
        image = Image.new('RGBA', (160, 120))
        draw = ImageDraw.Draw(image)
        x = 30 + int(35 * index / 23)
        draw.ellipse((x, 25, x + 60, 85), fill=(255, 72, 140, 255))
        draw.rectangle((110, 30, 140, 90), fill=(30, 180, 240, 128))
        image.save(frames / f'{index:03}.png')
    for name, codec in (
        ('alpha-vp9.webm', ['-c:v', 'libvpx-vp9', '-lossless', '1', '-pix_fmt', 'yuva420p']),
        ('alpha-vp8.webm', ['-c:v', 'libvpx', '-auto-alt-ref', '0', '-pix_fmt', 'yuva420p']),
        ('ordinary.mp4', ['-c:v', 'libx264', '-pix_fmt', 'yuv420p']),
    ):
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-framerate', '12', '-i', str(frames / '%03d.png'), *codec, str(output / name)], check=True)
