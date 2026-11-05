#!/usr/bin/env python3
"""Build the original app with today's identical performance fixture/timers.
Usage: python3 scripts/build-performance-baseline.py /tmp/axolotl-baseline
Uses a separate target directory; does not change a checkout or user profile.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('output', type=Path)
parser.add_argument('--ref', default='ea3b725')
parser.add_argument('--prepare-only', action='store_true')
args = parser.parse_args()
repo = Path(__file__).resolve().parent.parent
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
if any(output.iterdir()):
    parser.error('output directory must be empty')
source = output / 'source'
source.mkdir()
revision = subprocess.check_output(['git', 'rev-parse', args.ref], cwd=repo, text=True).strip()
archive = output / 'source.tar'
subprocess.run(['git', 'archive', revision, '-o', str(archive)], cwd=repo, check=True)
subprocess.run(['tar', '-xf', str(archive), '-C', str(source)], check=True)
archive.unlink()
rust = source / 'crates/desktop/src'
shutil.copyfile(repo / 'crates/desktop/src/smoke_performance.rs', rust / 'smoke_performance.rs')
patches = [
    ('main.rs', 'mod smoke_panels;', 'mod smoke_panels;\n#[cfg(feature = "smoke")]\nmod smoke_performance;'),
    ('smoke.rs', '    wait_title(&host, first, "New tab · Axolotl", cx).await?;', '    wait_title(&host, first, "New tab · Axolotl", cx).await?;\n    if std::env::args().any(|arg| arg == "--smoke-performance") { return crate::smoke_performance::run(view,cx).await; }'),
    ('app.rs', '    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {', '    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {\n        #[cfg(feature="smoke")]\n        let _timing = crate::smoke_performance::time("poll");'),
    ('app.rs', '        self.saver.tick(&self.model, Instant::now());', '        {\n        #[cfg(feature="smoke")]\n        let _timing = crate::smoke_performance::time("persistence");\n        self.saver.tick(&self.model, Instant::now());\n        }'),
    ('panel_controls.rs', '    pub fn prepare_views(&mut self, window: &mut Window, cx: &gpui::App) {', '    pub fn prepare_views(&mut self, window: &mut Window, cx: &gpui::App) {\n        #[cfg(feature="smoke")]\n        let _timing = crate::smoke_performance::time("prepare_views");'),
    ('floating_bar.rs', '    pub fn new(window: &Window, cx: &gpui::App) -> Result<Self> {', '    pub fn new(window: &Window, cx: &gpui::App) -> Result<Self> {\n        #[cfg(feature="smoke")]\n        let _timing = crate::smoke_performance::time("bar_creation");'),
]
for name, old, new in patches:
    path = rust / name
    text = path.read_text()
    if text.count(old) != 1:
        raise RuntimeError(f'baseline instrumentation anchor changed: {name}')
    path.write_text(text.replace(old, new, 1))
(output / 'baseline.json').write_text(json.dumps({'revision': revision, 'profile': 'release', 'features': ['smoke']}, indent=2) + '\n')
if not args.prepare_only:
    env = dict(os.environ, CARGO_TARGET_DIR=str(output / 'target'))
    subprocess.run(['cargo', 'build', '--manifest-path', str(source / 'Cargo.toml'), '-p', 'axolotl', '--release', '--locked', '--features', 'smoke'], env=env, check=True)
    binary = 'axolotl.exe' if os.name == 'nt' else 'axolotl'
    shutil.copy2(output / 'target/release' / binary, output / 'baseline')
print(output)
