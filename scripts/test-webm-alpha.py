#!/usr/bin/env python3
"""Launch Axolotl's real WKWebView regression against a localhost fixture."""
import functools
import http.server
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading

root = Path(__file__).resolve().parents[1]
fixture = root / 'crates/desktop/assets/video-fixture'
real_assets = os.environ.get('AXOLOTL_REAL_VIDEO_DIR')
class Handler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        if real_assets and path.startswith('/real-assets/'):
            name = path.split('?', 1)[0].rsplit('/', 1)[-1]
            return str(Path(real_assets) / name)
        return super().translate_path(path)

    def log_message(self, *_args):
        pass

test_host = os.environ.get('AXOLOTL_VIDEO_TEST_HOST', '127.0.0.1')
with http.server.ThreadingHTTPServer((test_host, 0), functools.partial(Handler, directory=str(fixture))) as server:
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        binary = str(Path(sys.argv[1]).resolve())
        output = Path(sys.argv[2] if len(sys.argv) > 2 else '/tmp/axolotl-video-validation')
        output.mkdir(parents=True, exist_ok=True)
        for baseline in (False, True):
            with tempfile.TemporaryDirectory(prefix='axolotl-video-') as profile:
                env = {**os.environ, 'AXOLOTL_DATA_DIR': profile,
                       'AXOLOTL_VIDEO_TEST_URL': f'http://{test_host}:{server.server_port}/',
                       'AXOLOTL_VIDEO_CANVAS_URL': f'http://{test_host}:{server.server_port}/canvas.html' + ('?real=1' if real_assets else ''),
                       'AXOLOTL_VIDEO_SNAPSHOT': str(Path(profile) / 'snapshot.png')}
                args = [binary, '--smoke-test', '--smoke-video']
                if baseline:
                    args.append('--smoke-video-baseline')
                result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=180)
                print(result.stdout)
                if result.returncode:
                    print(result.stderr, file=sys.stderr)
                    raise SystemExit(result.returncode)
                snapshot = Path(profile) / 'snapshot.png'
                if not snapshot.is_file() or snapshot.stat().st_size == 0:
                    raise RuntimeError('Native WKWebView snapshot was not written')
                shutil.copyfile(snapshot, output / ('before.png' if baseline else 'after.png'))
                if not baseline:
                    shutil.copyfile(str(snapshot) + '.textures.png', output / 'textures.png')
    finally:
        server.shutdown()
